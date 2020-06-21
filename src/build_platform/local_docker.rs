use crate::build_platform::registry::{PushError, PushResult, Registry};
use crate::build_platform::{Build, BuildError, BuildPlatform, BuildResult, Image};
use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::{cmd, git};
use std::fmt::Error;
use std::path::Path;
use std::process::ExitStatus;
use tempdir::TempDir;

/// use Docker in local
pub struct LocalDocker<'a> {
    pub registry: Box<dyn Registry<'a>>,
}

impl<'a> BuildPlatform<'a> for LocalDocker<'a> {
    fn is_valid(&self) -> bool {
        true
    }

    fn registry(self) -> Box<dyn Registry<'a>> {
        self.registry
    }

    fn build(&self, build: Build) -> Result<BuildResult, BuildError> {
        println!("launch build with LocalDocker");

        // git clone
        let tmp_dir = TempDir::new(build.image.name.as_str()).unwrap();
        let into_dir = tmp_dir.path();
        let dockerfile_dir = format!("{}/.", into_dir.to_str().unwrap());

        println!("{}", dockerfile_dir.clone());

        git::clone(
            build.git_repository.url.as_str(),
            into_dir,
            &build.git_repository.credentials,
        );

        // docker build
        let exit_status = cmd::exec("docker", vec!["build", dockerfile_dir.as_str()], |line| {
            println!("{}", line.unwrap());
        });

        match exit_status {
            Ok(status) => println!("cmd success: {}", status.success()),
            Err(_) => {}
        }

        Ok(BuildResult { build })
    }

    fn push(&self, image: Image) -> Result<PushResult<'a>, PushError> {
        unimplemented!()
    }
}
