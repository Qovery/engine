use crate::cmd::command::{CommandError, CommandKiller, QoveryCommand};
use lazy_static::lazy_static;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::process::ExitStatus;
use std::sync::Mutex;
use url::Url;

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

lazy_static! {
    // Docker login when launched in parallel can mess up ~/.docker/config.json
    // We use a mutex that will force serialization of logins in order to avoid that
    // Mostly use for CI/Test when all test start in parallel and it the login phase at the same time
    static ref LOGIN_LOCK: Mutex<()> = Mutex::new(());
}

#[derive(Clone, Debug)]
pub struct BuildResult {
    source_cached_image: Option<ContainerImage>,
    build_candidate_image: Option<ContainerImage>,
    cached_image_pulled: bool,
    image_exists_remotely: bool,
    built: bool,
    pushed: bool,
}

impl BuildResult {
    pub fn new() -> Self {
        Self {
            source_cached_image: None,
            build_candidate_image: None,
            cached_image_pulled: false,
            image_exists_remotely: false,
            built: false,
            pushed: false,
        }
    }

    pub fn source_cached_image(&mut self, source_cached_image: Option<ContainerImage>) -> &mut Self {
        self.source_cached_image = source_cached_image;
        self
    }

    pub fn build_candidate_image(&mut self, build_candidate_image: Option<ContainerImage>) -> &mut Self {
        self.build_candidate_image = build_candidate_image;
        self
    }

    pub fn cached_image_pulled(&mut self, cached_image_pulled: bool) -> &mut Self {
        self.cached_image_pulled = cached_image_pulled;
        self
    }

    pub fn image_exists_remotely(&mut self, image_exists_remotely: bool) -> &mut Self {
        self.image_exists_remotely = image_exists_remotely;
        self
    }

    pub fn built(&mut self, built: bool) -> &mut Self {
        self.built = built;
        self
    }

    pub fn pushed(&mut self, pushed: bool) -> &mut Self {
        self.pushed = pushed;
        self
    }
}

impl Default for BuildResult {
    fn default() -> Self {
        BuildResult::new()
    }
}

impl Display for BuildResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.build_candidate_image.is_none() {
            return f.write_str(
                r#"
Build summary:
    ⁉️ no image to be built provided
"#,
            );
        }

        let image_to_be_built = self
            .build_candidate_image
            .as_ref()
            .expect("cannot get image to be built");
        let output = format!(
            r#"
Build summary:
    🐳️ image to be built: `{}`
    {}
    {}
    {}
    {}
    {}"#,
            image_to_be_built.image_name(),
            match &self.image_exists_remotely {
                true => "♻️ image exists remotely",
                false => "🕳 image doesn't exist remotely",
            },
            // TODO(benjaminch): check whether cached image exists locally before pulling in order to get more details here
            match &self.source_cached_image {
                Some(cache) => format!("🍀 cached image provided: `{}`", cache.image_name()),
                None => "🕳 no cached image provided".to_string(),
            },
            match self.cached_image_pulled {
                true => "✔️ cached image pulled",
                false => "⁉️ cached image not pulled (most likely doesn't exists remotely)",
            },
            match self.built {
                true => "🎉 image built",
                false => "‼️ image not built",
            },
            match self.pushed {
                true => "🚀 image pushed",
                false => "‼️ image not pushed",
            }
        );

        f.write_str(output.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ContainerImage {
    pub registry: Url,
    pub name: String,
    pub tags: Vec<String>,
}

impl ContainerImage {
    pub fn new(registry: Url, name: String, tags: Vec<String>) -> Self {
        ContainerImage { registry, name, tags }
    }

    pub fn image_names(&self) -> Vec<String> {
        let host = if let Some(port) = self.registry.port() {
            format!("{}:{}", self.registry.host_str().unwrap_or_default(), port)
        } else {
            self.registry.host_str().unwrap_or_default().to_string()
        };

        self.tags
            .iter()
            .map(|tag| format!("{}/{}:{}", host, &self.name, tag))
            .collect()
    }

    pub fn image_name(&self) -> String {
        self.image_names().remove(0)
    }
}

#[derive(Debug, Clone)]
pub struct Docker {
    use_buildkit: bool,
    common_envs: Vec<(String, String)>,
}

impl Docker {
    pub fn new_with_options(enable_buildkit: bool, socket_location: Option<Url>) -> Result<Self, DockerError> {
        let mut docker = Docker {
            use_buildkit: enable_buildkit,
            common_envs: vec![(
                "DOCKER_BUILDKIT".to_string(),
                if enable_buildkit {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            )],
        };

        // Override DOCKER_HOST if we use a TCP socket
        if let Some(socket_location) = socket_location {
            docker
                .common_envs
                .push(("DOCKER_HOST".to_string(), socket_location.to_string()))
        }

        // If we don't use buildkit nothing more to do
        if !docker.use_buildkit {
            return Ok(docker);
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

        // In order to be able to use --cache-from --cache-to for buildkit,
        // we need to create our specific builder, which is not the default one (aka: the docker one)
        let args = vec![
            "buildx",
            "create",
            "--name",
            "qovery-engine",
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

    pub fn new(socket_location: Option<Url>) -> Result<Self, DockerError> {
        Self::new_with_options(true, socket_location)
    }

    fn get_all_envs<'a>(&'a self, envs: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all_envs: Vec<(&str, &str)> = self.common_envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all_envs.append(&mut envs.to_vec());

        all_envs
    }

    pub fn login(&self, registry: &Url) -> Result<(), DockerError> {
        info!("Docker login {} as user {}", registry, registry.username());

        let _lock = LOGIN_LOCK.lock().unwrap();
        let password = urlencoding::decode(registry.password().unwrap_or_default())
            .unwrap_or_default()
            .to_string();
        let args = vec![
            "login",
            registry.host_str().unwrap_or_default(),
            "-u",
            registry.username(),
            "-p",
            &password,
        ];

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
            &["image", "inspect", &image.image_name()],
            &self.get_all_envs(&[]),
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::never(),
        );

        Ok(matches!(ret, Ok(_)))
    }

    // Warning: this command is slow > 10 sec
    pub fn does_image_exist_remotely(&self, image: &ContainerImage) -> Result<bool, DockerError> {
        info!("Docker check remotely image exist {:?}", image);

        let ret = docker_exec(
            &["manifest", "inspect", &image.image_name()],
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
            &["pull", &image.image_name()],
            &self.get_all_envs(&[]),
            stdout_output,
            stderr_output,
            should_abort,
        )
    }

    pub fn build<Stdout, Stderr>(
        &self,
        dockerfile: &Path,
        context: &Path,
        image_to_build: &ContainerImage,
        build_args: &[(&str, &str)],
        cache: &ContainerImage,
        push_after_build: bool,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<BuildResult, DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        let mut build_result = BuildResult::new();

        // if there is no tags, nothing to build
        if image_to_build.tags.is_empty() {
            build_result.built = false;
            return Ok(build_result);
        }

        // Do some checks
        if !dockerfile.is_file() {
            return Err(DockerError::InvalidConfig {
                raw_error_message: format!("provided dockerfile `{:?}` is not a valid file", dockerfile),
            });
        }

        if !context.is_dir() {
            return Err(DockerError::InvalidConfig {
                raw_error_message: format!("provided docker build context `{:?}` is not a valid directory", context),
            });
        }

        if self.use_buildkit {
            self.build_with_buildkit(
                dockerfile,
                context,
                image_to_build,
                build_args,
                cache,
                push_after_build,
                stdout_output,
                stderr_output,
                should_abort,
            )
        } else {
            self.build_with_docker(
                dockerfile,
                context,
                image_to_build,
                build_args,
                cache,
                push_after_build,
                stdout_output,
                stderr_output,
                should_abort,
            )
        }
    }

    fn build_with_docker<Stdout, Stderr>(
        &self,
        dockerfile: &Path,
        context: &Path,
        image_to_build: &ContainerImage,
        build_args: &[(&str, &str)],
        cache: &ContainerImage,
        push_after_build: bool,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<BuildResult, DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker build {:?}", image_to_build.image_name());

        let mut build_result = BuildResult::new();
        build_result.build_candidate_image(Some(image_to_build.clone()));
        build_result.source_cached_image(Some(cache.clone()));

        // Best effort to pull the cache, if it does not exist that's ok too
        match self.pull(cache, stdout_output, stderr_output, should_abort) {
            Ok(_) => build_result.cached_image_pulled(true),
            Err(_) => build_result.cached_image_pulled(false),
        };

        let mut args_string: Vec<String> = vec![
            "build".to_string(),
            "--network".to_string(),
            "host".to_string(),
            "-f".to_string(),
            dockerfile.to_str().unwrap_or_default().to_string(),
        ];

        for image_name in image_to_build.image_names() {
            args_string.push("--tag".to_string());
            args_string.push(image_name)
        }

        for img_cache_name in cache.image_names() {
            args_string.push("--tag".to_string());
            args_string.push(img_cache_name.to_string());
        }

        for (k, v) in build_args {
            args_string.push("--build-arg".to_string());
            args_string.push(format!("{}={}", k, v));
        }

        args_string.push(context.to_str().unwrap_or_default().to_string());

        docker_exec(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(&[]),
            stdout_output,
            stderr_output,
            should_abort,
        )?;
        build_result.built(true);

        if push_after_build {
            self.push(image_to_build, stdout_output, stderr_output, should_abort)?;
            build_result.pushed(true);
        }

        Ok(build_result)
    }

    fn build_with_buildkit<Stdout, Stderr>(
        &self,
        dockerfile: &Path,
        context: &Path,
        image_to_build: &ContainerImage,
        build_args: &[(&str, &str)],
        cache: &ContainerImage,
        push_after_build: bool,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<BuildResult, DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker buildkit build {:?}", image_to_build.image_name());

        let mut build_result = BuildResult::new();
        build_result.build_candidate_image(Some(image_to_build.clone()));
        build_result.source_cached_image(Some(cache.clone()));

        let mut args_string: Vec<String> = vec![
            "buildx".to_string(),
            "build".to_string(),
            "--progress=plain".to_string(),
            "--network=host".to_string(),
            if push_after_build {
                build_result.pushed(true);
                "--output=type=registry".to_string() // tell buildkit to push image to registry
            } else {
                "--output=type=docker".to_string() // tell buildkit to load the image into docker after build
            },
            "--cache-from".to_string(),
            format!("type=registry,ref={}", cache.image_name()),
            // Disabled for now, because private ECR does not support it ...
            // https://github.com/aws/containers-roadmap/issues/876
            // "--cache-to".to_string(),
            // format!("type=registry,ref={}", cache.image_name()),
            "-f".to_string(),
            dockerfile.to_str().unwrap_or_default().to_string(),
        ];

        for image_name in image_to_build.image_names() {
            args_string.push("--tag".to_string());
            args_string.push(image_name.to_string())
        }

        for (k, v) in build_args {
            args_string.push("--build-arg".to_string());
            args_string.push(format!("{}={}", k, v));
        }

        args_string.push(context.to_str().unwrap_or_default().to_string());

        match docker_exec(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(&[]),
            stdout_output,
            stderr_output,
            should_abort,
        ) {
            Ok(_) => {
                build_result.cached_image_pulled(true); // --cache-from
                build_result.built(true);
                Ok(build_result)
            }
            Err(e) => Err(e),
        }
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
        let image_names = image.image_names();
        let mut args = vec!["push"];
        args.extend(image_names.iter().map(|x| x.as_str()));

        docker_exec(&args, &self.get_all_envs(&[]), stdout_output, stderr_output, should_abort)
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
        let mut args = vec!["tag"];
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
        self.pull(source_image, stdout_output, stderr_output, should_abort)?;
        self.tag(source_image, dest_image, stdout_output, stderr_output, should_abort)?;
        self.push(dest_image, stdout_output, stderr_output, should_abort)
    }

    pub fn prune_images(&self) -> Result<(), DockerError> {
        info!("Docker prune images");

        let all_prunes_commands = vec![
            vec!["buildx", "prune", "-a", "-f"],
            vec!["container", "prune", "-f"],
            vec!["image", "prune", "-a", "-f"],
            vec!["builder", "prune", "-a", "-f"],
            vec!["volume", "prune", "-f"],
        ];

        let mut errored_commands = vec![];
        for prune in all_prunes_commands {
            let ret = docker_exec(
                &prune,
                &self.get_all_envs(&[]),
                &mut |_| {},
                &mut |_| {},
                &CommandKiller::never(),
            );
            if let Err(e) = ret {
                errored_commands.push(e);
            }
        }

        if !errored_commands.is_empty() {
            return Err(errored_commands.remove(0));
        }

        Ok(())
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
    use crate::cmd::docker::{ContainerImage, Docker, DockerError};
    use std::path::Path;
    use std::time::Duration;
    use url::Url;

    fn private_registry_url() -> Url {
        Url::parse("http://localhost:5000").unwrap()
    }

    #[test]
    fn test_pull() {
        let docker = Docker::new(None).unwrap();

        // Invalid image should fails
        let image = ContainerImage {
            registry: Url::parse("https://docker.io").unwrap(),
            name: "alpine".to_string(),
            tags: vec!["666".to_string()],
        };
        let ret = docker.pull(
            &image,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Err(_)));

        // Valid image should be ok
        let image = ContainerImage {
            registry: Url::parse("https://docker.io").unwrap(),
            name: "alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };

        let ret = docker.pull(
            &image,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));

        // Should timeout
        let ret = docker.pull(
            &image,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::from_timeout(Duration::from_secs(1)),
        );
        assert!(matches!(ret, Err(DockerError::Timeout { .. })));
    }

    #[test]
    fn test_docker_build() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_options(false, None).unwrap();
        let image_to_build = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };
        let image_cache = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["cache".to_string()],
        };

        let ret = docker.build_with_docker(
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );

        assert!(matches!(ret, Ok(_)));

        // It should fails with buildkit dockerfile
        let ret = docker.build_with_docker(
            Path::new("tests/docker/multi_stage_simple/Dockerfile.buildkit"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );

        assert!(matches!(ret, Err(_)));
    }

    #[test]
    fn test_buildkit_build() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_options(true, None).unwrap();
        let image_to_build = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };
        let image_cache = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["cache".to_string()],
        };

        // It should work
        let ret = docker.build_with_buildkit(
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );

        assert!(matches!(ret, Ok(_)));

        let ret = docker.build_with_buildkit(
            Path::new("tests/docker/multi_stage_simple/Dockerfile.buildkit"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );

        assert!(matches!(ret, Ok(_)));
    }

    #[test]
    fn test_push() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_options(true, None).unwrap();
        let image_to_build = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };
        let image_cache = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["cache".to_string()],
        };

        // It should work
        let ret = docker.build_with_buildkit(
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));

        let ret = docker.does_image_exist_locally(&image_to_build);
        assert!(matches!(ret, Ok(true)));

        let ret = docker.does_image_exist_remotely(&image_to_build);
        assert!(matches!(ret, Ok(false)));

        let ret = docker.push(
            &image_to_build,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));

        let ret = docker.pull(
            &image_to_build,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));
    }

    #[test]
    fn test_mirror() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_options(true, None).unwrap();
        let image_source = ContainerImage {
            registry: Url::parse("https://docker.io").unwrap(),
            name: "alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };
        let image_dest = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["mirror".to_string()],
        };

        // It should work
        let ret = docker.mirror(
            &image_source,
            &image_dest,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));

        let ret = docker.pull(
            &image_dest,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));
    }
}
