use std::path::Path;
use std::rc::Rc;

use crate::build_platform::error::BuildPlatformError;
use crate::build_platform::{Build, BuildError, BuildPlatform, BuildResult, Image, Kind};
use crate::fs::workspace_directory;
use crate::git::checkout_submodules;
use crate::models::{
    Context, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressListener,
    ProgressScope,
};
use crate::transaction::CommitError::BuildImage;
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

    fn image_does_exist(&self, image: &Image) -> Result<bool, BuildError> {
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

    fn is_valid(&self) -> Result<(), BuildPlatformError> {
        if !crate::cmd::utilities::does_binary_exist("docker") {
            return Err(BuildPlatformError::Unexpected(
                "docker binary not found".to_string(),
            ));
        }

        Ok(())
    }

    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>) {
        self.listeners.push(listener);
    }

    fn build(&self, build: Build, force_build: bool) -> Result<BuildResult, BuildError> {
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
                error! {"Error while trying to clone repository {}", build.git_repository.url}
                return Err(BuildError::Git(err));
            }
        }

        // git checkout to given commit
        let repo = &git_clone.unwrap();
        let commit_id = &build.git_repository.commit_id;
        match git::checkout(&repo, &commit_id, build.git_repository.url.as_str()) {
            Ok(_) => {}
            Err(err) => return Err(BuildError::Git(err)),
        }

        // git checkout submodules
        let _ = checkout_submodules(&repo);
        // TODO what if we can't checkout submodules? Today we ignore it

        let into_dir_docker_style = format!("{}/.", into_dir.as_str());

        let dockerfile_relative_path = match build.git_repository.dockerfile_path.trim() {
            "" | "." | "/" | "/." | "./" | "Dockerfile" => "Dockerfile",
            dockerfile_root_path => dockerfile_root_path,
        };

        let dockerfile_complete_path =
            format!("{}/{}", into_dir.as_str(), dockerfile_relative_path);

        match Path::new(dockerfile_complete_path.as_str()).exists() {
            false => {
                error!(
                    "Unable to find Dockerfile path {}",
                    dockerfile_complete_path.as_str()
                );
                return Err(BuildError::Error);
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
            Err(_) => return Err(BuildError::Error),
        }

        listeners_helper.start_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: build.image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some("build is done ✔"),
            self.context.execution_id(),
        ));

        Ok(BuildResult { build })
    }

    fn build_error(&self, build: Build) -> Result<BuildResult, BuildError> {
        warn!("LocalDocker.build_error() called for {}", self.name());

        let listener_helper = ListenersHelper::new(&self.listeners);
        listener_helper.error(ProgressInfo::new(
            ProgressScope::Application {
                id: build.image.application_id,
            },
            ProgressLevel::Error,
            Some("something goes wrong (not implemented)"),
            self.context.execution_id(),
        ));

        // FIXME
        Err(BuildError::Error)
    }
}
