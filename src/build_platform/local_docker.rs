use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs};

use git2::{Cred, CredentialType};
use sysinfo::{Disk, DiskExt, SystemExt};

use crate::build_platform::{docker, Build, BuildPlatform, Credentials, Kind};
use crate::cmd::command;
use crate::cmd::command::CommandError::Killed;
use crate::cmd::command::{CommandKiller, QoveryCommand};
use crate::cmd::docker::{ContainerImage, Docker, DockerError};
use crate::errors::{CommandError, EngineError, Tag};
use crate::events::{EngineEvent, EventDetails, EventMessage, ToTransmitter, Transmitter};
use crate::fs::workspace_directory;
use crate::git;
use crate::logger::{LogLevel, Logger};
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};

const BUILD_DURATION_TIMEOUT_SEC: u64 = 30 * 60;

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
    name: String,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl LocalDocker {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        logger: Box<dyn Logger>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(LocalDocker {
            context,
            id: id.to_string(),
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

    /// Read Dockerfile content from location path and return an array of bytes
    fn get_dockerfile_content(&self, dockerfile_path: &str) -> Result<Vec<u8>, EngineError> {
        match fs::read(dockerfile_path) {
            Ok(bytes) => Ok(bytes),
            Err(err) => {
                let engine_error = EngineError::new_docker_cannot_read_dockerfile(
                    self.get_event_details(),
                    dockerfile_path.to_string(),
                    CommandError::new(err.to_string(), None),
                );
                self.logger
                    .log(LogLevel::Error, EngineEvent::Error(engine_error.clone(), None));
                Err(engine_error)
            }
        }
    }

    fn build_image_with_docker(
        &self,
        build: &Build,
        dockerfile_complete_path: &str,
        into_dir_docker_style: &str,
        env_var_args: Vec<String>,
        lh: &ListenersHelper,
        is_task_canceled: &dyn Fn() -> bool,
    ) -> Result<(), EngineError> {
        let image_to_build = ContainerImage {
            registry: build.image.registry_url.clone(),
            name: build.image.name(),
            tags: vec![build.image.tag.clone(), "latest".to_string()],
        };

        let image_cache = ContainerImage {
            registry: build.image.registry_url.clone(),
            name: build.image.name(),
            tags: vec!["latest".to_string()],
        };

        let dockerfile_content = self.get_dockerfile_content(dockerfile_complete_path)?;
        let env_var_args = match docker::match_used_env_var_args(env_var_args, dockerfile_content) {
            Ok(env_var_args) => env_var_args,
            Err(err) => {
                let engine_error = EngineError::new_docker_cannot_extract_env_vars_from_dockerfile(
                    self.get_event_details(),
                    dockerfile_complete_path.to_string(),
                    CommandError::new(err.to_string(), None),
                );
                self.logger
                    .log(LogLevel::Error, EngineEvent::Error(engine_error.clone(), None));
                return Err(engine_error);
            }
        };

        // FIXME: pass a Vec<(key, value)> instead of spliting always the string
        let env_vars = env_var_args
            .into_iter()
            .map(|val| {
                let (key, value) = val.rsplit_once('=').unwrap();
                (key.to_string(), value.to_string())
            })
            .collect::<Vec<_>>();

        let exit_status = self.context.docker.build(
            &Path::new(dockerfile_complete_path),
            &Path::new(into_dir_docker_style),
            &image_to_build,
            &env_vars
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect::<Vec<_>>(),
            &image_cache,
            true,
            &mut |line| {
                self.logger.log(
                    LogLevel::Info,
                    EngineEvent::Info(self.get_event_details(), EventMessage::new_from_safe(line.to_string())),
                );

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
                self.logger.log(
                    LogLevel::Info,
                    EngineEvent::Info(self.get_event_details(), EventMessage::new_from_safe(line.to_string())),
                );

                lh.deployment_in_progress(ProgressInfo::new(
                    ProgressScope::Application {
                        id: build.image.application_id.clone(),
                    },
                    ProgressLevel::Info,
                    Some(line),
                    self.context.execution_id(),
                ));
            },
            &CommandKiller::from(Duration::from_secs(BUILD_DURATION_TIMEOUT_SEC), is_task_canceled),
        );

        match exit_status {
            Ok(_) => Ok(()),
            Err(DockerError::Aborted(_)) => Err(EngineError::new_task_cancellation_requested(self.get_event_details())),
            Err(err) => Err(EngineError::new_docker_cannot_build_container_image(
                self.get_event_details(),
                self.name_with_id(),
                CommandError::new(format!("{:?}", err), None),
            )),
        }
    }

    fn build_image_with_buildpacks(
        &self,
        build: &Build,
        into_dir_docker_style: &str,
        env_var_args: Vec<String>,
        use_build_cache: bool,
        lh: &ListenersHelper,
        is_task_canceled: &dyn Fn() -> bool,
    ) -> Result<(), EngineError> {
        let name_with_tag = build.image.full_image_name_with_tag();
        let name_with_latest_tag = format!("{}:latest", build.image.full_image_name());

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

            let mut buildpacks_args = if env_var_args.is_empty() {
                buildpacks_args
            } else {
                let mut build_args = vec![];

                env_var_args.iter().for_each(|x| {
                    build_args.push("--env");
                    build_args.push(x.as_str());
                });

                buildpacks_args.extend(build_args);
                buildpacks_args
            };

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
                        let msg = format!(
                            "Cannot build: Invalid buildpacks language format: expected `builder[@version]` got {}",
                            buildpacks_language
                        );
                        lh.deployment_error(ProgressInfo::new(
                            ProgressScope::Application {
                                id: build.image.application_id.clone(),
                            },
                            ProgressLevel::Error,
                            Some(msg.clone()),
                            self.context.execution_id(),
                        ));

                        let err = EngineError::new_buildpack_invalid_language_format(
                            self.get_event_details(),
                            buildpacks_language.to_string(),
                        );

                        self.logger.log(LogLevel::Error, EngineEvent::Error(err.clone(), None));

                        return Err(err);
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
            let cmd_killer = CommandKiller::from(Duration::from_secs(BUILD_DURATION_TIMEOUT_SEC), is_task_canceled);
            exit_status = cmd.exec_with_abort(
                &mut |line| {
                    self.logger.log(
                        LogLevel::Info,
                        EngineEvent::Info(self.get_event_details(), EventMessage::new_from_safe(line.to_string())),
                    );

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
                    self.logger.log(
                        LogLevel::Warning,
                        EngineEvent::Warning(self.get_event_details(), EventMessage::new_from_safe(line.to_string())),
                    );

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
            Ok(_) => Ok(()),
            Err(Killed(_)) => Err(EngineError::new_task_cancellation_requested(self.get_event_details())),
            Err(err) => {
                let error = EngineError::new_buildpack_cannot_build_container_image(
                    self.get_event_details(),
                    self.name_with_id(),
                    BUILDPACKS_BUILDERS.iter().map(|b| b.to_string()).collect(),
                    CommandError::new(format!("{:?}", err), None),
                );

                self.logger
                    .log(LogLevel::Error, EngineEvent::Error(error.clone(), None));

                Err(error)
            }
        }
    }

    fn get_repository_build_root_path(&self, build: &Build) -> Result<String, EngineError> {
        workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("build/{}", build.image.name.as_str()),
        )
        .map_err(|err| {
            EngineError::new_cannot_get_workspace_directory(
                self.get_event_details(),
                CommandError::new(err.to_string(), None),
            )
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

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        if !crate::cmd::command::does_binary_exist("docker") {
            return Err(EngineError::new_missing_required_binary(
                self.get_event_details(),
                "docker".to_string(),
            ));
        }

        if !crate::cmd::command::does_binary_exist("pack") {
            return Err(EngineError::new_missing_required_binary(
                self.get_event_details(),
                "pack".to_string(),
            ));
        }

        Ok(())
    }

    fn build(&self, build: &Build, is_task_canceled: &dyn Fn() -> bool) -> Result<(), EngineError> {
        let event_details = self.get_event_details();
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let app_id = build.image.application_id.clone();

        // check if we should already abort the task
        if is_task_canceled() {
            return Err(EngineError::new_task_cancellation_requested(event_details.clone()));
        }

        // LOGGING
        let repository_root_path = PathBuf::from(self.get_repository_build_root_path(&build)?);
        let msg = format!(
            "Cloning repository: {} to {:?}",
            build.git_repository.url, repository_root_path
        );
        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application { id: app_id },
            ProgressLevel::Info,
            Some(msg.clone()),
            self.context.execution_id(),
        ));
        self.logger.log(
            LogLevel::Info,
            EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)),
        );
        // LOGGING

        // Create callback that will be called by git to provide credentials per user
        // If people use submodule, they need to provide us their ssh key
        let get_credentials = |user: &str| {
            let mut creds: Vec<(CredentialType, Cred)> = Vec::with_capacity(build.git_repository.ssh_keys.len() + 1);
            for ssh_key in build.git_repository.ssh_keys.iter() {
                let public_key = ssh_key.public_key.as_ref().map(|x| x.as_str());
                let passphrase = ssh_key.passphrase.as_ref().map(|x| x.as_str());
                if let Ok(cred) = Cred::ssh_key_from_memory(user, public_key, &ssh_key.private_key, passphrase) {
                    creds.push((CredentialType::SSH_MEMORY, cred));
                }
            }

            if let Some(Credentials { login, password }) = &build.git_repository.credentials {
                creds.push((
                    CredentialType::USER_PASS_PLAINTEXT,
                    Cred::userpass_plaintext(&login, &password).unwrap(),
                ));
            }

            creds
        };

        // Cleanup, mono repo can require to clone multiple time the same repo
        // FIXME: re-use the same repo and just checkout at the correct commit
        if repository_root_path.exists() {
            let _ = fs::remove_dir_all(&repository_root_path);
        }

        // Do the real git clone
        if let Err(clone_error) = git::clone_at_commit(
            &build.git_repository.url,
            &build.git_repository.commit_id,
            &repository_root_path,
            &get_credentials,
        ) {
            let error = EngineError::new_builder_clone_repository_error(
                self.get_event_details(),
                build.git_repository.url.to_string(),
                CommandError::new(clone_error.to_string(), None),
            );

            self.logger
                .log(LogLevel::Error, EngineEvent::Error(error.clone(), None));

            return Err(error);
        }

        if is_task_canceled() {
            return Err(EngineError::new_task_cancellation_requested(event_details.clone()));
        }

        let mut disable_build_cache = false;
        let mut env_var_args: Vec<String> = Vec::with_capacity(build.options.environment_variables.len());

        for ev in &build.options.environment_variables {
            if ev.key == "QOVERY_DISABLE_BUILD_CACHE" && ev.value.to_lowercase() == "true" {
                // this is a special flag to disable build cache dynamically
                // -- do not pass this env var key/value to as build parameter
                disable_build_cache = true;
            } else {
                env_var_args.push(format!("{}={}", ev.key, ev.value));
            }
        }

        // ensure docker_path is a mounted volume, otherwise ignore because it's not what Qovery does in production
        // ex: this cause regular cleanup on CI, leading to random tests errors
        match env::var_os("CI") {
            Some(_) => self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(
                        "CI environment variable found, no docker prune will be made".to_string(),
                    ),
                ),
            ),
            None => {
                // ensure there is enough disk space left before building a new image
                let docker_path_string = "/var/lib/docker";
                let docker_path = Path::new(docker_path_string);

                // get system info
                let mut system = sysinfo::System::new_all();
                system.refresh_all();

                for disk in system.get_disks() {
                    if disk.get_mount_point() == docker_path {
                        let event_details = self.get_event_details();
                        if let Err(e) = check_docker_space_usage_and_clean(
                            &self.context.docker,
                            disk,
                            event_details.clone(),
                            &*self.logger(),
                        ) {
                            self.logger.log(
                                LogLevel::Warning,
                                EngineEvent::Warning(
                                    event_details.clone(),
                                    EventMessage::new(e.to_string(), Some(e.to_string())),
                                ),
                            );
                        }
                        break;
                    };
                }
            }
        }

        let app_id = build.image.application_id.clone();

        // Check that the build context is correct
        let build_context_path = repository_root_path.join(&build.git_repository.root_path);
        if !build_context_path.is_dir() {
            listeners_helper.error(ProgressInfo::new(
                ProgressScope::Application { id: app_id.clone() },
                ProgressLevel::Error,
                Some(format!(
                    "Application build context is not present at location {:?}",
                    build_context_path
                )),
                self.context.execution_id(),
            ));

            let error = EngineError::new_docker_cannot_find_dockerfile(
                self.get_event_details(),
                build_context_path.to_str().unwrap_or_default().to_string(),
            );

            self.logger
                .log(LogLevel::Error, EngineEvent::Error(error.clone(), None));

            return Err(error);
        }

        // now we have to decide if we use buildpack or docker to build our application
        // if dockerfile_path is not present it means we need to use buildpack

        // If no Dockerfile specified, we should use BuildPacks
        let result = if let Some(dockerfile_path) = &build.git_repository.dockerfile_path {
            // build container from the provided Dockerfile

            let dockerfile_absolute_path = repository_root_path.join(dockerfile_path);

            // If the dockerfile does not exist, abort
            if !dockerfile_absolute_path.is_file() {
                listeners_helper.error(ProgressInfo::new(
                    ProgressScope::Application {
                        id: build.image.application_id.clone(),
                    },
                    ProgressLevel::Error,
                    Some(format!(
                        "Dockerfile is not present at location {}",
                        dockerfile_absolute_path.display()
                    )),
                    self.context.execution_id(),
                ));

                let error = EngineError::new_docker_cannot_find_dockerfile(
                    self.get_event_details(),
                    dockerfile_absolute_path.to_str().unwrap_or_default().to_string(),
                );

                self.logger
                    .log(LogLevel::Error, EngineEvent::Error(error.clone(), None));

                return Err(error);
            }

            self.build_image_with_docker(
                build,
                dockerfile_absolute_path.to_str().unwrap_or_default(),
                build_context_path.to_str().unwrap_or_default(),
                env_var_args,
                &listeners_helper,
                is_task_canceled,
            )
        } else {
            // build container with Buildpacks
            self.build_image_with_buildpacks(
                build,
                build_context_path.to_str().unwrap_or_default(),
                env_var_args,
                !disable_build_cache,
                &listeners_helper,
                is_task_canceled,
            )
        };

        let msg = match &result {
            Ok(_) => format!("✅ Container {} is built", self.name_with_id()),
            Err(engine_err) if engine_err.tag() == &Tag::TaskCancellationRequested => {
                format!("🚫 Container {} build has been canceled", self.name_with_id())
            }
            Err(engine_err) => {
                format!(
                    "❌ Container {} failed to be build: {}",
                    self.name_with_id(),
                    engine_err.message()
                )
            }
        };

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application { id: app_id },
            ProgressLevel::Info,
            Some(msg.to_string()),
            self.context.execution_id(),
        ));

        self.logger.log(
            LogLevel::Info,
            EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg.to_string())),
        );

        result
    }

    fn logger(&self) -> Box<dyn Logger> {
        self.logger.clone()
    }
}

impl Listen for LocalDocker {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

impl ToTransmitter for LocalDocker {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::BuildPlatform(self.id().to_string(), self.name().to_string())
    }
}

fn check_docker_space_usage_and_clean(
    docker: &Docker,
    docker_path_size_info: &Disk,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<(), DockerError> {
    let docker_max_disk_percentage_usage_before_purge = 60; // arbitrary percentage that should make the job anytime
    let available_space = docker_path_size_info.get_available_space();
    let docker_percentage_remaining = available_space * 100 / docker_path_size_info.get_total_space();

    if docker_percentage_remaining < docker_max_disk_percentage_usage_before_purge || available_space == 0 {
        logger.log(
            LogLevel::Warning,
            EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new_from_safe(format!(
                    "Docker disk remaining ({}%) is lower than {}%, requesting cleaning (purge)",
                    docker_percentage_remaining, docker_max_disk_percentage_usage_before_purge
                )),
            ),
        );

        return docker.prune_images();
    };

    logger.log(
        LogLevel::Info,
        EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!(
                "No need to purge old docker images, only {}% ({}/{}) disk used",
                100 - docker_percentage_remaining,
                docker_path_size_info.get_available_space(),
                docker_path_size_info.get_total_space(),
            )),
        ),
    );

    Ok(())
}
