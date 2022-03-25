use crate::cmd::command::{CommandError, CommandKiller, QoveryCommand};
use std::path::Path;
use std::process::ExitStatus;
use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum DockerError {
    #[error("Docker Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Docker terminated with an unknown error: {0}")]
    ExecutionError(#[from] std::io::Error),

    #[error("Docker terminated with a non success exit status code: {0}")]
    ExitStatusError(ExitStatus),

    #[error("Docker aborted due to user cancel request: {0}")]
    Aborted(String),

    #[error("Docker command terminated due to timeout: {0}")]
    Timeout(String),
}

#[derive(Debug)]
pub struct ContainerImage {
    pub registry: Url,
    pub name: String,
    pub tags: Vec<String>,
}

impl ContainerImage {
    pub fn image_names(&self) -> Vec<String> {
        let host = if let Some(port) = self.registry.port() {
            format!("{}:{}", self.registry.host_str().unwrap_or_default(), port)
        } else {
            self.registry.host_str().unwrap_or_default().to_string()
        };

        self.tags
            .iter()
            .map(|tag| format!("{}/{}:{}", host, &self.name, tag))
            .collect()
    }

    pub fn image_name(&self) -> String {
        self.image_names().remove(0)
    }
}

#[derive(Debug, Clone)]
pub struct Docker {
    use_buildkit: bool,
    common_envs: Vec<(String, String)>,
}

impl Docker {
    pub fn new_with_options(enable_buildkit: bool, socket_location: Option<Url>) -> Result<Self, DockerError> {
        let mut docker = Docker {
            use_buildkit: enable_buildkit,
            common_envs: vec![(
                "DOCKER_BUILDKIT".to_string(),
                if enable_buildkit {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            )],
        };

        // Override DOCKER_HOST if we use a TCP socket
        if let Some(socket_location) = socket_location {
            docker
                .common_envs
                .push(("DOCKER_HOST".to_string(), socket_location.to_string()))
        }

        // If we don't use buildkit nothing more to do
        if !docker.use_buildkit {
            return Ok(docker);
        }

        // First check that the buildx plugin is correctly installed
        let args = vec!["buildx", "version"];
        let buildx_cmd_exist = docker_exec(
            &args,
            &docker.get_all_envs(&[]),
            &mut |_| {},
            &mut |_| {},
            &CommandKiller::never(),
        );
        if buildx_cmd_exist.is_err() {
            return Err(DockerError::InvalidConfig(
                "Docker buildx plugin for buildkit is not correctly installed".to_string(),
            ));
        }

        // In order to be able to use --cache-from --cache-to for buildkit,
        // we need to create our specific builder, which is not the default one (aka: the docker one)
        let args = vec![
            "buildx",
            "create",
            "--name",
            "qovery-engine",
            "--driver-opt",
            "network=host",
            "--bootstrap",
            "--use",
        ];
        let _ = docker_exec(
            &args,
            &docker.get_all_envs(&[]),
            &mut |_| {},
            &mut |_| {},
            &CommandKiller::never(),
        );

        Ok(docker)
    }

    pub fn new(socket_location: Option<Url>) -> Result<Self, DockerError> {
        Self::new_with_options(true, socket_location)
    }

    fn get_all_envs<'a>(&'a self, envs: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all_envs: Vec<(&str, &str)> = self.common_envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all_envs.append(&mut envs.to_vec());

        all_envs
    }

    pub fn login(&self, registry: &Url) -> Result<(), DockerError> {
        info!("Docker login {} as user {}", registry, registry.username());
        let password = urlencoding::decode(registry.password().unwrap_or_default())
            .unwrap_or_default()
            .to_string();
        let args = vec![
            "login",
            registry.host_str().unwrap_or_default(),
            "-u",
            registry.username(),
            "-p",
            &password,
        ];

        docker_exec(
            &args,
            &self.get_all_envs(&[]),
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::never(),
        )?;

        Ok(())
    }

    pub fn does_image_exist_locally(&self, image: &ContainerImage) -> Result<bool, DockerError> {
        info!("Docker check locally image exist {:?}", image);

        let ret = docker_exec(
            &["image", "inspect", &image.image_name()],
            &self.get_all_envs(&[]),
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::never(),
        );

        Ok(matches!(ret, Ok(_)))
    }

    // Warning: this command is slow > 10 sec
    pub fn does_image_exist_remotely(&self, image: &ContainerImage) -> Result<bool, DockerError> {
        info!("Docker check remotely image exist {:?}", image);

        let ret = docker_exec(
            &["manifest", "inspect", &image.image_name()],
            &self.get_all_envs(&[]),
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::never(),
        );

        match ret {
            Ok(_) => Ok(true),
            Err(DockerError::ExitStatusError(_)) => Ok(false),
            Err(err) => Err(err),
        }
    }

    pub fn pull<Stdout, Stderr>(
        &self,
        image: &ContainerImage,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker pull {:?}", image);

        docker_exec(
            &["pull", &image.image_name()],
            &self.get_all_envs(&[]),
            stdout_output,
            stderr_output,
            should_abort,
        )
    }

    pub fn build<Stdout, Stderr>(
        &self,
        dockerfile: &Path,
        context: &Path,
        image_to_build: &ContainerImage,
        build_args: &[(&str, &str)],
        cache: &ContainerImage,
        push_after_build: bool,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        // if there is no tags, nothing to build
        if image_to_build.tags.is_empty() {
            return Ok(());
        }

        // Do some checks
        if !dockerfile.is_file() {
            return Err(DockerError::InvalidConfig(format!(
                "provided dockerfile `{:?}` is not a valid file",
                dockerfile
            )));
        }

        if !context.is_dir() {
            return Err(DockerError::InvalidConfig(format!(
                "provided docker build context `{:?}` is not a valid directory",
                context
            )));
        }

        if self.use_buildkit {
            self.build_with_buildkit(
                dockerfile,
                context,
                image_to_build,
                build_args,
                cache,
                push_after_build,
                stdout_output,
                stderr_output,
                should_abort,
            )
        } else {
            self.build_with_docker(
                dockerfile,
                context,
                image_to_build,
                build_args,
                cache,
                push_after_build,
                stdout_output,
                stderr_output,
                should_abort,
            )
        }
    }

    fn build_with_docker<Stdout, Stderr>(
        &self,
        dockerfile: &Path,
        context: &Path,
        image_to_build: &ContainerImage,
        build_args: &[(&str, &str)],
        cache: &ContainerImage,
        push_after_build: bool,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker build {:?}", image_to_build.image_name());

        // Best effort to pull the cache, if it does not exist that's ok too
        let _ = self.pull(cache, stdout_output, stderr_output, should_abort);

        let mut args_string: Vec<String> = vec![
            "build".to_string(),
            "--network".to_string(),
            "host".to_string(),
            "-f".to_string(),
            dockerfile.to_str().unwrap_or_default().to_string(),
        ];

        for image_name in image_to_build.image_names() {
            args_string.push("--tag".to_string());
            args_string.push(image_name)
        }

        for img_cache_name in cache.image_names() {
            args_string.push("--tag".to_string());
            args_string.push(img_cache_name)
        }

        for (k, v) in build_args {
            args_string.push("--build-arg".to_string());
            args_string.push(format!("{}={}", k, v));
        }

        args_string.push(context.to_str().unwrap_or_default().to_string());

        let _ = docker_exec(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(&[]),
            stdout_output,
            stderr_output,
            should_abort,
        )?;

        if push_after_build {
            let _ = self.push(image_to_build, stdout_output, stderr_output, should_abort)?;
        }

        Ok(())
    }

    fn build_with_buildkit<Stdout, Stderr>(
        &self,
        dockerfile: &Path,
        context: &Path,
        image_to_build: &ContainerImage,
        build_args: &[(&str, &str)],
        cache: &ContainerImage,
        push_after_build: bool,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker buildkit build {:?}", image_to_build.image_name());

        let mut args_string: Vec<String> = vec![
            "buildx".to_string(),
            "build".to_string(),
            "--progress=plain".to_string(),
            "--network=host".to_string(),
            if push_after_build {
                "--output=type=registry".to_string() // tell buildkit to push image to registry
            } else {
                "--output=type=docker".to_string() // tell buildkit to load the image into docker after build
            },
            "--cache-from".to_string(),
            format!("type=registry,ref={}", cache.image_name()),
            // Disabled for now, because private ECR does not support it ...
            // https://github.com/aws/containers-roadmap/issues/876
            // "--cache-to".to_string(),
            // format!("type=registry,ref={}", cache.image_name()),
            "-f".to_string(),
            dockerfile.to_str().unwrap_or_default().to_string(),
        ];

        for image_name in image_to_build.image_names() {
            args_string.push("--tag".to_string());
            args_string.push(image_name.to_string())
        }

        for (k, v) in build_args {
            args_string.push("--build-arg".to_string());
            args_string.push(format!("{}={}", k, v));
        }

        args_string.push(context.to_str().unwrap_or_default().to_string());

        docker_exec(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(&[]),
            stdout_output,
            stderr_output,
            should_abort,
        )
    }

    pub fn push<Stdout, Stderr>(
        &self,
        image: &ContainerImage,
        stdout_output: &mut Stdout,
        stderr_output: &mut Stderr,
        should_abort: &CommandKiller,
    ) -> Result<(), DockerError>
    where
        Stdout: FnMut(String),
        Stderr: FnMut(String),
    {
        info!("Docker push {:?}", image);
        let image_names = image.image_names();
        let mut args = vec!["push"];
        args.extend(image_names.iter().map(|x| x.as_str()));

        docker_exec(&args, &self.get_all_envs(&[]), stdout_output, stderr_output, should_abort)
    }

    pub fn prune_images(&self) -> Result<(), DockerError> {
        info!("Docker prune images");

        let all_prunes_commands = vec![
            vec!["container", "prune", "-f"],
            vec!["image", "prune", "-a", "-f"],
            vec!["builder", "prune", "-a", "-f"],
            vec!["volume", "prune", "-f"],
            vec!["buildx", "prune", "-a", "-f"],
        ];

        let mut errored_commands = vec![];
        for prune in all_prunes_commands {
            let ret = docker_exec(
                &prune,
                &self.get_all_envs(&[]),
                &mut |_| {},
                &mut |_| {},
                &CommandKiller::never(),
            );
            if let Err(e) = ret {
                errored_commands.push(e);
            }
        }

        if !errored_commands.is_empty() {
            return Err(errored_commands.remove(0));
        }

        Ok(())
    }
}

fn docker_exec<F, X>(
    args: &[&str],
    envs: &[(&str, &str)],
    stdout_output: &mut F,
    stderr_output: &mut X,
    cmd_killer: &CommandKiller,
) -> Result<(), DockerError>
where
    F: FnMut(String),
    X: FnMut(String),
{
    let mut cmd = QoveryCommand::new("docker", args, envs);
    let ret = cmd.exec_with_abort(stdout_output, stderr_output, cmd_killer);

    match ret {
        Ok(_) => Ok(()),
        Err(CommandError::TimeoutError(msg)) => Err(DockerError::Timeout(msg)),
        Err(CommandError::Killed(msg)) => Err(DockerError::Aborted(msg)),
        Err(CommandError::ExitStatusError(err)) => Err(DockerError::ExitStatusError(err)),
        Err(CommandError::ExecutionError(err)) => Err(DockerError::ExecutionError(err)),
    }
}

// start a local registry to run this test
// docker run --rm -ti -p 5000:5000 --name registry registry:2
#[cfg(feature = "test-with-docker")]
#[cfg(test)]
mod tests {
    use crate::cmd::command::CommandKiller;
    use crate::cmd::docker::{ContainerImage, Docker, DockerError};
    use std::path::Path;
    use std::time::Duration;
    use url::Url;

    fn private_registry_url() -> Url {
        Url::parse("http://localhost:5000").unwrap()
    }

    #[test]
    fn test_pull() {
        let docker = Docker::new(None).unwrap();

        // Invalid image should fails
        let image = ContainerImage {
            registry: Url::parse("https://docker.io").unwrap(),
            name: "alpine".to_string(),
            tags: vec!["666".to_string()],
        };
        let ret = docker.pull(
            &image,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Err(_)));

        // Valid image should be ok
        let image = ContainerImage {
            registry: Url::parse("https://docker.io").unwrap(),
            name: "alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };

        let ret = docker.pull(
            &image,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));

        // Should timeout
        let ret = docker.pull(
            &image,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::from_timeout(Duration::from_secs(1)),
        );
        assert!(matches!(ret, Err(DockerError::Timeout(_))));
    }

    #[test]
    fn test_docker_build() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_options(false, None).unwrap();
        let image_to_build = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };
        let image_cache = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["cache".to_string()],
        };

        let ret = docker.build_with_docker(
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );

        assert!(matches!(ret, Ok(_)));

        // It should fails with buildkit dockerfile
        let ret = docker.build_with_docker(
            Path::new("tests/docker/multi_stage_simple/Dockerfile.buildkit"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );

        assert!(matches!(ret, Err(_)));
    }

    #[test]
    fn test_buildkit_build() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_options(true, None).unwrap();
        let image_to_build = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };
        let image_cache = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["cache".to_string()],
        };

        // It should work
        let ret = docker.build_with_buildkit(
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );

        assert!(matches!(ret, Ok(_)));

        let ret = docker.build_with_buildkit(
            Path::new("tests/docker/multi_stage_simple/Dockerfile.buildkit"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );

        assert!(matches!(ret, Ok(_)));
    }

    #[test]
    fn test_push() {
        // start a local registry to run this test
        // docker run --rm -d -p 5000:5000 --name registry registry:2
        let docker = Docker::new_with_options(true, None).unwrap();
        let image_to_build = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["3.15".to_string()],
        };
        let image_cache = ContainerImage {
            registry: private_registry_url(),
            name: "erebe/alpine".to_string(),
            tags: vec!["cache".to_string()],
        };

        // It should work
        let ret = docker.build_with_buildkit(
            Path::new("tests/docker/multi_stage_simple/Dockerfile"),
            Path::new("tests/docker/multi_stage_simple/"),
            &image_to_build,
            &[],
            &image_cache,
            false,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));

        let ret = docker.does_image_exist_locally(&image_to_build);
        assert!(matches!(ret, Ok(true)));

        let ret = docker.does_image_exist_remotely(&image_to_build);
        assert!(matches!(ret, Ok(false)));

        let ret = docker.push(
            &image_to_build,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));

        let ret = docker.pull(
            &image_to_build,
            &mut |msg| println!("{}", msg),
            &mut |msg| eprintln!("{}", msg),
            &CommandKiller::never(),
        );
        assert!(matches!(ret, Ok(_)));
    }
}
