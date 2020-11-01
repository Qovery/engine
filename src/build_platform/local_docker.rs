use std::path::Path;
use std::rc::Rc;

use git2::Error;

use crate::build_platform::{Build, BuildPlatform, BuildResult, Image, Kind};
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::fs::workspace_directory;
use crate::git::checkout_submodules;
use crate::models::{
    Context, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressListener,
    ProgressScope,
};
use crate::{cmd, git};

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
        let envs = match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        };

        Ok(
            match crate::cmd::utilities::exec_with_envs(
                "docker",
                vec!["image", "inspect", image.name_with_tag().as_str()],
                envs,
            ) {
                Ok(_) => true,
                _ => false,
            },
        )
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
            return Err(self.engine_error(
                EngineErrorCause::Internal,
                String::from("docker binary not found"),
            ));
        }

        Ok(())
    }

    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>) {
        self.listeners.push(listener);
    }

    fn build(&self, build: Build, force_build: bool) -> Result<BuildResult, EngineError> {
        info!("LocalDocker.build() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !force_build && self.image_does_exist(&build.image)? {
            info!(
                "image {:?} does already exist - no need to build it",
                build.image
            );

            return Ok(BuildResult { build });
        }

        // git clone
        let into_dir = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("build/{}", build.image.name.as_str()),
        );

        info!("cloning repository: {}", build.git_repository.url);
        let git_clone = git::clone(
            build.git_repository.url.as_str(),
            &into_dir,
            &build.git_repository.credentials,
        );

        match git_clone {
            Ok(_) => {}
            Err(err) => {
                let message = format!(
                    "Error while cloning repository {}. Error: {:?}",
                    &build.git_repository.url, err
                );

                error!("{}", message);

                return Err(self.engine_error(EngineErrorCause::Internal, message));
            }
        }

        // git checkout to given commit
        let repo = &git_clone.unwrap();
        let commit_id = &build.git_repository.commit_id;
        match git::checkout(&repo, &commit_id, build.git_repository.url.as_str()) {
            Ok(_) => {}
            Err(err) => {
                let message = format!(
                    "Error while git checkout repository {} with commit id {}. Error: {:?}",
                    &build.git_repository.url, commit_id, err
                );

                error!("{}", message);

                return Err(self.engine_error(EngineErrorCause::Internal, message));
            }
        }

        // git checkout submodules
        match checkout_submodules(repo) {
            Ok(_) => {}
            Err(err) => {
                let message = format!(
                    "Error while checkout submodules from repository {}. Error: {:?}",
                    &build.git_repository.url, err
                );

                error!("{}", message);

                return Err(self.engine_error(EngineErrorCause::Internal, message));
            }
        }

        let into_dir_docker_style = format!("{}/.", into_dir.as_str());

        let dockerfile_relative_path = match build.git_repository.dockerfile_path.trim() {
            "" | "." | "/" | "/." | "./" | "Dockerfile" => "Dockerfile",
            dockerfile_root_path => dockerfile_root_path,
        };

        let dockerfile_complete_path =
            format!("{}/{}", into_dir.as_str(), dockerfile_relative_path);

        match Path::new(dockerfile_complete_path.as_str()).exists() {
            false => {
                let message = format!(
                    "Unable to find Dockerfile path {}",
                    dockerfile_complete_path.as_str()
                );

                error!("{}", &message);

                return Err(self.engine_error(EngineErrorCause::Internal, message));
            }
            _ => {}
        }

        let env_var_args = &build
            .options
            .environment_variables
            .iter()
            .map(|ev| format!("'{}={}'", ev.key, ev.value))
            .collect::<Vec<_>>();

        let name_with_tag = build.image.name_with_tag();
        let mut docker_args = vec![
            "build",
            "-f",
            dockerfile_complete_path.as_str(),
            "-t",
            name_with_tag.as_str(),
        ];

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

        docker_args.push(into_dir_docker_style.as_str());

        let envs = match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        };

        // docker build
        let exit_status = cmd::utilities::exec_with_envs_and_output(
            "docker",
            docker_args,
            envs,
            |line| {
                let line_string = line.unwrap();
                info!("{}", line_string.as_str());

                listeners_helper.start_in_progress(ProgressInfo::new(
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

                listeners_helper.error(ProgressInfo::new(
                    ProgressScope::Application {
                        id: build.image.application_id.clone(),
                    },
                    ProgressLevel::Error,
                    Some(line_string.as_str()),
                    self.context.execution_id(),
                ));
            },
        );

        match exit_status {
            Ok(_) => {}
            Err(err) => {
                return Err(self.engine_error(
                    EngineErrorCause::User(
                        "It looks like your Dockerfile is wrong. Did you consider building \
                        your container locally using `qovery run` or `docker build`?",
                    ),
                    format!(
                        "error while building container image {}. Error: {:?}",
                        self.name_with_id(),
                        err
                    ),
                ));
            }
        }

        listeners_helper.start_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: build.image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(format!(
                "container build is done for {} ✔",
                self.name_with_id()
            )),
            self.context.execution_id(),
        ));

        Ok(BuildResult { build })
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
