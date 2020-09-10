use std::rc::Rc;

use crate::build_platform::error::BuildPlatformError;
use crate::build_platform::{Build, BuildError, BuildPlatform, BuildResult, Image, Kind};
use crate::fs::workspace_directory;
use crate::models::{Context, Listeners, ListenersHelper, ProgressInfo, ProgressListener};
use crate::{cmd, git};
use git2::Oid;

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
            match crate::cmd::exec_with_envs(
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
        if !crate::cmd::does_binary_exist("docker") {
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

        let git_clone = git::clone(
            build.git_repository.url.as_str(),
            &into_dir,
            &build.git_repository.credentials,
        );

        match git_clone {
            Ok(_) => {}
            Err(err) => return Err(BuildError::Git(err)),
        }

        // git checkout to given commit
        let repo = &git_clone.unwrap();
        let commit_id = &build.git_repository.commit_id;
        match git::checkout(&repo, &commit_id) {
            Ok(_) => {}
            Err(err) => return Err(BuildError::Git(err)),
        }

        let dockerfile_dir = match build.git_repository.dockerfile_path.trim() {
            "" | "." | "/" | "/." | "./" => format!("{}/.", into_dir.as_str()),
            dockerfile_root_path => format!("{}/{}/.", into_dir.as_str(), dockerfile_root_path),
        };

        // TODO check that the Dockerfile exists

        let env_var_args = &build
            .options
            .environment_variables
            .iter()
            .map(|ev| format!("'{}={}'", ev.key, ev.value))
            .collect::<Vec<_>>();

        let name_with_tag = build.image.name_with_tag();
        let mut args = vec![
            "build",
            "-t",
            name_with_tag.as_str(),
            dockerfile_dir.as_str(),
        ];

        let final_args = if env_var_args.is_empty() {
            args
        } else {
            let mut build_args = vec![];
            env_var_args.iter().for_each(|x| {
                build_args.push("--build-arg");
                build_args.push(x.as_str());
            });

            args.extend(build_args);
            args
        };

        let envs = match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        };

        // docker build
        let exit_status = cmd::exec_with_envs_and_output(
            "docker",
            final_args,
            envs,
            |line| {
                let line_string = line.unwrap();
                info!("{}", line_string.as_str());

                listeners_helper.on_progress(ProgressInfo::new(
                    "build",
                    line_string.as_str(),
                    self.context.execution_id(),
                ));
            },
            |line| {
                let line_string = line.unwrap();
                error!("{}", line_string.as_str());

                listeners_helper.on_error(ProgressInfo::new(
                    "build",
                    line_string.as_str(),
                    self.context.execution_id(),
                ));
            },
        );

        match exit_status {
            Ok(_) => {}
            Err(_) => return Err(BuildError::Error),
        }

        listeners_helper.on_complete(ProgressInfo::new(
            "build",
            "build is done ✅",
            self.context.execution_id(),
        ));

        Ok(BuildResult { build })
    }

    fn build_error(&self, _build: Build) -> Result<BuildResult, BuildError> {
        warn!("LocalDocker.build_error() called for {}", self.name());

        let listener_helper = ListenersHelper::new(&self.listeners);
        listener_helper.on_error(ProgressInfo::new(
            "build",
            "something goes wrong",
            self.context.execution_id(),
        ));

        // FIXME
        Err(BuildError::Error)
    }
}
