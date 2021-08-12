use std::path::Path;
use std::{env, fs};

use chrono::Duration;
use sysinfo::{Disk, DiskExt, SystemExt};

use crate::build_platform::{Build, BuildPlatform, BuildResult, Image, Kind};
use crate::error::{EngineError, EngineErrorCause, SimpleError, SimpleErrorKind};
use crate::fs::workspace_directory;
use crate::git::checkout_submodules;
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::{cmd, git};

const BUILD_DURATION_TIMEOUT_MIN: i64 = 30;

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
}

impl LocalDocker {
    pub fn new(context: Context, id: &str, name: &str) -> Self {
        LocalDocker {
            context,
            id: id.to_string(),
            name: name.to_string(),
            listeners: vec![],
        }
    }

    fn image_does_exist(&self, image: &Image) -> Result<bool, EngineError> {
        Ok(matches!(
            crate::cmd::utilities::exec(
                "docker",
                vec!["image", "inspect", image.name_with_tag().as_str()],
                &self.get_docker_host_envs(),
            ),
            Ok(_)
        ))
    }

    fn get_docker_host_envs(&self) -> Vec<(&str, &str)> {
        match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        }
    }

    fn build_image_with_docker(
        &self,
        build: Build,
        dockerfile_complete_path: &str,
        into_dir_docker_style: &str,
        env_var_args: Vec<String>,
        use_build_cache: bool,
        lh: &ListenersHelper,
    ) -> Result<BuildResult, EngineError> {
        let mut docker_args = if !use_build_cache {
            vec!["build", "--no-cache"]
        } else {
            vec!["build"]
        };

        let args = self.context.docker_build_options();
        for v in args.iter() {
            for s in v.iter() {
                docker_args.push(String::as_str(s));
            }
        }

        let name_with_tag = build.image.name_with_tag();

        docker_args.extend(vec!["-f", dockerfile_complete_path, "-t", name_with_tag.as_str()]);

        let mut docker_args = if env_var_args.is_empty() {
            docker_args
        } else {
            let mut build_args = vec![];
            env_var_args.iter().for_each(|x| {
                build_args.push("--build-arg");
                build_args.push(x.as_str());
            });

            docker_args.extend(build_args);
            docker_args
        };

        docker_args.push(into_dir_docker_style);

        // docker build
        let exit_status = cmd::utilities::exec_with_envs_and_output(
            "docker",
            docker_args,
            self.get_docker_host_envs(),
            |line| {
                let line_string = line.unwrap();
                info!("{}", line_string.as_str());

                lh.deployment_in_progress(ProgressInfo::new(
                    ProgressScope::Application {
                        id: build.image.application_id.clone(),
                    },
                    ProgressLevel::Info,
                    Some(line_string.as_str()),
                    self.context.execution_id(),
                ));
            },
            |line| {
                let line_string = line.unwrap();
                error!("{}", line_string.as_str());

                lh.deployment_in_progress(ProgressInfo::new(
                    ProgressScope::Application {
                        id: build.image.application_id.clone(),
                    },
                    ProgressLevel::Warn,
                    Some(line_string.as_str()),
                    self.context.execution_id(),
                ));
            },
            Duration::minutes(BUILD_DURATION_TIMEOUT_MIN),
        );

        match exit_status {
            Ok(_) => Ok(BuildResult { build }),
            Err(err) => Err(self.engine_error(
                EngineErrorCause::User(
                    "It looks like there is something wrong in your Dockerfile. Try building the application locally with `docker build --no-cache`.",
                ),
                format!(
                    "error while building container image {}. Error: {:?}",
                    self.name_with_id(),
                    err
                ),
            )),
        }
    }

    fn build_image_with_buildpacks(
        &self,
        build: Build,
        into_dir_docker_style: &str,
        env_var_args: Vec<String>,
        use_build_cache: bool,
        lh: &ListenersHelper,
    ) -> Result<BuildResult, EngineError> {
        let name_with_tag = build.image.name_with_tag();

        let args = self.context.docker_build_options();

        let mut exit_status: Result<Vec<String>, SimpleError> =
            Err(SimpleError::new(SimpleErrorKind::Other, Some("no builder names")));

        for builder_name in BUILDPACKS_BUILDERS.iter() {
            let mut buildpacks_args = if !use_build_cache {
                vec!["build", name_with_tag.as_str(), "--clear-cache"]
            } else {
                vec!["build", name_with_tag.as_str()]
            };

            for v in args.iter() {
                for s in v.iter() {
                    buildpacks_args.push(String::as_str(s));
                }
            }

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

            // Just a fallback for now to help our bot loving users deploy their apps
            // Long term solution requires lots of changes in UI and Core as well
            // And passing some params to the engine
            if let Ok(content) = fs::read_to_string(into_dir_docker_style.clone().to_owned() + "/Procfile") {
                if content.contains("worker") {
                    buildpacks_args.push("--default-process");
                    buildpacks_args.push("worker");
                }
            }

            // buildpacks build
            exit_status = cmd::utilities::exec_with_envs_and_output(
                "pack",
                buildpacks_args,
                self.get_docker_host_envs(),
                |line| {
                    let line_string = line.unwrap();
                    info!("{}", line_string.as_str());

                    lh.deployment_in_progress(ProgressInfo::new(
                        ProgressScope::Application {
                            id: build.image.application_id.clone(),
                        },
                        ProgressLevel::Info,
                        Some(line_string.as_str()),
                        self.context.execution_id(),
                    ));
                },
                |line| {
                    let line_string = line.unwrap();
                    error!("{}", line_string.as_str());

                    lh.deployment_in_progress(ProgressInfo::new(
                        ProgressScope::Application {
                            id: build.image.application_id.clone(),
                        },
                        ProgressLevel::Warn,
                        Some(line_string.as_str()),
                        self.context.execution_id(),
                    ));
                },
                Duration::minutes(BUILD_DURATION_TIMEOUT_MIN),
            );

            if exit_status.is_ok() {
                // quit now if the builder successfully build the app
                break;
            }
        }

        match exit_status {
            Ok(_) => Ok(BuildResult { build }),
            Err(err) => {
                warn!("{:?}", err);

                Err(self.engine_error(
                    EngineErrorCause::User(
                        "None builders supports Your application can't be built without providing a Dockerfile",
                    ),
                    format!(
                        "Qovery can't build your container image {} with one of the following builders: {}. \
                    Please do provide a valid Dockerfile to build your application or contact the support.",
                        self.name_with_id(),
                        BUILDPACKS_BUILDERS.join(", ")
                    ),
                ))
            }
        }
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
        if !crate::cmd::utilities::does_binary_exist("docker") {
            return Err(self.engine_error(EngineErrorCause::Internal, String::from("docker binary not found")));
        }

        if !crate::cmd::utilities::does_binary_exist("pack") {
            return Err(self.engine_error(EngineErrorCause::Internal, String::from("pack binary not found")));
        }

        Ok(())
    }

    fn build(&self, build: Build, force_build: bool) -> Result<BuildResult, EngineError> {
        info!("LocalDocker.build() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !force_build && self.image_does_exist(&build.image)? {
            info!(
                "image {:?} found on repository, container build is not required",
                build.image
            );

            return Ok(BuildResult { build });
        }

        // git clone
        let repository_root_path = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("build/{}", build.image.name.as_str()),
        );

        info!(
            "cloning repository: {} to {}",
            build.git_repository.url, repository_root_path
        );
        let git_clone = git::clone(
            build.git_repository.url.as_str(),
            &repository_root_path,
            &build.git_repository.credentials,
        );

        if let Err(err) = git_clone {
            let message = format!(
                "Error while cloning repository {}. Error: {:?}",
                &build.git_repository.url, err
            );
            error!("{}", message);
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        // git checkout to given commit
        let repo = git_clone.unwrap();
        let commit_id = &build.git_repository.commit_id;
        if let Err(err) = git::checkout(&repo, commit_id, build.git_repository.url.as_str()) {
            let message = format!(
                "Error while git checkout repository {} with commit id {}. Error: {:?}",
                &build.git_repository.url, commit_id, err
            );
            error!("{}", message);
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        // git checkout submodules
        if let Err(err) = checkout_submodules(&repo) {
            let message = format!(
                "Error while checkout submodules from repository {}. Error: {:?}",
                &build.git_repository.url, err
            );
            error!("{}", message);

            return Err(self.engine_error(EngineErrorCause::Internal, message));
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
            Some(_) => info!("CI environment variable found, no docker prune will be made"),
            None => {
                // ensure there is enough disk space left before building a new image
                let docker_path_string = "/var/lib/docker";
                let docker_path = Path::new(docker_path_string);

                // get system info
                let mut system = sysinfo::System::new_all();
                system.refresh_all();

                for disk in system.get_disks() {
                    if disk.get_mount_point() == docker_path {
                        match check_docker_space_usage_and_clean(disk, self.get_docker_host_envs()) {
                            Ok(msg) => info!("{:?}", msg),
                            Err(e) => error!("{:?}", e.message),
                        }
                        break;
                    };
                }
            }
        }

        let app_id = build.image.application_id.clone();
        let build_context_path = format!("{}/{}/.", repository_root_path.as_str(), build.git_repository.root_path);
        // If no Dockerfile specified, we should use BuildPacks
        let result = if build.git_repository.dockerfile_path.is_some() {
            // build container from the provided Dockerfile

            let dockerfile_relative_path = build.git_repository.dockerfile_path.as_ref().unwrap();
            let dockerfile_normalized_path = match dockerfile_relative_path.trim() {
                "" | "." | "/" | "/." | "./" | "Dockerfile" => "Dockerfile",
                dockerfile_root_path => dockerfile_root_path,
            };

            let dockerfile_relative_path = format!("{}/{}", build.git_repository.root_path, dockerfile_normalized_path);
            let dockerfile_absolute_path = format!("{}/{}", repository_root_path.as_str(), dockerfile_relative_path);

            // If the dockerfile does not exist, abort
            if !Path::new(dockerfile_absolute_path.as_str()).exists() {
                warn!("Dockerfile not found under {}", dockerfile_absolute_path);
                listeners_helper.error(ProgressInfo::new(
                    ProgressScope::Application {
                        id: build.image.application_id.clone(),
                    },
                    ProgressLevel::Error,
                    Some(format!(
                        "Dockerfile is not present at location {}",
                        dockerfile_relative_path
                    )),
                    self.context.execution_id(),
                ));

                return Err(self.engine_error(
                    EngineErrorCause::User("Dockerfile not found at location"),
                    format!(
                        "Your Dockerfile is not present at the specified location {}/{}",
                        build.git_repository.root_path.as_str(),
                        build.git_repository.dockerfile_path.unwrap_or_default().as_str()
                    ),
                ));
            }

            self.build_image_with_docker(
                build,
                dockerfile_absolute_path.as_str(),
                build_context_path.as_str(),
                env_var_args,
                !disable_build_cache,
                &listeners_helper,
            )
        } else {
            // build container with Buildpacks
            self.build_image_with_buildpacks(
                build,
                build_context_path.as_str(),
                env_var_args,
                !disable_build_cache,
                &listeners_helper,
            )
        };

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application { id: app_id },
            ProgressLevel::Info,
            Some(format!("container {} is built ✔", self.name_with_id())),
            self.context.execution_id(),
        ));

        result
    }

    fn build_error(&self, build: Build) -> Result<BuildResult, EngineError> {
        warn!("LocalDocker.build_error() called for {}", self.name());

        let listener_helper = ListenersHelper::new(&self.listeners);

        // FIXME
        let message = String::from("something goes wrong (not implemented)");

        listener_helper.error(ProgressInfo::new(
            ProgressScope::Application {
                id: build.image.application_id,
            },
            ProgressLevel::Error,
            Some(message.as_str()),
            self.context.execution_id(),
        ));

        // FIXME
        Err(self.engine_error(EngineErrorCause::Internal, message))
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

fn check_docker_space_usage_and_clean(
    docker_path_size_info: &Disk,
    envs: Vec<(&str, &str)>,
) -> Result<String, SimpleError> {
    let docker_max_disk_percentage_usage_before_purge = 60; // arbitrary percentage that should make the job anytime
    let available_space = docker_path_size_info.get_available_space();
    let docker_percentage_remaining = available_space * 100 / docker_path_size_info.get_total_space();

    if docker_percentage_remaining < docker_max_disk_percentage_usage_before_purge || available_space == 0 {
        warn!(
            "Docker disk remaining ({}%) is lower than {}%, requesting cleaning (purge)",
            docker_percentage_remaining, docker_max_disk_percentage_usage_before_purge
        );

        return match docker_prune_images(envs) {
            Err(e) => {
                error!("error while purging docker images: {:?}", e.message);
                Err(e)
            }
            _ => Ok("docker images have been purged".to_string()),
        };
    };

    Ok(format!(
        "no need to purge old docker images, only {}% ({}/{}) disk used",
        100 - docker_percentage_remaining,
        docker_path_size_info.get_available_space(),
        docker_path_size_info.get_total_space(),
    ))
}

fn docker_prune_images(envs: Vec<(&str, &str)>) -> Result<(), SimpleError> {
    let all_prunes_commands = vec![
        vec!["container", "prune", "-f"],
        vec!["image", "prune", "-a", "-f"],
        vec!["builder", "prune", "-a", "-f"],
        vec!["volume", "prune", "-f"],
    ];

    for prune in all_prunes_commands {
        match cmd::utilities::exec_with_envs_and_output(
            "docker",
            prune.clone(),
            envs.clone(),
            |line| {
                let line_string = line.unwrap_or_default();
                debug!("{}", line_string.as_str());
            },
            |line| {
                let line_string = line.unwrap_or_default();
                debug!("{}", line_string.as_str());
            },
            Duration::minutes(BUILD_DURATION_TIMEOUT_MIN),
        ) {
            Ok(_) => {}
            Err(e) => error!("error while puring {}. {:?}", prune[0], e.message),
        };
    }

    Ok(())
}
