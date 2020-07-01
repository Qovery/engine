use crate::build_platform::error::BuildPlatformError;
use crate::build_platform::{Build, BuildError, BuildPlatform, BuildResult};
use crate::fs::workspace_directory;
use crate::{cmd, git};

/// use Docker in local
pub struct LocalDocker {}

impl LocalDocker {
    pub fn new() -> Self {
        LocalDocker {}
    }
}

impl BuildPlatform for LocalDocker {
    fn is_valid(&self) -> Result<(), BuildPlatformError> {
        // TODO check docker binary
        Ok(())
    }

    fn build(&self, build: Build) -> Result<BuildResult, BuildError> {
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
            let mut build_arg = vec!["--build-arg"];
            build_arg.extend(env_var_args.iter().map(|x| x.as_str()).collect::<Vec<_>>());
            args.extend(build_arg);
            args
        };

        // docker build
        let exit_status = cmd::exec_with_output("docker", final_args, |line| {
            println!("{}", line.unwrap());
        });

        match exit_status {
            Ok(_) => {}
            Err(_) => return Err(BuildError::Error),
        }

        Ok(BuildResult { build })
    }

    fn build_error(&self, build: Build) -> Result<BuildResult, BuildError> {
        unimplemented!()
    }
}
