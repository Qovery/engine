#![allow(clippy::redundant_closure)]

use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::{env, fs};

use git2::{Cred, CredentialType};
use sysinfo::{DiskExt, RefreshKind, SystemExt};
use uuid::Uuid;

use crate::build_platform::dockerfile_utils::extract_dockerfile_args;
use crate::build_platform::{Build, BuildError, BuildPlatform, Credentials, Kind};
use crate::cmd::command;
use crate::cmd::command::CommandError::Killed;
use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::cmd::docker::{BuildResult, ContainerImage, DockerError};
use crate::events::{EngineEvent, EventMessage, Transmitter};
use crate::fs::workspace_directory;
use crate::git;
use crate::io_models::context::Context;
use crate::io_models::progress_listener::{
    Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::logger::Logger;
use crate::utilities::to_short_id;

/// https://buildpacks.io/
const BUILDPACKS_BUILDERS: [&str; 1] = [
    "heroku/buildpacks:20",
    // removed because it does not support dynamic port binding
    //"gcr.io/buildpacks/builder:v1",
    //"paketobuildpacks/builder:base",
];

/// use Docker in local
pub struct LocalDocker {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl LocalDocker {
    pub fn new(context: Context, long_id: Uuid, name: &str, logger: Box<dyn Logger>) -> Result<Self, BuildError> {
        Ok(LocalDocker {
            context,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            listeners: vec![],
            logger,
        })
    }

    fn get_docker_host_envs(&self) -> Vec<(&str, &str)> {
        if let Some(socket_path) = self.context.docker_tcp_socket() {
            vec![("DOCKER_HOST", socket_path.as_str())]
        } else {
            vec![]
        }
    }

    fn reclaim_space_if_needed(&self) {
        // ensure there is enough disk space left before building a new image
        // For CI, we should skip this job
        if env::var_os("CI").is_some() {
            self.logger.log(EngineEvent::Info(
                self.get_event_details(),
                EventMessage::new_from_safe("CI environment variable found, no docker prune will be made".to_string()),
            ));

            return;
        }

        // arbitrary percentage that should make the job anytime
        const DISK_FREE_SPACE_PERCENTAGE_BEFORE_PURGE: u64 = 40;
        let mount_points_to_check = vec![Path::new("/var/lib/docker"), Path::new("/")];
        let mut disk_free_space_percent: u64 = 100;

        let sys_info = sysinfo::System::new_with_specifics(RefreshKind::new().with_disks().with_disks_list());
        let should_reclaim_space = sys_info.disks().iter().any(|disk| {
            // Check disk own the mount point we are interested in
            if !mount_points_to_check.contains(&disk.mount_point()) {
                return false;
            }

            // Check if we have hit our threshold regarding remaining disk space
            disk_free_space_percent = disk.available_space() * 100 / disk.total_space();
            if disk_free_space_percent <= DISK_FREE_SPACE_PERCENTAGE_BEFORE_PURGE {
                return true;
            }

            false
        });

        if !should_reclaim_space {
            debug!(
                "Docker skipping image purge, still {} % disk free space",
                disk_free_space_percent
            );
            return;
        }

        let msg = format!(
            "Purging docker images to reclaim disk space. Only {} % disk free space, This may take some time",
            disk_free_space_percent
        );
        self.logger
            .log(EngineEvent::Info(self.get_event_details(), EventMessage::new_from_safe(msg)));

        // Request a purge if a disk is being low on space
        if let Err(err) = self.context.docker.prune_images() {
            let msg = format!("Error while purging docker images: {}", err);
            self.logger
                .log(EngineEvent::Warning(self.get_event_details(), EventMessage::new_from_safe(msg)));
        }
    }

    fn build_image_with_docker(
        &self,
        build: &mut Build,
        dockerfile_complete_path: &str,
        into_dir_docker_style: &str,
        lh: &ListenersHelper,
        is_task_canceled: &dyn Fn() -> bool,
    ) -> Result<BuildResult, BuildError> {
        // logger
        let log_info = {
            let app_id = build.image.application_id.clone();
            move |msg: String| {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(),
                    EventMessage::new_from_safe(msg.clone()),
                ));

                lh.deployment_in_progress(ProgressInfo::new(
                    ProgressScope::Application { id: app_id.clone() },
                    ProgressLevel::Info,
                    Some(msg),
                    self.context.execution_id(),
                ));
            }
        };

        // Going to inject only env var that are used by the dockerfile
        // so extracting it and modifying the image tag and env variables
        let dockerfile_content = fs::read(dockerfile_complete_path).map_err(|err| BuildError::IoError {
            application: build.image.application_id.clone(),
            action_description: "reading dockerfile content".to_string(),
            raw_error: err,
        })?;
        let dockerfile_args = match extract_dockerfile_args(dockerfile_content) {
            Ok(dockerfile_args) => dockerfile_args,
            Err(err) => {
                return Err(BuildError::InvalidConfig {
                    application: build.image.application_id.clone(),
                    raw_error_message: format!("Cannot extract env vars from your dockerfile {}", err),
                });
            }
        };

        // Keep only the env variables we want for our build
        // and force re-compute the image tag
        build.environment_variables.retain(|k, _| dockerfile_args.contains(k));
        build.compute_image_tag();

        let mut build_result = BuildResult::new();

        // Prepare image we want to build
        let image_to_build = ContainerImage {
            registry: build.image.registry_url.clone(),
            name: build.image.name(),
            tags: vec![build.image.tag.clone(), "latest".to_string()],
        };
        build_result.build_candidate_image(Some(image_to_build.clone()));

        let image_cache = ContainerImage {
            registry: build.image.registry_url.clone(),
            name: build.image.name(),
            tags: vec!["latest".to_string()],
        };
        build_result.source_cached_image(Some(image_cache.clone()));

        // Check if the image does not exist already remotely, if yes, we skip the build
        let image_name = image_to_build.image_name();
        log_info(format!("🕵️ Checking if image already exist remotely {}", image_name));
        if let Ok(true) = self.context.docker.does_image_exist_remotely(&image_to_build) {
            log_info(format!("🎯 Skipping build. Image already exist in the registry {}", image_name));

            // skip build
            build_result.image_exists_remotely(true);
            return Ok(build_result);
        }

        log_info(format!("⛏️ Building image. It does not exist remotely {}", image_name));

        // Actually do the build of the image
        let env_vars: Vec<(&str, &str)> = build
            .environment_variables
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let exit_status = self.context.docker.build(
            Path::new(dockerfile_complete_path),
            Path::new(into_dir_docker_style),
            &image_to_build,
            &env_vars,
            &image_cache,
            true,
            &mut |line| log_info(line),
            &mut |line| log_info(line),
            &CommandKiller::from(build.timeout, is_task_canceled),
        );

        match exit_status {
            Ok(build_result) => Ok(build_result),
            Err(DockerError::Aborted { .. }) => Err(BuildError::Aborted {
                application: build.image.application_id.clone(),
            }),
            Err(err) => Err(BuildError::DockerError {
                application: build.image.application_id.clone(),
                raw_error: err,
            }),
        }
    }

    fn build_image_with_buildpacks(
        &self,
        build: &Build,
        into_dir_docker_style: &str,
        use_build_cache: bool,
        lh: &ListenersHelper,
        is_task_canceled: &dyn Fn() -> bool,
    ) -> Result<BuildResult, BuildError> {
        const LATEST_TAG: &str = "latest";
        let name_with_tag = build.image.full_image_name_with_tag();
        let container_image = ContainerImage::new(
            build.image.registry_url.clone(),
            build.image.name.to_string(),
            vec![build.image.tag.to_string()],
        );
        let container_image_cache = ContainerImage::new(
            build.image.registry_url.clone(),
            build.image.name.to_string(),
            vec![LATEST_TAG.to_string()],
        );
        let name_with_latest_tag = format!("{}:{}", build.image.full_image_name(), LATEST_TAG);
        let mut build_result = BuildResult::new();
        build_result.build_candidate_image(Some(container_image));
        build_result.source_cached_image(Some(container_image_cache));

        let mut exit_status: Result<(), command::CommandError> = Err(command::CommandError::ExecutionError(
            Error::new(ErrorKind::InvalidData, "No builder names".to_string()),
        ));

        for builder_name in BUILDPACKS_BUILDERS.iter() {
            let mut buildpacks_args = if !use_build_cache {
                vec!["build", "--publish", name_with_tag.as_str(), "--clear-cache"]
            } else {
                vec!["build", "--publish", name_with_tag.as_str()]
            };

            // always add 'latest' tag
            buildpacks_args.extend(vec!["-t", name_with_latest_tag.as_str()]);
            buildpacks_args.extend(vec!["--path", into_dir_docker_style]);

            let mut args_buffer = Vec::with_capacity(build.environment_variables.len());
            for (key, value) in &build.environment_variables {
                args_buffer.push("--env".to_string());
                args_buffer.push(format!("{}={}", key, value));
            }
            buildpacks_args.extend(args_buffer.iter().map(|value| value.as_str()).collect::<Vec<&str>>());

            buildpacks_args.push("-B");
            buildpacks_args.push(builder_name);
            if let Some(buildpacks_language) = &build.git_repository.buildpack_language {
                buildpacks_args.push("-b");
                match buildpacks_language.split('@').collect::<Vec<&str>>().as_slice() {
                    [builder] => {
                        // no version specified, so we use the latest builder
                        buildpacks_args.push(builder);
                    }
                    [builder, _version] => {
                        // version specified, we need to use the specified builder
                        // but also ensure that the user has set the correct runtime version in his project
                        // this is language dependent
                        // https://elements.heroku.com/buildpacks/heroku/heroku-buildpack-python
                        // https://devcenter.heroku.com/articles/buildpacks
                        // TODO: Check user project is correctly configured for this builder and version
                        buildpacks_args.push(builder);
                    }
                    _ => {
                        return Err(BuildError::InvalidConfig {
                            application: build.image.application_id.clone(),
                            raw_error_message: format!(
                                "Invalid buildpacks language format: expected `builder[@version]` got {}",
                                buildpacks_language
                            ),
                        });
                    }
                }
            }

            // Just a fallback for now to help our bot loving users deploy their apps
            // Long term solution requires lots of changes in UI and Core as well
            // And passing some params to the engine
            if let Ok(content) = fs::read_to_string(format!("{}/{}", into_dir_docker_style, "Procfile")) {
                if content.contains("worker") {
                    buildpacks_args.push("--default-process");
                    buildpacks_args.push("worker");
                }
            }

            // buildpacks build
            let mut cmd = QoveryCommand::new("pack", &buildpacks_args, &self.get_docker_host_envs());
            let cmd_killer = CommandKiller::from(build.timeout, is_task_canceled);
            exit_status = cmd.exec_with_abort(
                &mut |line| {
                    self.logger.log(EngineEvent::Info(
                        self.get_event_details(),
                        EventMessage::new_from_safe(line.to_string()),
                    ));

                    lh.deployment_in_progress(ProgressInfo::new(
                        ProgressScope::Application {
                            id: build.image.application_id.clone(),
                        },
                        ProgressLevel::Info,
                        Some(line),
                        self.context.execution_id(),
                    ));
                },
                &mut |line| {
                    self.logger.log(EngineEvent::Warning(
                        self.get_event_details(),
                        EventMessage::new_from_safe(line.to_string()),
                    ));

                    lh.deployment_in_progress(ProgressInfo::new(
                        ProgressScope::Application {
                            id: build.image.application_id.clone(),
                        },
                        ProgressLevel::Warn,
                        Some(line),
                        self.context.execution_id(),
                    ));
                },
                &cmd_killer,
            );

            if exit_status.is_ok() {
                // quit now if the builder successfully build the app
                break;
            }
        }

        match exit_status {
            Ok(_) => {
                build_result.built(true);
                Ok(build_result)
            }
            Err(Killed(_)) => Err(BuildError::Aborted {
                application: build.image.application_id.clone(),
            }),
            Err(err) => Err(BuildError::BuildpackError {
                application: build.image.application_id.clone(),
                raw_error: err,
            }),
        }
    }

    fn get_repository_build_root_path(&self, build: &Build) -> Result<String, BuildError> {
        workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("build/{}", build.image.name.as_str()),
        )
        .map_err(|err| BuildError::IoError {
            application: build.image.application_id.clone(),
            action_description: "when creating build workspace".to_string(),
            raw_error: err,
        })
    }
}

impl BuildPlatform for LocalDocker {
    fn context(&self) -> &Context {
        &self.context
    }

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

    fn build(&self, build: &mut Build, is_task_canceled: &dyn Fn() -> bool) -> Result<BuildResult, BuildError> {
        let event_details = self.get_event_details();
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let app_id = build.image.application_id.clone();

        // check if we should already abort the task
        if is_task_canceled() {
            return Err(BuildError::Aborted {
                application: build.image.application_id.clone(),
            });
        }

        // LOGGING
        let repository_root_path = PathBuf::from(self.get_repository_build_root_path(build)?);
        let msg = format!(
            "📥 Cloning repository: {} to {}",
            build.git_repository.url,
            repository_root_path.to_string_lossy()
        );
        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application { id: app_id.clone() },
            ProgressLevel::Info,
            Some(msg.clone()),
            self.context.execution_id(),
        ));
        self.logger
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));

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

            if let Some(Credentials { login, password }) = &build.git_repository.credentials {
                creds.push((
                    CredentialType::USER_PASS_PLAINTEXT,
                    Cred::userpass_plaintext(login, password).unwrap(),
                ));
            }

            creds
        };

        // Cleanup, mono repo can require to clone multiple time the same repo
        // FIXME: re-use the same repo and just checkout at the correct commit
        if repository_root_path.exists() {
            let app_id = app_id;
            fs::remove_dir_all(&repository_root_path).map_err(|err| BuildError::IoError {
                application: app_id,
                action_description: "cleaning old repository".to_string(),
                raw_error: err,
            })?;
        }

        // Do the real git clone
        if let Err(clone_error) = git::clone_at_commit(
            &build.git_repository.url,
            &build.git_repository.commit_id,
            &repository_root_path,
            &get_credentials,
        ) {
            return Err(BuildError::GitError {
                application: build.image.application_id.clone(),
                raw_error: clone_error,
            });
        }

        if is_task_canceled() {
            return Err(BuildError::Aborted {
                application: build.image.application_id.clone(),
            });
        }

        // ensure docker_path is a mounted volume, otherwise ignore because it's not what Qovery does in production
        // ex: this cause regular cleanup on CI, leading to random tests errors
        self.reclaim_space_if_needed();

        let app_id = build.image.application_id.clone();

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

        // now we have to decide if we use buildpack or docker to build our application
        // If no Dockerfile specified, we should use BuildPacks
        let result = if let Some(dockerfile_path) = &build.git_repository.dockerfile_path {
            // build container from the provided Dockerfile

            let dockerfile_absolute_path = repository_root_path.join(dockerfile_path);

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
                &listeners_helper,
                is_task_canceled,
            )
        } else {
            // build container with Buildpacks
            self.build_image_with_buildpacks(
                build,
                build_context_path.to_str().unwrap_or_default(),
                !build.disable_cache,
                &listeners_helper,
                is_task_canceled,
            )
        };

        // log image building infos
        if let Ok(build_result) = &result {
            listeners_helper.deployment_in_progress(ProgressInfo::new(
                ProgressScope::Application { id: app_id },
                ProgressLevel::Info,
                Some(build_result.to_string()),
                self.context.execution_id(),
            ));
            self.logger.log(EngineEvent::Info(
                event_details,
                EventMessage::new_from_safe(build_result.to_string()),
            ));
        }

        result
    }

    fn logger(&self) -> Box<dyn Logger> {
        self.logger.clone()
    }
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    fn to_transmitter(&self) -> Transmitter {
        Transmitter::BuildPlatform(self.long_id, self.name().to_string())
    }
}
