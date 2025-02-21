use crate::cmd::command::{CommandError, CommandKiller, ExecutableCommand, QoveryCommand};
use crate::io_models::models::CpuArchitecture;
use itertools::Itertools;
use once_cell::sync::Lazy;
use std::cmp::max;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Duration;
use std::{fs, thread};
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum DockerError {
    #[error("Docker Invalid configuration: {raw_error_message:?}")]
    InvalidConfig { raw_error_message: String },

    #[error("Docker terminated with an unknown error: {raw_error:?}")]
    ExecutionError { raw_error: std::io::Error },

    #[error("Docker terminated with a non success exit status code: {exit_status:?}")]
    ExitStatusError { exit_status: ExitStatus },

    #[error("Docker aborted due to user cancel request: {raw_error_message:?}")]
    Aborted { raw_error_message: String },

    #[error("Docker command terminated due to timeout: {raw_error_message:?}")]
    Timeout { raw_error_message: String },
}

impl DockerError {
    pub fn is_aborted(&self) -> bool {
        matches!(self, DockerError::Aborted { .. })
    }
}

// Docker login when launched in parallel can mess up ~/.docker/config.json
// We use a mutex that will force serialization of logins in order to avoid that
// Mostly use for CI/Test when all test start in parallel and it the login phase at the same time
static LOGIN_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static DEFAULT_BUILDER_NAME: &str = "qovery-engine";

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Architecture {
    AMD64,
    ARM64,
}

impl Architecture {
    pub(crate) fn to_platform(self) -> &'static str {
        match self {
            Architecture::AMD64 => "linux/amd64",
            Architecture::ARM64 => "linux/arm64",
        }
    }
}

impl FromStr for Architecture {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "amd64" | "AMD64" => Ok(Architecture::AMD64),
            "arm64" | "ARM64" => Ok(Architecture::ARM64),
            _ => Err(format!("unknown architecture: {s}")),
        }
    }
}

impl From<&CpuArchitecture> for Architecture {
    fn from(value: &CpuArchitecture) -> Self {
        match value {
            CpuArchitecture::AMD64 => Architecture::AMD64,
            CpuArchitecture::ARM64 => Architecture::ARM64,
        }
    }
}

impl Display for Architecture {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Architecture::AMD64 => f.write_str("amd64"),
            Architecture::ARM64 => f.write_str("arm64"),
        }
    }
}

#[derive(Debug, Clone)]
enum ImageId {
    #[allow(dead_code)]
    Digest(String),
    Tags(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct ContainerImage {
    pub registry: Url,
    pub name: String,
    id: ImageId,
}

impl ContainerImage {
    pub fn new(registry: Url, name: String, tags: Vec<String>) -> Self {
        assert!(!tags.is_empty(), "cannot create a container image without tags");

        ContainerImage {
            registry,
            name,
            id: ImageId::Tags(tags),
        }
    }

    fn _new_for_digest(registry: Url, name: String, digest: String) -> Self {
        ContainerImage {
            registry,
            name,
            id: ImageId::Digest(digest),
        }
    }

    fn host_with_port(&self) -> String {
        let host = if let Some(port) = self.registry.port() {
            format!("{}:{}", self.registry.host_str().unwrap_or_default(), port)
        } else {
            self.registry.host_str().unwrap_or_default().to_string()
        };

        host
    }

    pub fn image_names(&self) -> Vec<String> {
        let host = self.host_with_port();
        match &self.id {
            ImageId::Digest(digest) => vec![format!("{}/{}@{}", host, &self.name, digest)],
            ImageId::Tags(tags) => tags
                .iter()
                .map(|tag| format!("{}/{}:{}", host, &self.name, tag))
                .collect(),
        }
    }

    pub fn image_name(&self) -> String {
        self.image_names().remove(0)
    }

    pub fn repository_with_host(&self) -> String {
        format!("{}/{}", self.host_with_port(), self.name)
    }
}

#[derive(Debug, Clone)]
enum BuilderLocation {
    Local,
    Kubernetes {
        namespace: String,
        builder_prefix: String,
        supported_architectures: Vec<Architecture>,
        enable_rootless: bool,
    },
}

impl BuilderLocation {
    fn supported_architectures(&self) -> &[Architecture] {
        match self {
            BuilderLocation::Local => &[],
            BuilderLocation::Kubernetes {
                supported_architectures,
                ..
            } => supported_architectures,
        }
    }
}

#[derive(Debug)]
pub struct Docker {
    config_path: TempDir,
    builder_location: BuilderLocation,
    socket_location: Option<Url>,
    common_envs: Vec<(String, String)>,
}

pub struct BuilderHandle {
    config_path: PathBuf,
    pub nb_builder: NonZeroUsize,
    pub builder_name: Option<String>,
}

impl Drop for BuilderHandle {
    fn drop(&mut self) {
        if let Some(builder_name) = &self.builder_name {
            let _ = docker_exec(
                &[
                    "--config",
                    self.config_path.to_str().unwrap_or(""),
                    "buildx",
                    "rm",
                    "-f",
                    builder_name,
                ],
                &[],
                &mut |_| {},
                &mut |_| {},
                &CommandKiller::never(),
            );
        }
    }
}

impl Clone for Docker {
    fn clone(&self) -> Self {
        Self {
            config_path: TempDir::with_prefix("docker-").unwrap(),
            builder_location: self.builder_location.clone(),
            socket_location: self.socket_location.clone(),
            common_envs: self.common_envs.clone(),
        }
    }
}

impl Docker {
    pub fn new(socket_location: Option<Url>) -> Result<Self, DockerError> {
        let Ok(tmp_dir) = TempDir::with_prefix("docker-") else {
            return Err(DockerError::InvalidConfig {
                raw_error_message: "Cannot create temporary directory to store docker config".to_string(),
            });
        };

        let mut docker = Docker {
            config_path: tmp_dir,
            builder_location: BuilderLocation::Local,
            socket_location,
            common_envs: vec![("DOCKER_BUILDKIT".to_string(), "1".to_string())],
        };

        // Override DOCKER_HOST if we use a TCP socket
        if let Some(socket_location) = &docker.socket_location {
            docker
                .common_envs
                .push(("DOCKER_HOST".to_string(), socket_location.to_string()))
        }

        // First check that the buildx plugin is correctly installed
        let args = vec!["buildx", "version"];
        let buildx_cmd_exist = docker_exec(
            &args,
            &docker.get_all_envs(&[]),
            &mut |_| {},
            &mut |_| {},
            &CommandKiller::never(),
        );
        if buildx_cmd_exist.is_err() {
            return Err(DockerError::InvalidConfig {
                raw_error_message: "Docker buildx plugin for buildkit is not correctly installed".to_string(),
            });
        }

        Ok(docker)
    }

    pub fn new_with_local_builder(socket_location: Option<Url>) -> Result<Self, DockerError> {
        let docker = Self::new(socket_location)?;

        // In order to be able to use --cache-from --cache-to for buildkit,
        // we need to create our specific builder, which is not the default one (aka: the docker one).
        // Reference doc https://docs.docker.com/engine/reference/commandline/buildx_create
        let args = vec![
            "--config",
            docker.config_path.path().to_str().unwrap_or(""),
            "buildx",
            "create",
            "--name",
            DEFAULT_BUILDER_NAME,
            "--buildkitd-flags",
            "--debug",
            "--driver-opt",
            "network=host",
            "--bootstrap",
            "--use",
        ];
        let _ = docker_exec(
            &args,
            &docker.get_all_envs(&[]),
            &mut |_| {},
            &mut |_| {},
            &CommandKiller::never(),
        );

        Ok(docker)
    }

    pub fn new_with_kube_builder(
        socket_location: Option<Url>,
        supported_architectures: &[Architecture],
        namespace: String,
        builder_prefix: String,
        args: Vec<(String, String)>,
        enable_rootless: bool,
    ) -> Result<Self, DockerError> {
        let mut docker = Self::new(socket_location)?;

        docker.builder_location = BuilderLocation::Kubernetes {
            namespace,
            builder_prefix,
            supported_architectures: supported_architectures.iter().dedup().cloned().collect_vec(),
            enable_rootless,
        };
        docker.common_envs.extend(args);

        Ok(docker)
    }

    pub fn spawn_builder(
        &self,
        exec_id: &str,
        builder_name: &str,
        nb_builder: NonZeroUsize,
        requested_architectures: &[Architecture],
        (cpu_request_milli, cpu_limit_milli): (u32, u32),
        (memory_request_gib, memory_limit_gib): (u32, u32),
        ephemeral_storage_gib: Option<u32>,
        should_abort: &CommandKiller,
        http_registries: &[&str],
        tls_invalid_registries: &[&str],
        bootstrap_builder: bool,
    ) -> Result<BuilderHandle, DockerError> {
        match &self.builder_location {
            // For local builder, we have at max 1 builder available
            BuilderLocation::Local => Ok(BuilderHandle {
                config_path: self.config_path.path().to_path_buf(),
                nb_builder: NonZeroUsize::new(1).unwrap(),
                builder_name: None,
            }),
            BuilderLocation::Kubernetes {
                namespace,
                builder_prefix,
                supported_architectures,
                enable_rootless,
            } => {
                let available_architectures = requested_architectures
                    .iter()
                    .filter(|arch| supported_architectures.contains(arch))
                    .count();

                if available_architectures != requested_architectures.len() {
                    return Err(DockerError::InvalidConfig {
                        raw_error_message: format!(
                            "Some requested architectures are not supported by current docker builder. Available architectures are {supported_architectures:?} while requested are {requested_architectures:?}."
                        ),
                    });
                }

                // We create build handle here to force the drop to run if some operation fail
                let builder_name = format!("qovery-{}", builder_name);
                let build_handle = BuilderHandle {
                    config_path: self.config_path.path().to_path_buf(),
                    nb_builder,
                    builder_name: Some(builder_name.clone()),
                };

                info!("docker spawn builder {:?} {:?}", builder_name, requested_architectures);
                let buildkitd_cfg_arg = {
                    let cfg_path = self.config_path.path().join("buildkitd.toml");
                    let mut cfg_file = String::new();
                    for http in http_registries {
                        cfg_file.push_str(&format!("[registry.\"{}\"]\n  http = true\n", http));
                    }
                    for tls in tls_invalid_registries {
                        cfg_file.push_str(&format!("[registry.\"{}\"]\n  insecure = true\n", tls));
                    }
                    info!("Docker buildkitd config {}", &cfg_file);
                    let _ = fs::write(&cfg_path, cfg_file);

                    format!("--buildkitd-config={}", cfg_path.to_string_lossy())
                };

                // Reference doc https://docs.docker.com/engine/reference/commandline/buildx_create
                for (ix, arch) in requested_architectures.iter().enumerate() {
                    let mut node_name = format!("{builder_prefix}{exec_id}-{arch}");
                    node_name.truncate(60);
                    let node_name = node_name.trim_matches(|c: char| !c.is_alphanumeric());
                    let platform = format!("linux/{arch}");
                    let mut driver_opt = format!(
                        concat!(
                            "--driver-opt=",
                            "\"namespace={}\",",
                            "\"replicas={}\",",
                            "\"loadbalance=random\",",
                            "\"nodeselector=kubernetes.io/arch={}\",",
                            "\"tolerations=key=node.kubernetes.io/not-ready,effect=NoExecute,operator=Exists,tolerationSeconds=10800\",",
                            "\"labels=qovery.com/no-kill=true,qovery.com/is-builder=true\",",
                            "\"requests.cpu={}m\",",
                            "\"limits.cpu={}m\",",
                            "\"requests.memory={}Gi\",",
                            "\"limits.memory={}Gi\""
                        ),
                        namespace,
                        nb_builder,
                        arch,
                        cpu_request_milli,
                        cpu_limit_milli,
                        memory_request_gib,
                        memory_limit_gib
                    );
                    if *enable_rootless {
                        driver_opt.push_str(",\"rootless=true\"");
                    }
                    if let Some(ephemeral_storage) = ephemeral_storage_gib {
                        driver_opt.push_str(&format!(",\"requests.ephemeral-storage={}Gi\"", ephemeral_storage));
                    }

                    let mut args = vec![
                        "--config",
                        self.config_path.path().to_str().unwrap_or(""),
                        "buildx",
                        "create",
                        if ix == 0 { "" } else { "--append" },
                        "--name",
                        &builder_name,
                        "--platform",
                        &platform,
                        "--node",
                        node_name,
                        "--buildkitd-flags",
                        "--debug",
                        &buildkitd_cfg_arg,
                        "--driver=kubernetes",
                        &driver_opt,
                    ];
                    if bootstrap_builder {
                        args.push("--bootstrap");
                    }

                    docker_exec(
                        &args,
                        &self.get_all_envs(&[]),
                        &mut |line| info!("{}", line),
                        &mut |line| info!("{}", line),
                        should_abort,
                    )?;
                }

                Ok(build_handle)
            }
        }
    }

    pub fn socket_url(&self) -> &Option<Url> {
        &self.socket_location
    }

    fn get_all_envs<'a>(&'a self, envs: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all_envs: Vec<(&str, &str)> = self.common_envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all_envs.append(&mut envs.to_vec());

        all_envs
    }

    // TODO(benjaminch): should be removed, it's very dirty but allows builds to connect to Google Artifact Registry
    pub fn login_artifact_registry(
        &self,
        registry: &Url,
        google_client_email: &str,
        google_creds: &str,
    ) -> Result<(), DockerError> {
        // Save creds to file as CLI cannot ingest it otherwise ...
        let Ok(tmp_dir) = TempDir::with_prefix("gcp-credentials-") else {
            return Err(DockerError::InvalidConfig {
                raw_error_message: "Cannot create temporary directory to store google credentials".to_string(),
            });
        };
        let gcp_credentials_file_path =
            format!("{}/google-credentials.json", tmp_dir.path().to_str().unwrap_or_default());
        match fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&gcp_credentials_file_path)
            .map_err(|_e| DockerError::InvalidConfig {
                raw_error_message: "Cannot create google credentials file to connect".to_string(),
            }) {
            Ok(mut f) => f
                .write(google_creds.as_bytes())
                .map_err(|_e| DockerError::InvalidConfig {
                    raw_error_message: "Cannot write into google credentials file to connect".to_string(),
                }),
            Err(e) => return Err(e),
        }?;

        QoveryCommand::new(
            "gcloud",
            &[
                "auth",
                "activate-service-account",
                google_client_email,
                format!("--key-file={}", gcp_credentials_file_path).as_str(),
            ],
            &[],
        )
        .exec()
        .map_err(|_e| DockerError::InvalidConfig {
            raw_error_message: "Cannot connected to gcloud".to_string(),
        })?;

        self.login(registry)
    }

    pub fn login(&self, registry: &Url) -> Result<(), DockerError> {
        let username = match urlencoding::decode(registry.username()) {
            Ok(decoded_username) => decoded_username,
            Err(err) => {
                return Err(DockerError::InvalidConfig {
                    raw_error_message: format!("Cannot decode username due to: {}", err),
                });
            }
        };
        info!("Docker login {} as user {}", registry, username);

        let password = registry
            .password()
            .and_then(|password| urlencoding::decode(password).ok())
            .unwrap_or_default();
        let args = vec![
            "--config",
            self.config_path.path().to_str().unwrap_or(""),
            "login",
            registry.host_str().unwrap_or_default(),
            "-u",
            &username,
            "-p",
            &password,
        ];

        let _lock = LOGIN_LOCK.lock().unwrap();
        docker_exec(
            &args,
            &self.get_all_envs(&[]),
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::never(),
        )?;

        Ok(())
    }

    pub fn does_image_exist_locally(&self, image: &ContainerImage) -> Result<bool, DockerError> {
        info!("Docker check locally image exist {:?}", image);

        let ret = docker_exec(
            &[
                "--config",
                self.config_path.path().to_str().unwrap_or(""),
                "image",
                "inspect",
                &image.image_name(),
            ],
            &self.get_all_envs(&[]),
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::never(),
        );

        Ok(ret.is_ok())
    }

    pub fn does_image_exist_remotely(&self, image: &ContainerImage) -> Result<bool, DockerError> {
        info!("Docker check remotely image exist {:?}", image);

        let builder = self.configure_builder_for_http_registries(image);
        let image_name = image.image_name();
        let mut args = vec![
            "--config",
            self.config_path.path().to_str().unwrap_or(""),
            "buildx",
            "imagetools",
            "inspect",
            &image_name,
        ];
        if let Some(builder_name) = &builder.as_ref().and_then(|b| b.builder_name.as_deref()) {
            args.push("--builder");
            args.push(builder_name)
        }

        let ret = docker_exec(
            &args,
            &self.get_all_envs(&[]),
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::never(),
        );

        match ret {
            Ok(_) => Ok(true),
            Err(DockerError::ExitStatusError { .. }) => Ok(false),
            Err(err) => Err(err),
        }
    }

    pub fn pull<Stdout, Stderr>(
        &self,
        image: &ContainerImage,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker pull {:?}", image);

        docker_exec(
            &[
                "--config",
                self.config_path.path().to_str().unwrap_or(""),
                "pull",
                &image.image_name(),
            ],
            &self.get_all_envs(&[]),
            stdout_output,
            stderr_output,
            should_abort,
        )
    }

    pub fn build<Stdout, Stderr>(
        &self,
        builder_name: &Option<&str>,
        dockerfile: &Path,
        context: &Path,
        image_to_build: &ContainerImage,
        build_args: &[(&str, &str)],
        cache: &ContainerImage,
        push_after_build: bool,
        architectures: &[Architecture],
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
        target_build_stage: Option<&String>,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        // Do some checks
        if !dockerfile.is_file() {
            return Err(DockerError::InvalidConfig {
                raw_error_message: format!("provided dockerfile `{dockerfile:?}` is not a valid file"),
            });
        }

        if !context.is_dir() {
            return Err(DockerError::InvalidConfig {
                raw_error_message: format!("provided docker build context `{context:?}` is not a valid directory"),
            });
        }

        self.build_with_buildkit(
            builder_name,
            dockerfile,
            context,
            image_to_build,
            build_args,
            cache,
            push_after_build,
            architectures,
            stdout_output,
            stderr_output,
            should_abort,
            target_build_stage,
        )
    }

    fn build_with_buildkit<Stdout, Stderr>(
        &self,
        builder_name: &Option<&str>,
        dockerfile: &Path,
        context: &Path,
        image_to_build: &ContainerImage,
        build_args: &[(&str, &str)],
        cache: &ContainerImage,
        push_after_build: bool,
        architectures: &[Architecture],
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
        target_build_stage: Option<&String>,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker buildkit build {:?}", image_to_build.image_name());

        let mut args_string: Vec<String> = vec![
            "--config".to_string(),
            self.config_path.path().to_str().unwrap_or("").to_string(),
            "buildx".to_string(),
            "build".to_string(),
            if let Some(builder_name) = builder_name {
                format!("--builder={}", builder_name)
            } else {
                format!("--builder={}", DEFAULT_BUILDER_NAME)
            },
            "--progress=plain".to_string(),
            if push_after_build {
                "--output=type=registry".to_string() // tell buildkit to push image to registry
            } else {
                "--output=type=docker".to_string() // tell buildkit to load the image into docker after build
            },
            "--cache-from".to_string(),
            format!("type=registry,ref={}", cache.image_name()),
            "-f".to_string(),
            dockerfile.to_str().unwrap_or_default().to_string(),
        ];

        if let Some(target_build_stage) = target_build_stage {
            args_string.push("--target".to_owned());
            args_string.push(target_build_stage.to_string());
        }

        if push_after_build {
            args_string.push("--cache-to".to_string());
            args_string.push(format!(
                "type=registry,mode=max,image-manifest=true,oci-mediatypes=true,ref={}",
                cache.image_name()
            ));
        }

        // Build for all requested architectures, if empty build for the current architecture the engine is running on
        if !architectures.is_empty() {
            args_string.push(format!(
                "--platform={}",
                architectures.iter().map(|arch| arch.to_platform()).join(",")
            ));
        };

        for image_name in image_to_build.image_names() {
            args_string.push("--tag".to_string());
            args_string.push(image_name.to_string())
        }

        for (k, v) in build_args {
            args_string.push("--build-arg".to_string());
            args_string.push(format!("{k}={v}"));
        }
        args_string.push(context.to_str().unwrap_or_default().to_string());

        // Hack
        // Sometimes, the build can fail with a transient error, we need to retry, for stability ...
        let mut nb_retry = 3;
        let started_at = std::time::Instant::now();
        let ret = loop {
            let mut transient_error = false;
            let ret = {
                let mut stderr_output = |line: String| {
                    if line.contains("ERROR: listing workers for Build")
                        || line.contains("use of closed network connection")
                        || line.contains("i/o timeout")
                    {
                        transient_error = true;
                    }

                    stderr_output(line);
                };
                docker_exec(
                    &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
                    &self.get_all_envs(&[]),
                    stdout_output,
                    &mut stderr_output,
                    should_abort,
                )
            };

            if ret.is_err() && transient_error && should_abort.should_abort().is_none() {
                if nb_retry == 0 && started_at.elapsed() > Duration::from_secs(60 * 3) {
                    info!(
                        "Docker buildkit build failed with a transient error, but we already retried for too long, aborting ..."
                    );
                    break ret;
                }

                nb_retry = max(nb_retry - 1, 0);
                info!("Docker buildkit build failed with a transient error, retrying ...");
                thread::sleep(Duration::from_secs(1));
                continue;
            }

            break ret;
        };

        ret
    }

    pub fn push<Stdout, Stderr>(
        &self,
        image: &ContainerImage,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker push {:?}", image);
        for image_name in image.image_names() {
            let args = vec![
                "--config",
                self.config_path.path().to_str().unwrap_or(""),
                "push",
                image_name.as_str(),
            ];
            docker_exec(&args, &self.get_all_envs(&[]), stdout_output, stderr_output, should_abort)?
        }

        Ok(())
    }

    pub fn tag<Stdout, Stderr>(
        &self,
        source_image: &ContainerImage,
        dest_image: &ContainerImage,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker tag {:?} {:?}", source_image, dest_image);
        let mut args = vec!["--config", self.config_path.path().to_str().unwrap_or(""), "tag"];
        let source_image = source_image.image_name();
        let dest_image = dest_image.image_name();
        args.push(source_image.as_str());
        args.push(dest_image.as_str());

        docker_exec(&args, &self.get_all_envs(&[]), stdout_output, stderr_output, should_abort)
    }

    pub fn mirror<Stdout, Stderr>(
        &self,
        source_image: &ContainerImage,
        dest_image: &ContainerImage,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker mirror {:?} {:?}", source_image, dest_image);
        self.create_manifest(
            dest_image,
            &[source_image.image_name().as_str()],
            stdout_output,
            stderr_output,
            should_abort,
        )
    }

    fn create_manifest<Stdout, Stderr>(
        &self,
        image: &ContainerImage,
        digests: &[&str],
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        let builder = self.configure_builder_for_http_registries(image);
        let image_tag = image.image_name();
        info!("Docker create manifest {} with digests {:?}", image_tag, digests);
        let mut args = vec![
            "--config",
            self.config_path.path().to_str().unwrap_or(""),
            "buildx",
            "imagetools",
            "create",
            "-t",
            image_tag.as_str(),
        ];
        if let Some(builder_name) = &builder.as_ref().and_then(|b| b.builder_name.as_deref()) {
            args.push("--builder");
            args.push(builder_name)
        }

        docker_exec(
            &[&args, digests].concat(),
            &self.get_all_envs(&[]),
            stdout_output,
            stderr_output,
            should_abort,
        )
    }

    fn configure_builder_for_http_registries(&self, image: &ContainerImage) -> Option<BuilderHandle> {
        // If the registry is HTTP, we need to create a new buildx config to allow HTTP registry
        let mut builder = None;
        if image.registry.scheme() == "http" {
            let http_registries = format!(
                "{}:{}",
                image.registry.host_str().unwrap_or(""),
                image.registry.port_or_known_default().unwrap_or(80)
            );

            info!("Docker configure builder for HTTP registry {:?}", http_registries);
            let now = format!("{}", Uuid::new_v4());
            builder = self
                .spawn_builder(
                    &now,
                    &now,
                    NonZeroUsize::new(1).unwrap(),
                    self.builder_location.supported_architectures(),
                    (0, 0),
                    (0, 0),
                    None,
                    &CommandKiller::never(),
                    &[http_registries.as_str()],
                    &[],
                    false,
                )
                .ok();
        };

        builder
    }
}

fn docker_exec<F, X>(
    args: &[&str],
    envs: &[(&str, &str)],
    stdout_output: &mut F,
    stderr_output: &mut X,
    cmd_killer: &CommandKiller,
) -> Result<(), DockerError>
where
    F: FnMut(String),
    X: FnMut(String),
{
    let mut cmd = QoveryCommand::new("docker", args, envs);
    cmd.set_kill_grace_period(Duration::from_secs(0));
    let ret = cmd.exec_with_abort(stdout_output, stderr_output, cmd_killer);

    match ret {
        Ok(_) => Ok(()),
        Err(CommandError::TimeoutError(msg)) => Err(DockerError::Timeout { raw_error_message: msg }),
        Err(CommandError::Killed(msg)) => Err(DockerError::Aborted { raw_error_message: msg }),
        Err(CommandError::ExitStatusError(err)) => Err(DockerError::ExitStatusError { exit_status: err }),
        Err(CommandError::ExecutionError(err)) => Err(DockerError::ExecutionError { raw_error: err }),
    }
}

// start a local registry to run this test
// docker run --rm -ti -p 5000:5000 --name registry registry:2
#[cfg(feature = "test-local-docker")]
#[cfg(test)]
mod tests {
    use crate::cmd::command::CommandKiller;
    use crate::cmd::docker::{Architecture, ContainerImage, Docker, DockerError};
    use std::num::NonZeroUsize;
    use std::path::Path;
    use std::time::Duration;
    use url::Url;
    use uuid::Uuid;

    fn private_registry_url() -> Url {
        Url::parse("http://localhost:5000").unwrap()
    }

    #[cfg(target_arch = "x86_64")]
    static CPU_ARCHITECTURE: &[Architecture] = &[Architecture::AMD64];
    #[cfg(target_arch = "aarch64")]
    static CPU_ARCHITECTURE: &[Architecture] = &[Architecture::ARM64];

    #[test]
    fn test_pull() {
        let docker = Docker::new(None).unwrap();

        // Invalid image should fails
        let image = ContainerImage::new(
            Url::parse("https://public.ecr.aws").unwrap(),
            "r3m4q3r9/pub-mirror-debian".to_string(),
            vec!["1.0".to_string()],
        );
        let ret = docker.pull(
            &image,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_err());

        // Valid image should be ok
        let image = ContainerImage::new(
            Url::parse("https://public.ecr.aws").unwrap(),
            "r3m4q3r9/pub-mirror-debian".to_string(),
            vec!["11.6-ci".to_string()],
        );

        let ret = docker.pull(
            &image,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());

        // Should timeout
        let ret = docker.pull(
            &image,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::from_timeout(Duration::from_secs(0)),
        );
        assert!(matches!(ret, Err(DockerError::Timeout { .. })));
    }

    #[test]
    fn test_buildkit_build() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_local_builder(None).unwrap();
        let image_to_build = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["3.15".to_string()],
        );
        let image_cache = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["cache".to_string()],
        );

        // It should work
        let ret = docker.build_with_buildkit(
            &None,
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            CPU_ARCHITECTURE,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
            None,
        );

        assert!(ret.is_ok());

        let ret = docker.build_with_buildkit(
            &None,
            Path::new("tests/docker/multi_stage_simple/Dockerfile.buildkit"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            CPU_ARCHITECTURE,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
            None,
        );

        assert!(ret.is_ok());
    }

    #[test]
    fn test_push() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_local_builder(None).unwrap();
        let image_to_build = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["v42.42".to_string()],
        );
        let image_cache = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["cache".to_string()],
        );

        // It should work
        let ret = docker.build_with_buildkit(
            &None,
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            CPU_ARCHITECTURE,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
            None,
        );
        assert!(ret.is_ok());

        let ret = docker.does_image_exist_locally(&image_to_build);
        assert!(matches!(ret, Ok(true)));

        let ret = docker.does_image_exist_remotely(&image_to_build);
        assert!(matches!(ret, Ok(false)));

        let ret = docker.push(
            &image_to_build,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());

        let ret = docker.does_image_exist_remotely(&image_to_build);
        assert!(matches!(ret, Ok(true)));

        let ret = docker.pull(
            &image_to_build,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());
    }

    #[test]
    fn test_mirror() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_local_builder(None).unwrap();
        let image_source = ContainerImage::new(
            Url::parse("https://public.ecr.aws").unwrap(),
            "r3m4q3r9/pub-mirror-debian".to_string(),
            vec!["11.6-ci".to_string()],
        );
        let image_dest = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["mirror".to_string()],
        );

        // It should work
        let ret = docker.mirror(
            &image_source,
            &image_dest,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());

        let ret = docker.pull(
            &image_dest,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());
    }

    #[cfg(feature = "test-local-kube")]
    #[test]
    fn test_with_kube_builder() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let mut kube_config = dirs::home_dir().unwrap();
        kube_config.push(".kube/config");
        let args = vec![
            ("AWS_DEFAULT_REGION", "eu-west-3"),
            ("KUBECONFIG", kube_config.to_str().unwrap()),
            // ("AWS_SECRET_ACCESS_KEY", "xxxxx"),
            // ("AWS_ACCESS_KEY_ID", "xxxx"),
            // ("KUBECONFIG", "xxx"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let docker = Docker::new_with_kube_builder(
            None,
            &[Architecture::ARM64, Architecture::AMD64],
            "default".to_string(),
            "builder-".to_string(),
            args,
            true,
        )
        .unwrap();
        let builder = docker
            .spawn_builder(
                Uuid::new_v4().to_string().as_str(),
                "builder",
                NonZeroUsize::new(1).unwrap(),
                CPU_ARCHITECTURE,
                (0, 1000),
                (0, 1),
                Some(1),
                &CommandKiller::never(),
                &[],
                &[],
                true,
            )
            .unwrap();

        let image_to_build = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["3.15".to_string()],
        );
        let image_cache = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["cache".to_string()],
        );

        // It should work
        let ret = docker.build_with_buildkit(
            &builder.builder_name.as_deref(),
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &[Architecture::AMD64],
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
            None,
        );

        assert!(ret.is_ok());
    }

    #[test]
    fn test_buildkit_build_with_target() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_local_builder(None).unwrap();
        let image_to_build = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["3.15".to_string()],
        );
        let image_cache = ContainerImage::new(
            private_registry_url(),
            "local-repo/alpine".to_string(),
            vec!["cache".to_string()],
        );

        // It should work
        let ret = docker.build_with_buildkit(
            &None,
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            CPU_ARCHITECTURE,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
            Some(&"build".to_string()),
        );

        assert!(ret.is_ok());

        let ret = docker.build_with_buildkit(
            &None,
            Path::new("tests/docker/multi_stage_simple/Dockerfile.buildkit"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            CPU_ARCHITECTURE,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
            Some(&"build".to_string()),
        );

        assert!(ret.is_ok());
    }
}
