#![allow(clippy::redundant_closure)]

use std::collections::{BTreeMap, HashSet};
use std::io::{Error, Write};
use std::num::NonZeroUsize;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use std::{fs, thread};

use git2::{Cred, CredentialType, ErrorClass};
use retry::OperationResult;
use retry::delay::Fibonacci;
use tempfile::TempDir;
use time::Instant;
use uuid::Uuid;

use crate::cmd::command::CommandKiller;
use crate::cmd::docker;
use crate::cmd::docker::{Architecture, BuilderHandle, ContainerImage};
use crate::cmd::git_lfs::{GitLfs, GitLfsError};
use crate::environment::report::logger::EnvLogger;
use crate::infrastructure::models::build_platform::dockerfile_utils::{
    DockerfileSecretMounts, extract_dockerfile_args, extract_dockerfile_secret_mounts,
};
use crate::infrastructure::models::build_platform::{
    Build, BuildError, BuildPlatform, BuildSource, CUSTOM_FRAGMENT_PLACEHOLDER, DockerfileFragment, Kind,
    SYNTHESIZED_DOCKERFILE_NAME, to_build_error,
};

use crate::cmd::git;
use crate::environment::models::abort::Abort;
use crate::fs::workspace_directory;
use crate::io_models::container::Registry;
use crate::io_models::context::Context;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::utilities::to_short_id;

const DOCKER_IGNORE: &str = r#"
# Ignore all logs
*.log

# Ignore git repository files
.git
.gitignore
"#;

/// use Docker in local
pub struct LocalDocker {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    builder_counter: AtomicUsize,
    metrics_registry: Box<dyn MetricsRegistry>,
}

const MAX_GIT_LFS_SIZE_GB: u64 = 5;
const MAX_GIT_LFS_SIZE_KB: u64 = MAX_GIT_LFS_SIZE_GB * 1024 * 1024; // 5GB

impl LocalDocker {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        metrics_registry: Box<dyn MetricsRegistry>,
    ) -> Result<Self, BuildError> {
        Ok(LocalDocker {
            context,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            builder_counter: AtomicUsize::new(0),
            metrics_registry,
        })
    }

    /// Injects custom Dockerfile fragment into the build
    /// If explicit fragment config provided, use that (file path or inline content)
    fn inject_custom_build_fragment(
        dockerfile_content: &str,
        build_context_path: &Path,
        dockerfile_fragment: Option<&DockerfileFragment>,
        service_id: &str,
    ) -> Result<String, BuildError> {
        if !dockerfile_content.contains(CUSTOM_FRAGMENT_PLACEHOLDER) && dockerfile_fragment.is_some() {
            return Err(BuildError::InvalidConfig {
                application: service_id.to_string(),
                raw_error_message: format!(
                    "Dockerfile does not contain the required placeholder `{CUSTOM_FRAGMENT_PLACEHOLDER}` for injecting custom fragment"
                ),
            });
        }

        let custom_fragment = match dockerfile_fragment {
            None => String::new(),
            Some(DockerfileFragment::Inline { content }) => {
                info!("Using inline Dockerfile fragment ({} bytes)", content.len());
                content.clone()
            }
            Some(DockerfileFragment::File { path }) => {
                // Path is provided relative to root_module_path
                // Strip leading slash if present since build_context_path is absolute
                let relative_path = path.strip_prefix('/').unwrap_or(path);
                let fragment_path = build_context_path.join(relative_path);

                match fs::read_to_string(&fragment_path) {
                    Ok(content) => {
                        info!(
                            "Found custom build fragment at {:?}, injecting into Dockerfile ({} bytes)",
                            fragment_path,
                            content.len()
                        );
                        content
                    }
                    Err(e) => {
                        return Err(BuildError::IoError {
                            application: service_id.to_string(),
                            action_description: format!(
                                "reading dockerfile fragment from {fragment_path:?} (configured path: {path})"
                            ),
                            raw_error: e,
                        });
                    }
                }
            }
        };

        Ok(dockerfile_content.replace(CUSTOM_FRAGMENT_PLACEHOLDER, &custom_fragment))
    }

    fn build_image_with_docker(
        &self,
        build: &mut Build,
        dockerfile_complete_path: &str,
        into_dir_docker_style: &str,
        logger: &EnvLogger,
        metrics_registry: Arc<dyn MetricsRegistry>,
        abort: &dyn Abort,
    ) -> Result<(), BuildError> {
        // Going to inject only env var that are used by the dockerfile
        // so extracting it and modifying the image tag and env variables
        let build_record =
            metrics_registry.start_record(build.image.service_long_id, StepLabel::Service, StepName::Build);
        let dockerfile_content = fs::read(dockerfile_complete_path).map_err(|err| BuildError::IoError {
            application: build.image.service_id.clone(),
            action_description: "reading dockerfile content".to_string(),
            raw_error: err,
        })?;
        let dockerfile_args = match extract_dockerfile_args(&dockerfile_content) {
            Ok(dockerfile_args) => dockerfile_args,
            Err(err) => {
                build_record.stop(StepStatus::Error);
                return Err(BuildError::InvalidConfig {
                    application: build.image.service_id.clone(),
                    raw_error_message: format!("Cannot extract env vars from your dockerfile {err}"),
                });
            }
        };
        let secret_mounts = match extract_dockerfile_secret_mounts(&dockerfile_content) {
            Ok(secret_mounts) => secret_mounts,
            Err(err) => {
                build_record.stop(StepStatus::Error);
                return Err(BuildError::InvalidConfig {
                    application: build.image.service_id.clone(),
                    raw_error_message: format!("Cannot extract secret mounts from your dockerfile {err}"),
                });
            }
        };

        // Fail before doing any work: nothing later can wire a mount whose id we would have to guess.
        if secret_mounts.has_mount_without_id {
            build_record.stop(StepStatus::Error);
            return Err(BuildError::InvalidConfig {
                application: build.image.service_id.clone(),
                raw_error_message: "Your Dockerfile declares a `RUN --mount=type=secret` without an `id=`. \
                     Add `id=<BUILD_VARIABLE_NAME>` to the mount so Qovery knows which build variable to pass."
                    .to_string(),
            });
        }

        // Keep only the env variables we want for our build
        // and force re-compute the image tag.
        // Secret mount ids are kept too: `compute_image_tag` hashes values, so a rotated secret
        // yields a new tag and therefore a rebuild instead of being skipped as already-present.
        build
            .environment_variables
            .retain(|k, _| dockerfile_args.contains(k) || secret_mounts.ids.contains(k));
        build.compute_image_tag();

        // Prepare image we want to build
        let image_to_build = ContainerImage::new(
            build.image.registry_url.clone(),
            build.image.name(),
            vec![build.image.tag.clone(), "latest".to_string()],
        );

        let image_cache = match build.disable_buildkit_cache {
            true => None,
            false => Some(ContainerImage::new(
                build.image.registry_url.clone(),
                build.image.name(),
                vec![build.compute_cache_tag()],
            )),
        };

        // Login to the registry at repository level if needed
        let login_ret = retry::retry(Fibonacci::from(Duration::from_secs(1)).take(4), || {
            self.context
                .docker
                .login(&build.image.registry_url)
                .inspect_err(|_err| {
                    logger.send_warning("🔓 Retrying to login to registry due to error...".to_string());
                })
        });

        if let Err(err) = login_ret {
            logger.send_warning(format!(
                "❌ Failed to login to registry {} due to {}",
                build.image.registry_url, err
            ));
            let err = BuildError::DockerError {
                application: build.image.service_id.clone(),
                raw_error: err.error,
            };
            return Err(err);
        }

        // Check if the image does not exist already remotely, if yes, we skip the build
        let image_name = image_to_build.image_name();
        logger.send_progress(format!("🕵️ Checking if image already exists remotely {image_name}"));
        if let Ok(true) = self.context.docker.does_image_exist_remotely(&image_to_build) {
            logger.send_progress(format!("🎯 Skipping build. Image already exists in the registry {image_name}"));
            build_record.stop(StepStatus::Skip);
            // skip build
            return Ok(());
        }

        logger.send_progress(format!("⛏️ Building image. It does not exist remotely {image_name}"));

        // login if there are some private registries used
        for registry in &build.registries {
            // TODO(benjaminch): To handle GCP Artifact Registry login, credentials to be injected, maybe this whole login should be done later on or delegated to container registry objects
            // Method to be called for GCP: cmd::docker::Docker::login_artifact_registry()
            if let Registry::GcpArtifactRegistry { url, .. } = registry {
                logger.send_warning(format!(
                    "Skipping logging at this step for Artifact Registry `{}`",
                    url.host_str().unwrap_or_default()
                ));
                continue;
            }

            let url = registry
                .get_url_with_credentials()
                .map_err(|_| BuildError::CannotGetCredentials {
                    raw_error_message: "Cannot get the registry credentials".to_string(),
                })?;
            if url.password().is_none() {
                continue;
            }

            logger.send_progress(format!(
                "🔓 Login to registry {} as user {}",
                url.host_str().unwrap_or_default(),
                url.username()
            ));

            let login_ret = retry::retry(Fibonacci::from(Duration::from_secs(1)).take(4), || {
                self.context.docker.login(&url).inspect_err(|_err| {
                    logger.send_warning("🔓 Retrying to login to registry due to error...".to_string());
                })
            });

            if let Err(err) = login_ret {
                logger.send_warning(format!(
                    "❌ Failed to login to registry {} due to {}",
                    url.host_str().unwrap_or_default(),
                    err
                ));
                let err = BuildError::DockerError {
                    application: build.image.service_id.clone(),
                    raw_error: err.error,
                };
                return Err(err);
            }
        }

        // Actually do the build of the image
        let (build_args, secret_values) =
            split_build_inputs(&build.environment_variables, &dockerfile_args, &secret_mounts);

        let arch: Vec<Architecture> = build
            .architectures
            .iter()
            .map(|arch| docker::Architecture::from(arch))
            .collect();

        let builder_handle =
            self.provision_builder(build, |line| logger.send_progress(line), &CommandKiller::from_cancelable(abort))?;

        // Written as late as possible, and held only until the build returns: dropping it removes
        // the files, whatever ended the build, including a `CommandKiller` timeout or an abort that
        // never returns normally.
        let secret_files = match BuildSecretFiles::new(&build.image.service_id, &secret_values) {
            Ok(secret_files) => secret_files,
            Err(err) => {
                build_record.stop(StepStatus::Error);
                return Err(err);
            }
        };
        let secrets = secret_files.as_build_flags();

        let exit_status = self.context.docker.build(
            &builder_handle.builder_name.as_deref(),
            Path::new(dockerfile_complete_path),
            Path::new(into_dir_docker_style),
            &image_to_build,
            &build_args,
            &secrets,
            image_cache.as_ref(),
            true,
            &arch,
            &mut |line| logger.send_progress(line),
            &mut |line| logger.send_progress(line),
            &CommandKiller::from(build.timeout, abort),
            build
                .git_repository()
                .and_then(|repository| repository.docker_target_build_stage.as_ref()),
        );

        if let Err(err) = exit_status {
            build_record.stop(StepStatus::Error);
            return Err(to_build_error(build.image.service_id.clone(), err));
        }
        build_record.stop(StepStatus::Success);
        Ok(())
    }

    fn provision_builder(
        &self,
        build: &Build,
        env_logger: impl Fn(String),
        should_abort: &CommandKiller,
    ) -> Result<BuilderHandle, BuildError> {
        let (max_cpu, max_ram) = (build.max_cpu_in_milli, build.max_ram_in_gib);

        env_logger(format!(
            "🧑‍🏭 Provisioning docker builder with {max_cpu}m CPU and {max_ram}gib RAM for parallel build. This can take some time"
        ));
        // Docker has a hardcoded timeout of 1 minute for the builder creation
        // it may be too short for us, so retry until we reach our deadline
        // https://github.com/docker/buildx/blob/master/driver/kubernetes/driver.go#L116
        let deadline = Instant::now() + Duration::from_secs(60 * 10); // 10min

        // We need to do special handling for insecure registries or http ones.
        let cr = &build.image.registry_url;
        let http_registries = if cr.scheme() == "http" {
            vec![format!(
                "{}:{}",
                cr.host_str().unwrap_or(""),
                cr.port_or_known_default().unwrap_or(80)
            )]
        } else {
            vec![]
        };
        let insecure_registries = if build.image.registry_insecure {
            vec![format!(
                "{}:{}",
                cr.host_str().unwrap_or(""),
                cr.port_or_known_default().unwrap_or(443)
            )]
        } else {
            vec![]
        };

        let arch: Vec<Architecture> = build
            .architectures
            .iter()
            .map(|arch| docker::Architecture::from(arch))
            .collect();

        let provision_builder = self.metrics_registry.start_record(
            build.image.service_long_id,
            StepLabel::Service,
            StepName::ProvisionBuilder,
        );

        let exec_id = self
            .context
            .execution_id()
            .rsplit_once('-')
            .unwrap_or((self.context.execution_id(), ""))
            .0;

        let builder_handle = loop {
            match self.context.docker.spawn_builder(
                &format!("{}-{}", exec_id, self.builder_counter.fetch_add(1, Ordering::Relaxed)),
                build.image.service_long_id.to_string().as_str(),
                NonZeroUsize::new(1).unwrap(),
                &arch,
                (max_cpu, max_cpu),
                (max_ram, max_ram),
                build.ephemeral_storage_in_gib,
                should_abort,
                http_registries
                    .iter()
                    .map(String::as_ref)
                    .collect::<Vec<_>>()
                    .as_slice(),
                insecure_registries
                    .iter()
                    .map(String::as_ref)
                    .collect::<Vec<_>>()
                    .as_slice(),
                true,
            ) {
                Ok(build_handle) => break build_handle,
                Err(err) => {
                    error!("cannot provision docker builder: {}", err);
                    if should_abort.should_abort().is_some() {
                        provision_builder.stop(StepStatus::Cancel);
                        return Err(BuildError::Aborted {
                            application: build.image.service_id.clone(),
                        });
                    }

                    if err.is_aborted() || Instant::now() >= deadline {
                        provision_builder.stop(StepStatus::Error);
                        return Err(BuildError::DockerError {
                            application: build.image.service_id.clone(),
                            raw_error: err,
                        });
                    }

                    env_logger("⚠️ Cannot provision docker builder. Retrying...".to_string());
                    thread::sleep(Duration::from_secs(1));
                }
            }
        };
        provision_builder.stop(StepStatus::Success);

        Ok(builder_handle)
    }

    /// Builds an image that has no source repository: the Dockerfile is fully synthesized by the
    /// engine and written into an otherwise empty build context. Only its `FROM` base image and the
    /// fragment spliced into it contribute to the result, so there is nothing to clone.
    fn build_from_dockerfile(
        &self,
        build: &mut Build,
        dockerfile_content: &str,
        logger: &EnvLogger,
        metrics_registry: Arc<dyn MetricsRegistry>,
        abort: &dyn Abort,
    ) -> Result<(), BuildError> {
        let app_id = build.image.service_id.clone();
        let build_context_path = self.get_repository_build_root_path(build)?;

        logger.send_progress("📝 Preparing build context for the generated Dockerfile".to_string());

        // The workspace is reused across deployments of the same service, so start from a clean
        // context: a leftover file would silently end up in the image.
        if build_context_path.exists() {
            fs::remove_dir_all(&build_context_path).map_err(|err| BuildError::IoError {
                application: app_id.clone(),
                action_description: "cleaning old build context".to_string(),
                raw_error: err,
            })?;
        }
        fs::create_dir_all(&build_context_path).map_err(|err| BuildError::IoError {
            application: app_id.clone(),
            action_description: "creating build context".to_string(),
            raw_error: err,
        })?;

        let dockerfile_content = Self::inject_custom_build_fragment(
            dockerfile_content,
            &build_context_path,
            build.dockerfile_fragment.as_ref(),
            &app_id,
        )?;

        let dockerfile_path = build_context_path.join(SYNTHESIZED_DOCKERFILE_NAME);
        fs::write(&dockerfile_path, dockerfile_content).map_err(|err| BuildError::IoError {
            application: app_id.clone(),
            action_description: "writing dockerfile content".to_string(),
            raw_error: err,
        })?;
        fs::write(build_context_path.join(".dockerignore"), DOCKER_IGNORE).map_err(|err| BuildError::IoError {
            application: app_id,
            action_description: "writing .dockerignore content".to_string(),
            raw_error: err,
        })?;

        self.build_image_with_docker(
            build,
            dockerfile_path.to_str().unwrap_or_default(),
            build_context_path.to_str().unwrap_or_default(),
            logger,
            metrics_registry,
            abort,
        )
    }

    fn get_repository_build_root_path(&self, build: &Build) -> Result<PathBuf, BuildError> {
        workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("build/{}", build.image.service_id.as_str()),
        )
        .map_err(|err| BuildError::IoError {
            application: build.image.service_id.clone(),
            action_description: "when creating build workspace".to_string(),
            raw_error: err,
        })
    }
}

/// Name and value of one variable handed to a build, borrowed from `Build::environment_variables`.
type BuildVariable<'a> = (&'a str, &'a str);

/// Split the build variables into the values passed as `--build-arg` and the values mounted as
/// `--secret`.
///
/// The Dockerfile decides which is which: a name it declares as an `ARG` becomes a build arg, a
/// name it mounts as a secret id becomes a secret. A name declared both ways goes to both, because
/// both consumers are then real. Whether the user flagged the variable as secret plays no part —
/// see the plan for QOV-2210: gating on it would drop a variable the user can plainly see.
fn split_build_inputs<'a>(
    environment_variables: &'a BTreeMap<String, String>,
    dockerfile_args: &HashSet<String>,
    secret_mounts: &DockerfileSecretMounts,
) -> (Vec<BuildVariable<'a>>, Vec<BuildVariable<'a>>) {
    let mut build_args = Vec::new();
    let mut secrets = Vec::new();

    for (key, value) in environment_variables {
        if dockerfile_args.contains(key) {
            build_args.push((key.as_str(), value.as_str()));
        }
        if secret_mounts.ids.contains(key) {
            secrets.push((key.as_str(), value.as_str()));
        }
    }

    (build_args, secrets)
}

/// Build secret values written to disk so that BuildKit can read them through
/// `--secret id=<id>,src=<path>`.
///
/// The files sit in their own temporary directory, outside the build context: `local_docker` only
/// writes a `.dockerignore` for the synthesized-Dockerfile path, so a Git build uses the user's and
/// the context cannot be relied on to exclude anything.
struct BuildSecretFiles {
    /// Held only for its `Drop`, which removes the directory and everything in it. `None` when the
    /// Dockerfile mounts no secret, so that a build without one touches the filesystem exactly as
    /// it did before build secrets existed.
    _dir: Option<TempDir>,

    /// Secret id, and the file its value was written to.
    files: Vec<(String, PathBuf)>,
}

impl BuildSecretFiles {
    fn new(service_id: &str, secrets: &[BuildVariable]) -> Result<Self, BuildError> {
        if secrets.is_empty() {
            return Ok(BuildSecretFiles {
                _dir: None,
                files: Vec::new(),
            });
        }

        let io_error = |action_description: &str| {
            let (service_id, action_description) = (service_id.to_string(), action_description.to_string());
            move |err: Error| BuildError::IoError {
                application: service_id.clone(),
                action_description: action_description.clone(),
                raw_error: err,
            }
        };

        let dir = tempfile::Builder::new()
            .prefix("qovery-build-secrets-")
            .tempdir()
            .map_err(io_error("creating the build secrets directory"))?;

        // tempfile creates the directory with the process umask applied, which is 0755 on a default
        // macOS and on most Linux images. The 0600 on each file below is what actually protects the
        // values; narrowing the directory too just keeps the file names from being listed.
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))
            .map_err(io_error("restricting the build secrets directory"))?;

        let mut files = Vec::with_capacity(secrets.len());
        for (index, (id, value)) in secrets.iter().enumerate() {
            // Named by index rather than by id: the id comes from the Dockerfile, and must not be
            // able to steer where we write. BuildKit resolves the secret on the flag's `id=`.
            let path = dir.path().join(format!("secret-{index}"));

            // Created with the mode instead of chmod-ed afterwards, which would leave the value
            // world-readable for a moment.
            let mut file = fs::OpenOptions::new()
                .mode(0o600)
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(io_error("creating a build secret file"))?;

            // Written verbatim: the build step reads the file bytes as the secret value, so a
            // trailing newline would end up inside the secret.
            file.write_all(value.as_bytes())
                .map_err(io_error("writing a build secret file"))?;

            files.push(((*id).to_string(), path));
        }

        Ok(BuildSecretFiles { _dir: Some(dir), files })
    }

    fn as_build_flags(&self) -> Vec<(&str, &Path)> {
        self.files
            .iter()
            .map(|(id, path)| (id.as_str(), path.as_path()))
            .collect()
    }
}

impl BuildPlatform for LocalDocker {
    fn kind(&self) -> Kind {
        Kind::LocalDocker
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn build(
        &self,
        build: &mut Build,
        logger: &EnvLogger,
        metrics_registry: Arc<dyn MetricsRegistry>,
        abort: &dyn Abort,
    ) -> Result<(), BuildError> {
        // check if we should already abort the task
        if abort.status().should_cancel() {
            return Err(BuildError::Aborted {
                application: build.image.service_id.clone(),
            });
        }

        let synthesized_dockerfile = match &build.source {
            BuildSource::Git(_) => None,
            BuildSource::Dockerfile { content } => Some(content.clone()),
        };
        if let Some(content) = synthesized_dockerfile {
            return self.build_from_dockerfile(build, &content, logger, metrics_registry, abort);
        }

        let git_repository = build.git_repository().ok_or_else(|| BuildError::InvalidConfig {
            application: build.image.service_id.clone(),
            raw_error_message: "Build has no git repository to clone".to_string(),
        })?;

        // LOGGING
        let repository_root_path = self.get_repository_build_root_path(build)?;
        logger.send_progress(format!("📥 Cloning repository {}", git_repository.url));

        // Retrieve git credentials
        let git_user_creds = match git_repository.credentials() {
            None => None,
            Some(Ok(creds)) => Some(creds),
            Some(Err(err)) => {
                logger.send_warning(format!("🗝️ Unable to get credentials for git repository: {err}"));
                None
            }
        };

        // Create callback that will be called by git to provide credentials per user
        // If people use submodule, they need to provide us their ssh key
        let get_credentials = |user: &str| {
            let mut creds: Vec<(CredentialType, Cred)> = Vec::with_capacity(git_repository.ssh_keys.len() + 1);
            for ssh_key in git_repository.ssh_keys.iter() {
                let public_key = ssh_key.public_key.as_deref();
                let passphrase = ssh_key.passphrase.as_deref();
                if let Ok(cred) = Cred::ssh_key_from_memory(user, public_key, &ssh_key.private_key, passphrase) {
                    creds.push((CredentialType::SSH_MEMORY, cred));
                }
            }

            if let Some(git_creds) = &git_user_creds {
                creds.push((
                    CredentialType::USER_PASS_PLAINTEXT,
                    Cred::userpass_plaintext(&git_creds.login, &git_creds.password).unwrap(),
                ));
            }

            creds
        };

        // Cleanup, mono repo can require to clone multiple time the same repo
        // FIXME: re-use the same repo and just checkout at the correct commit
        if repository_root_path.exists() {
            let app_id = build.image.service_id.clone();
            fs::remove_dir_all(&repository_root_path).map_err(|err| BuildError::IoError {
                application: app_id,
                action_description: "cleaning old repository".to_string(),
                raw_error: err,
            })?;
        }

        // Do the real git clone
        let git_clone_record =
            metrics_registry.start_record(build.image.service_long_id, StepLabel::Service, StepName::GitClone);
        if let Err(error) = retry::retry(retry::delay::Fixed::from_millis(10_000).take(3), || {
            if let Err(BuildError::GitError {
                application: _,
                git_cmd,
                context,
                raw_error,
            }) = git::clone_at_commit(
                &git_repository.url,
                &git_repository.commit_id,
                &repository_root_path,
                &get_credentials,
                git_repository.skip_submodules,
            ) {
                let message = raw_error.message();
                let git_error_class = raw_error.class();
                // Some errors can happen "randomly":
                // - SSL error: syscall failure: Resource temporarily unavailable
                // - Timeout on git clone
                debug!("Error on git clone: git_error_class={:?}, message={}", git_error_class, message);
                return if git_error_class == ErrorClass::Os
                    || git_error_class == ErrorClass::Ssl
                    || (git_error_class == ErrorClass::Net && message.contains("timed out"))
                {
                    debug!("Retrying git clone...");
                    logger.send_warning(format!(
                        "⚠️ Retrying cloning your git repository, due to following error: {message}"
                    ));
                    OperationResult::Retry(BuildError::GitError {
                        application: build.image.service_id.clone(),
                        git_cmd,
                        context,
                        raw_error,
                    })
                } else {
                    OperationResult::Err(BuildError::GitError {
                        application: build.image.service_id.clone(),
                        git_cmd,
                        context,
                        raw_error,
                    })
                };
            }
            OperationResult::Ok(())
        }) {
            git_clone_record.stop(StepStatus::Error);
            return Err(error.error);
        }
        git_clone_record.stop(StepStatus::Success);

        let _git_cleanup = scopeguard::guard(&repository_root_path, |path| {
            info!("Removing git repository at path: {:?}", path);
            let _ = fs::remove_dir_all(path);
        });

        if abort.status().should_cancel() {
            return Err(BuildError::Aborted {
                application: build.image.service_id.clone(),
            });
        }

        let app_id = build.image.service_id.clone();

        // Fetch git-lfs/big files for the repository if necessary
        let git_lfs = if let Some(creds) = git_user_creds {
            GitLfs::new(creds.login, creds.password)
        } else {
            GitLfs::default()
        };
        let cmd_killer = CommandKiller::from_cancelable(abort);
        let size_estimate_kb = git_lfs
            .files_size_estimate_in_kb(&repository_root_path, &git_repository.commit_id, &cmd_killer)
            .unwrap_or(0);

        if size_estimate_kb > 0 {
            if size_estimate_kb > MAX_GIT_LFS_SIZE_KB {
                return Err(BuildError::InvalidConfig {
                    application: app_id,
                    raw_error_message: format!(
                        "GIT LFS files size are too big and are over the max allowed size of {MAX_GIT_LFS_SIZE_GB} GB"
                    ),
                });
            }

            info!("fetching git-lfs files");
            logger.send_progress("🗜️ Fetching git-lfs files for repository".to_string());
            match git_lfs.checkout_files_for_commit(&repository_root_path, &git_repository.commit_id, &cmd_killer) {
                Ok(_) => {}
                Err(GitLfsError::Aborted { .. }) => return Err(BuildError::Aborted { application: app_id }),
                Err(GitLfsError::Timeout { .. }) => return Err(BuildError::Aborted { application: app_id }),
                Err(GitLfsError::ExecutionError { raw_error }) => {
                    return Err(BuildError::IoError {
                        application: app_id,
                        action_description: "git lfs checkout".to_string(),
                        raw_error,
                    });
                }
                Err(GitLfsError::ExitStatusError { .. }) => {
                    return Err(BuildError::IoError {
                        application: app_id,
                        action_description: "git lfs checkout".to_string(),
                        raw_error: Error::other("git lfs checkout failed"),
                    });
                }
            }
        }

        // Check that the build context is correct
        let build_context_path = repository_root_path.join(&git_repository.root_path);
        if !build_context_path.is_dir() {
            return Err(BuildError::InvalidConfig {
                application: app_id,
                raw_error_message: format!(
                    "Specified build context path {:?} does not exist within the repository",
                    git_repository.root_path
                ),
            });
        }

        // Safety check to ensure we can't go up in the directory
        if !build_context_path
            .canonicalize()
            .unwrap_or_default()
            .starts_with(repository_root_path.canonicalize().unwrap_or_default())
        {
            return Err(BuildError::InvalidConfig {
                application: app_id,
                raw_error_message: format!(
                    "Specified build context path {:?} tries to access directory outside of his git repository",
                    git_repository.root_path,
                ),
            });
        }

        let dockerfile_path = git_repository
            .dockerfile_path
            .as_ref()
            .ok_or(BuildError::InvalidConfig {
                application: app_id.clone(),
                raw_error_message: "Dockerfile path is not defined".to_string(),
            })?;

        let dockerfile_absolute_path = repository_root_path.join(dockerfile_path);

        // if the dockerfile content is provided, write it to the file before building
        if let Some(dockerfile_content) = &git_repository.dockerfile_content {
            let dockerfile_content = Self::inject_custom_build_fragment(
                dockerfile_content,
                &build_context_path,
                build.dockerfile_fragment.as_ref(),
                &app_id,
            )?;

            fs::write(&dockerfile_absolute_path, dockerfile_content).map_err(|err| BuildError::IoError {
                application: app_id.clone(),
                action_description: "writing dockerfile content".to_string(),
                raw_error: err,
            })?;

            if let Some(dockerfile_directory) = dockerfile_absolute_path.parent() {
                let docker_ignore_path = dockerfile_directory.join(".dockerignore");

                fs::write(docker_ignore_path, DOCKER_IGNORE).map_err(|err| BuildError::IoError {
                    application: app_id.clone(),
                    action_description: "writing .dockerignore content".to_string(),
                    raw_error: err,
                })?;
            }
        }

        // if the extra files are provided, write them to the file before building
        for extra_file_to_inject in &git_repository.extra_files_to_inject {
            let extra_file_absolute_path = repository_root_path.join(extra_file_to_inject.path.clone());
            fs::write(&extra_file_absolute_path, &extra_file_to_inject.content).map_err(|err| BuildError::IoError {
                application: app_id.clone(),
                action_description: "writing extra".to_string(),
                raw_error: err,
            })?;
        }

        // If the dockerfile does not exist, abort
        if !dockerfile_absolute_path.is_file() {
            return Err(BuildError::InvalidConfig {
                application: app_id,
                raw_error_message: format!(
                    "Specified dockerfile path {:?} does not exist within the repository",
                    dockerfile_path
                ),
            });
        }

        self.build_image_with_docker(
            build,
            dockerfile_absolute_path.to_str().unwrap_or_default(),
            build_context_path.to_str().unwrap_or_default(),
            logger,
            metrics_registry.clone(),
            abort,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_inject_fragment_with_file_path() {
        let temp_dir = tempdir().unwrap();
        let fragment_content = "RUN apk add --no-cache jq curl\nRUN echo 'custom fragment'";

        // Create the fragment file at a custom path
        fs::write(temp_dir.path().join("custom/fragment.dockerfile"), fragment_content).unwrap_or_else(|_| {
            fs::create_dir_all(temp_dir.path().join("custom")).unwrap();
            fs::write(temp_dir.path().join("custom/fragment.dockerfile"), fragment_content).unwrap();
        });
        fs::create_dir_all(temp_dir.path().join("custom")).unwrap();
        fs::write(temp_dir.path().join("custom/fragment.dockerfile"), fragment_content).unwrap();

        let dockerfile_template = r#"FROM alpine:3.22
WORKDIR /data
COPY . .
#{{custom_fragment}}
RUN chmod +x entrypoint.sh
USER app"#;

        let fragment = Some(DockerfileFragment::File {
            path: "/custom/fragment.dockerfile".to_string(),
        });

        let result = LocalDocker::inject_custom_build_fragment(
            dockerfile_template,
            temp_dir.path(),
            fragment.as_ref(),
            "my_service_id",
        )
        .unwrap();

        assert!(result.contains("RUN apk add --no-cache jq curl"));
        assert!(result.contains("RUN echo 'custom fragment'"));
        assert!(!result.contains("#{{custom_fragment}}"));
    }

    #[test]
    fn test_inject_fragment_with_inline_content() {
        let temp_dir = tempdir().unwrap();
        let inline_content = "RUN apk add --no-cache aws-cli jq curl";

        let dockerfile_template = r#"FROM alpine:3.22
WORKDIR /data
COPY . .
#{{custom_fragment}}
RUN chmod +x entrypoint.sh
USER app"#;

        let fragment = Some(DockerfileFragment::Inline {
            content: inline_content.to_string(),
        });

        let result = LocalDocker::inject_custom_build_fragment(
            dockerfile_template,
            temp_dir.path(),
            fragment.as_ref(),
            "my_service_id",
        )
        .unwrap();

        assert!(result.contains("RUN apk add --no-cache aws-cli jq curl"));
        assert!(!result.contains("#{{custom_fragment}}"));
    }

    #[test]
    fn test_inject_fragment_none_removes_placeholder() {
        let temp_dir = tempdir().unwrap();

        let dockerfile_template = r#"FROM alpine:3.22
WORKDIR /data
COPY . .
#{{custom_fragment}}
RUN chmod +x entrypoint.sh
USER app"#;

        // No fragment configured - V2 behavior: placeholder is replaced with empty string
        let result =
            LocalDocker::inject_custom_build_fragment(dockerfile_template, temp_dir.path(), None, "my_service_id")
                .unwrap();

        // Placeholder should be replaced with empty string
        assert!(!result.contains("#{{custom_fragment}}"));
        // The surrounding structure should remain
        assert!(result.contains("RUN chmod +x entrypoint.sh"));
    }

    #[test]
    fn test_inject_fragment_empty_inline_content() {
        let temp_dir = tempdir().unwrap();

        let dockerfile_template = r#"FROM alpine:3.22
#{{custom_fragment}}
RUN chmod +x entrypoint.sh"#;

        let fragment = Some(DockerfileFragment::Inline { content: String::new() });

        let result = LocalDocker::inject_custom_build_fragment(
            dockerfile_template,
            temp_dir.path(),
            fragment.as_ref(),
            "my_service_id",
        )
        .unwrap();

        assert!(!result.contains("#{{custom_fragment}}"));
        assert!(result.contains("FROM alpine:3.22"));
        assert!(result.contains("RUN chmod +x entrypoint.sh"));
    }

    #[test]
    fn test_inject_fragment_without_placeholder_returns_error() {
        let temp_dir = tempdir().unwrap();

        // Dockerfile without the placeholder
        let dockerfile_template = r#"FROM alpine:3.22
RUN chmod +x entrypoint.sh"#;

        let fragment = Some(DockerfileFragment::Inline {
            content: "RUN apk add jq".to_string(),
        });

        let result = LocalDocker::inject_custom_build_fragment(
            dockerfile_template,
            temp_dir.path(),
            fragment.as_ref(),
            "my_service_id",
        );

        // Should return an error since there's no placeholder but a fragment is provided
        assert!(result.is_err());
        if let Err(BuildError::InvalidConfig { raw_error_message, .. }) = result {
            assert!(raw_error_message.contains("#{{custom_fragment}}"));
        } else {
            panic!("Expected InvalidConfig error for missing placeholder");
        }
    }

    #[test]
    fn test_inject_fragment_file_not_found_returns_error() {
        let temp_dir = tempdir().unwrap();

        let dockerfile_template = "FROM alpine:3.22\n#{{custom_fragment}}\nRUN echo done";

        let fragment = Some(DockerfileFragment::File {
            path: "/non-existent/fragment.dockerfile".to_string(),
        });

        let result = LocalDocker::inject_custom_build_fragment(
            dockerfile_template,
            temp_dir.path(),
            fragment.as_ref(),
            "my_service_id",
        );

        assert!(result.is_err());
        if let Err(BuildError::IoError { action_description, .. }) = result {
            assert!(action_description.contains("non-existent/fragment.dockerfile"));
        } else {
            panic!("Expected IoError for missing fragment file");
        }
    }

    #[test]
    fn test_inject_fragment_inline_preserves_content_exactly() {
        let temp_dir = tempdir().unwrap();
        let fragment_content = "# Install tools\nRUN apk add --no-cache \\\n    jq \\\n    curl";

        let dockerfile_template = "FROM alpine:3.22\n#{{custom_fragment}}\nRUN echo done";

        let fragment = Some(DockerfileFragment::Inline {
            content: fragment_content.to_string(),
        });

        let result = LocalDocker::inject_custom_build_fragment(
            dockerfile_template,
            temp_dir.path(),
            fragment.as_ref(),
            "my_service_id",
        )
        .unwrap();

        // Fragment content should be preserved exactly, including newlines and escapes
        assert!(result.contains(fragment_content));
    }

    /// A source-less build has an empty build context, so its Dockerfile is only ever the
    /// engine-generated content plus the spliced fragment. This is what the agentic workflow relies
    /// on to layer onto its base image without a repository to clone.
    #[test]
    fn inject_fragment_into_a_synthesized_dockerfile_needs_no_build_context() {
        let empty_context = tempdir().unwrap();
        let dockerfile_template =
            format!("FROM public.ecr.aws/r3m4q3r9/qovery-ai-runner:0.0.3\nUSER root\n{CUSTOM_FRAGMENT_PLACEHOLDER}\n");

        let fragment = Some(DockerfileFragment::Inline {
            content: "RUN apt-get update && apt-get install -y jq".to_string(),
        });

        let result = LocalDocker::inject_custom_build_fragment(
            &dockerfile_template,
            empty_context.path(),
            fragment.as_ref(),
            "my_service_id",
        )
        .unwrap();

        assert_eq!(
            result,
            "FROM public.ecr.aws/r3m4q3r9/qovery-ai-runner:0.0.3\nUSER root\nRUN apt-get update && apt-get install -y jq\n"
        );
        assert!(!result.contains(CUSTOM_FRAGMENT_PLACEHOLDER));
    }

    fn secret_mounts(ids: &[&str]) -> DockerfileSecretMounts {
        DockerfileSecretMounts {
            ids: ids.iter().map(|id| id.to_string()).collect(),
            has_mount_without_id: false,
        }
    }

    fn env_vars(vars: &[(&str, &str)]) -> BTreeMap<String, String> {
        vars.iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn test_split_build_inputs_routes_by_dockerfile_declaration() {
        let vars = env_vars(&[
            ("AN_ARG", "arg-value"),
            ("A_SECRET", "secret-value"),
            ("UNUSED", "unused-value"),
        ]);

        let (build_args, secrets) =
            split_build_inputs(&vars, &HashSet::from(["AN_ARG".to_string()]), &secret_mounts(&["A_SECRET"]));

        assert_eq!(build_args, vec![("AN_ARG", "arg-value")]);
        assert_eq!(secrets, vec![("A_SECRET", "secret-value")]);
    }

    #[test]
    fn test_split_build_inputs_sends_a_name_declared_both_ways_to_both() {
        let vars = env_vars(&[("BOTH", "value")]);

        let (build_args, secrets) =
            split_build_inputs(&vars, &HashSet::from(["BOTH".to_string()]), &secret_mounts(&["BOTH"]));

        assert_eq!(build_args, vec![("BOTH", "value")]);
        assert_eq!(secrets, vec![("BOTH", "value")]);
    }

    #[test]
    fn test_split_build_inputs_ignores_an_unmatched_secret_id() {
        // D4: an id with no matching variable produces no flag at all. BuildKit then fails the step
        // only if the mount is declared `required=true`.
        let vars = env_vars(&[("AN_ARG", "arg-value")]);

        let (build_args, secrets) =
            split_build_inputs(&vars, &HashSet::from(["AN_ARG".to_string()]), &secret_mounts(&["NEVER_SET"]));

        assert_eq!(build_args, vec![("AN_ARG", "arg-value")]);
        assert!(secrets.is_empty());
    }

    /// A Dockerfile without a secret mount is the overwhelming majority, and it must not gain a
    /// filesystem write it did not have before build secrets existed.
    #[test]
    fn test_build_secret_files_touches_nothing_when_there_is_no_secret() {
        let secret_files = BuildSecretFiles::new("my_service_id", &[]).unwrap();

        assert!(secret_files._dir.is_none());
        assert!(secret_files.as_build_flags().is_empty());
    }

    #[test]
    fn test_build_secret_files_writes_the_value_verbatim_and_privately() {
        let secret_files = BuildSecretFiles::new("my_service_id", &[("NPM_TOKEN", "s3cr3t"), ("OTHER", "")]).unwrap();
        let flags = secret_files.as_build_flags();

        assert_eq!(flags.len(), 2);
        assert_eq!(flags[0].0, "NPM_TOKEN");
        // No trailing newline: the build step reads the file bytes as the secret value.
        assert_eq!(fs::read(flags[0].1).unwrap(), b"s3cr3t");
        assert_eq!(fs::read(flags[1].1).unwrap(), b"");

        for (_, path) in &flags {
            let mode = fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{path:?} should not be readable by anyone else");

            let dir = path
                .parent()
                .expect("a secret file always sits in the secrets directory");
            let dir_mode = fs::metadata(dir).unwrap().permissions().mode();
            assert_eq!(dir_mode & 0o777, 0o700, "{dir:?} should not be listable by anyone else");
        }
    }

    #[test]
    fn test_build_secret_files_removes_the_files_when_dropped() {
        let paths: Vec<PathBuf> = {
            let secret_files = BuildSecretFiles::new("my_service_id", &[("NPM_TOKEN", "s3cr3t")]).unwrap();
            secret_files
                .as_build_flags()
                .iter()
                .map(|(_, path)| path.to_path_buf())
                .collect()
        };

        for path in paths {
            assert!(!path.exists(), "{path:?} should have been removed with its directory");
        }
    }
}
