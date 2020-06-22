use crate::build_platform::error::BuildPlatformError;
use crate::build_platform::{Build, BuildError, BuildPlatform, BuildResult, Image};
use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::container_registry::{ContainerRegistry, PushError, PushResult};
use crate::{cmd, git};
use std::fmt::Error;
use std::path::Path;
use std::process::ExitStatus;
use tempdir::TempDir;

/// use Docker in local
pub struct LocalDocker {}

impl BuildPlatform for LocalDocker {
    fn is_valid(&self) -> Result<(), BuildPlatformError> {
        Ok(())
    }

    fn build(&self, build: Build) -> Result<BuildResult, BuildError> {
        println!("launch build with LocalDocker");

        // git clone
        let tmp_dir = TempDir::new(build.image.name.as_str()).unwrap();
        let into_dir = tmp_dir.path();
        let dockerfile_dir = format!("{}/.", into_dir.to_str().unwrap());

        git::clone(
            build.git_repository.url.as_str(),
            into_dir,
            &build.git_repository.credentials,
        );

        // docker build
        let exit_status = cmd::exec_with_output(
            "docker",
            vec![
                "build",
                "-t",
                build.image.name_with_tag().as_str(),
                dockerfile_dir.as_str(),
            ],
            |line| {
                println!("{}", line.unwrap());
            },
        );

        match exit_status {
            Ok(status) => println!("cmd success: {}", status.success()),
            Err(_) => {}
        }

        Ok(BuildResult { build })
    }
}
