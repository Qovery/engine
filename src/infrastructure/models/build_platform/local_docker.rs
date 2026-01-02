#![allow(clippy::redundant_closure)]

use std::io::Error;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use std::{fs, thread};

use git2::{Cred, CredentialType, ErrorClass};
use retry::OperationResult;
use retry::delay::Fibonacci;
use time::Instant;
use uuid::Uuid;

use crate::cmd::command::CommandKiller;
use crate::cmd::docker;
use crate::cmd::docker::{Architecture, BuilderHandle, ContainerImage};
use crate::cmd::git_lfs::{GitLfs, GitLfsError};
use crate::environment::report::logger::EnvLogger;
use crate::infrastructure::models::build_platform::dockerfile_utils::extract_dockerfile_args;
use crate::infrastructure::models::build_platform::{
    Build, BuildError, BuildPlatform, DockerfileFragment, Kind, to_build_error,
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

/// Placeholder in Dockerfile templates that gets replaced with custom fragment content.
/// Used by the inject_custom_build_fragment function.
const CUSTOM_FRAGMENT_PLACEHOLDER: &str = "#{{custom_fragment}}";

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
    ) -> Result<String, BuildError> {
        if !dockerfile_content.contains(CUSTOM_FRAGMENT_PLACEHOLDER) && dockerfile_fragment.is_some() {
            return Err(BuildError::InvalidConfig {
                application: "terraform".to_string(),
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
                            application: "terraform".to_string(),
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
        let dockerfile_args = match extract_dockerfile_args(dockerfile_content) {
            Ok(dockerfile_args) => dockerfile_args,
            Err(err) => {
                build_record.stop(StepStatus::Error);
                return Err(BuildError::InvalidConfig {
                    application: build.image.service_id.clone(),
                    raw_error_message: format!("Cannot extract env vars from your dockerfile {err}"),
                });
            }
        };

        // Keep only the env variables we want for our build
        // and force re-compute the image tag
        build.environment_variables.retain(|k, _| dockerfile_args.contains(k));
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
                vec!["cache".to_string()],
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
        let env_vars: Vec<(&str, &str)> = build
            .environment_variables
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let arch: Vec<Architecture> = build
            .architectures
            .iter()
            .map(|arch| docker::Architecture::from(arch))
            .collect();

        let builder_handle =
            self.provision_builder(build, |line| logger.send_progress(line), &CommandKiller::from_cancelable(abort))?;

        let exit_status = self.context.docker.build(
            &builder_handle.builder_name.as_deref(),
            Path::new(dockerfile_complete_path),
            Path::new(into_dir_docker_style),
            &image_to_build,
            &env_vars,
            image_cache.as_ref(),
            true,
            &arch,
            &mut |line| logger.send_progress(line),
            &mut |line| logger.send_progress(line),
            &CommandKiller::from(build.timeout, abort),
            build.git_repository.docker_target_build_stage.as_ref(),
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

        // LOGGING
        let repository_root_path = self.get_repository_build_root_path(build)?;
        logger.send_progress(format!("📥 Cloning repository {}", build.git_repository.url));

        // Retrieve git credentials
        let git_user_creds = match build.git_repository.credentials() {
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
            let mut creds: Vec<(CredentialType, Cred)> = Vec::with_capacity(build.git_repository.ssh_keys.len() + 1);
            for ssh_key in build.git_repository.ssh_keys.iter() {
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
                &build.git_repository.url,
                &build.git_repository.commit_id,
                &repository_root_path,
                &get_credentials,
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
            .files_size_estimate_in_kb(&repository_root_path, &build.git_repository.commit_id, &cmd_killer)
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
            match git_lfs.checkout_files_for_commit(&repository_root_path, &build.git_repository.commit_id, &cmd_killer)
            {
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
        let build_context_path = repository_root_path.join(&build.git_repository.root_path);
        if !build_context_path.is_dir() {
            return Err(BuildError::InvalidConfig {
                application: app_id,
                raw_error_message: format!(
                    "Specified build context path {:?} does not exist within the repository",
                    &build.git_repository.root_path
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
                    &build.git_repository.root_path,
                ),
            });
        }

        let dockerfile_path = build
            .git_repository
            .dockerfile_path
            .as_ref()
            .ok_or(BuildError::InvalidConfig {
                application: app_id.clone(),
                raw_error_message: "Dockerfile path is not defined".to_string(),
            })?;

        let dockerfile_absolute_path = repository_root_path.join(dockerfile_path);

        // if the dockerfile content is provided, write it to the file before building
        if let Some(dockerfile_content) = &build.git_repository.dockerfile_content {
            let dockerfile_content = Self::inject_custom_build_fragment(
                dockerfile_content,
                &build_context_path,
                build.dockerfile_fragment.as_ref(),
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
        for extra_file_to_inject in &build.git_repository.extra_files_to_inject {
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
                    &dockerfile_path
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

        let result =
            LocalDocker::inject_custom_build_fragment(dockerfile_template, temp_dir.path(), fragment.as_ref()).unwrap();

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

        let result =
            LocalDocker::inject_custom_build_fragment(dockerfile_template, temp_dir.path(), fragment.as_ref()).unwrap();

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
        let result = LocalDocker::inject_custom_build_fragment(dockerfile_template, temp_dir.path(), None).unwrap();

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

        let result =
            LocalDocker::inject_custom_build_fragment(dockerfile_template, temp_dir.path(), fragment.as_ref()).unwrap();

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

        let result = LocalDocker::inject_custom_build_fragment(dockerfile_template, temp_dir.path(), fragment.as_ref());

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

        let result = LocalDocker::inject_custom_build_fragment(dockerfile_template, temp_dir.path(), fragment.as_ref());

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

        let result =
            LocalDocker::inject_custom_build_fragment(dockerfile_template, temp_dir.path(), fragment.as_ref()).unwrap();

        // Fragment content should be preserved exactly, including newlines and escapes
        assert!(result.contains(fragment_content));
    }
}
