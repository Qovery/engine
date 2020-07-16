use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::build_platform::error::BuildPlatformError;
use crate::build_platform::{Build, BuildError, BuildPlatform, BuildResult, Kind};
use crate::fs::workspace_directory;
use crate::models::{Listeners, ProgressInfo, ProgressListener};
use crate::{cmd, git};

/// use Docker in local
pub struct LocalDocker {
    id: String,
    name: String,
    listeners: Listeners,
}

impl LocalDocker {
    pub fn new(id: &str, name: &str) -> Self {
        LocalDocker {
            id: id.to_string(),
            name: name.to_string(),
            listeners: vec![],
        }
    }
}

impl BuildPlatform for LocalDocker {
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
        if !crate::cmd::does_command_exists("docker") {
            return Err(BuildPlatformError::Unexpected(
                "docker binary not found".to_string(),
            ));
        }

        Ok(())
    }

    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>) {
        self.listeners.push(listener);
    }

    fn build(&self, build: Build) -> Result<BuildResult, BuildError> {
        info!("LocalDocker.build() called for {}", self.name());

        // git clone
        let into_dir = workspace_directory(format!("build/{}", build.image.name.as_str()));

        let git_clone = git::clone(
            build.git_repository.url.as_str(),
            &into_dir,
            &build.git_repository.credentials,
        );

        match git_clone {
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

        // docker build
        let exit_status = cmd::exec_with_output("docker", final_args, |line| {
            let line_string = line.unwrap();
            let line_str = line_string.as_str();
            info!("{}", line_str);

            self.listeners
                .iter()
                .for_each(|l| l.on_progress(ProgressInfo::new("build", 50, line_str)));
        });

        match exit_status {
            Ok(_) => {}
            Err(_) => return Err(BuildError::Error),
        }

        self.listeners
            .iter()
            .for_each(|l| l.on_complete(ProgressInfo::new("build", 100, "build done")));

        Ok(BuildResult { build })
    }

    fn build_error(&self, build: Build) -> Result<BuildResult, BuildError> {
        warn!("LocalDocker.build_error() called for {}", self.name());

        self.listeners
            .iter()
            .for_each(|l| l.on_error(ProgressInfo::new("build", 100, "something goes wrong")));

        // FIXME
        Err(BuildError::Error)
    }
}
