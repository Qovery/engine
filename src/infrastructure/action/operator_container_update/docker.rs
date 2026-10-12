use super::config::ContainerSpec;
use super::model::{
    ContainerId, ContainerInspect, ContainerName, ImageInspect, ImageLookup, ImageReference, Platform, RestartPolicy,
    ServerVersion,
};
use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use regex::escape;
use serde::de::DeserializeOwned;
use std::fmt::{Display, Formatter};
use std::io::{Error as IoError, Write};
use std::time::Duration;
use tempfile::NamedTempFile;
use url::Url;

const DOCKER_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const DOCKER_PULL_TIMEOUT: Duration = Duration::from_secs(600);
const DOCKER_STOP_GRACE_PERIOD_SECONDS: &str = "30";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DockerOperation {
    Version,
    List,
    Inspect,
    Pull,
    Create,
    UpdateRestartPolicy,
    Stop,
    Rename,
    Start,
    Remove,
}

impl Display for DockerOperation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DockerOperation::Version => "version",
            DockerOperation::List => "ps",
            DockerOperation::Inspect => "inspect",
            DockerOperation::Pull => "pull",
            DockerOperation::Create => "create",
            DockerOperation::UpdateRestartPolicy => "update --restart",
            DockerOperation::Stop => "stop",
            DockerOperation::Rename => "rename",
            DockerOperation::Start => "start",
            DockerOperation::Remove => "rm",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DockerFailure {
    pub operation: DockerOperation,
    pub message: String,
}

impl Display for DockerFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "docker {} failed: {}", self.operation, self.message)
    }
}

/// The Docker daemon operations an Operator container update needs.
pub trait OperatorDocker {
    fn host_platform(&self) -> Result<Platform, DockerFailure>;
    fn list_running_containers(&self) -> Result<Vec<ContainerInspect>, DockerFailure>;
    fn find_containers_named(&self, names: &[ContainerName]) -> Result<Vec<ContainerInspect>, DockerFailure>;
    fn inspect_container(&self, id: &ContainerId) -> Result<ContainerInspect, DockerFailure>;
    fn container_exists(&self, id: &ContainerId) -> Result<bool, DockerFailure>;
    fn pull_image(&self, image: &ImageReference) -> Result<(), DockerFailure>;
    fn inspect_image(&self, image: ImageLookup<'_>) -> Result<ImageInspect, DockerFailure>;
    fn create_container(&self, spec: &ContainerSpec) -> Result<ContainerId, DockerFailure>;
    fn update_restart_policy(&self, id: &ContainerId, policy: RestartPolicy) -> Result<(), DockerFailure>;
    fn stop_container(&self, id: &ContainerId) -> Result<(), DockerFailure>;
    fn rename_container(&self, id: &ContainerId, name: &ContainerName) -> Result<(), DockerFailure>;
    fn start_container(&self, id: &ContainerId) -> Result<(), DockerFailure>;
    fn remove_container(&self, id: &ContainerId) -> Result<(), DockerFailure>;
}

pub struct DockerCli {
    docker_host: Option<String>,
}

impl DockerCli {
    pub fn new(socket_location: Option<&Url>) -> Self {
        DockerCli {
            docker_host: socket_location.map(Url::to_string),
        }
    }

    fn run(&self, operation: DockerOperation, args: &[&str], timeout: Duration) -> Result<String, DockerFailure> {
        let envs: Vec<(&str, &str)> = self
            .docker_host
            .as_deref()
            .map(|host| vec![("DOCKER_HOST", host)])
            .unwrap_or_default();
        let mut command = QoveryCommand::new("docker", args, &envs);
        command.set_kill_grace_period(Duration::from_secs(0));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let result = command.exec_with_abort(
            &mut |line| stdout.push(line),
            &mut |line| stderr.push(line),
            &CommandKiller::from_timeout(timeout),
        );
        match result {
            Ok(()) => Ok(stdout.join("\n")),
            Err(err) => Err(DockerFailure {
                operation,
                message: if stderr.is_empty() {
                    err.to_string()
                } else {
                    stderr.join(" ")
                },
            }),
        }
    }

    fn parse<T: DeserializeOwned>(operation: DockerOperation, output: &str) -> Result<T, DockerFailure> {
        serde_json::from_str(output).map_err(|err| DockerFailure {
            operation,
            message: format!("cannot parse the Docker output: {err}"),
        })
    }

    fn list_and_inspect(&self, filters: &[&str]) -> Result<Vec<ContainerInspect>, DockerFailure> {
        let mut args = vec!["container", "ls", "--quiet", "--no-trunc"];
        args.extend_from_slice(filters);
        let output = self.run(DockerOperation::List, &args, DOCKER_COMMAND_TIMEOUT)?;
        let ids: Vec<&str> = output.lines().map(str::trim).filter(|id| !id.is_empty()).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec!["container", "inspect"];
        args.extend(ids);
        let output = self.run(DockerOperation::Inspect, &args, DOCKER_COMMAND_TIMEOUT)?;
        Self::parse(DockerOperation::Inspect, &output)
    }
}

impl OperatorDocker for DockerCli {
    fn host_platform(&self) -> Result<Platform, DockerFailure> {
        let output = self.run(
            DockerOperation::Version,
            &["version", "--format", "{{json .Server}}"],
            DOCKER_COMMAND_TIMEOUT,
        )?;
        let server: ServerVersion = Self::parse(DockerOperation::Version, &output)?;
        Platform::parse(&server.os, &server.arch).map_err(|err| DockerFailure {
            operation: DockerOperation::Version,
            message: err.to_string(),
        })
    }

    fn list_running_containers(&self) -> Result<Vec<ContainerInspect>, DockerFailure> {
        self.list_and_inspect(&[])
    }

    fn find_containers_named(&self, names: &[ContainerName]) -> Result<Vec<ContainerInspect>, DockerFailure> {
        let filter = exact_name_filter(names);
        let containers = self.list_and_inspect(&["--all", "--filter", &filter])?;
        Ok(containers
            .into_iter()
            .filter(|container| names.contains(&container.name))
            .collect())
    }

    fn inspect_container(&self, id: &ContainerId) -> Result<ContainerInspect, DockerFailure> {
        let output = self.run(
            DockerOperation::Inspect,
            &["container", "inspect", id.as_str()],
            DOCKER_COMMAND_TIMEOUT,
        )?;
        let mut containers: Vec<ContainerInspect> = Self::parse(DockerOperation::Inspect, &output)?;
        containers.pop().ok_or_else(|| DockerFailure {
            operation: DockerOperation::Inspect,
            message: format!("container {id} not found"),
        })
    }

    fn container_exists(&self, id: &ContainerId) -> Result<bool, DockerFailure> {
        let filter = format!("id={id}");
        let output = self.run(
            DockerOperation::List,
            &["container", "ls", "--all", "--quiet", "--no-trunc", "--filter", &filter],
            DOCKER_COMMAND_TIMEOUT,
        )?;
        Ok(output.lines().any(|line| line.trim() == id.as_str()))
    }

    fn pull_image(&self, image: &ImageReference) -> Result<(), DockerFailure> {
        self.run(
            DockerOperation::Pull,
            &["image", "pull", "--quiet", image.as_str()],
            DOCKER_PULL_TIMEOUT,
        )
        .map(|_| ())
    }

    fn inspect_image(&self, image: ImageLookup<'_>) -> Result<ImageInspect, DockerFailure> {
        let output = self.run(
            DockerOperation::Inspect,
            &["image", "inspect", "--", image.as_str()],
            DOCKER_COMMAND_TIMEOUT,
        )?;
        let mut images: Vec<ImageInspect> = Self::parse(DockerOperation::Inspect, &output)?;
        images.pop().ok_or_else(|| DockerFailure {
            operation: DockerOperation::Inspect,
            message: format!("image {} not found", image.as_str()),
        })
    }

    fn create_container(&self, spec: &ContainerSpec) -> Result<ContainerId, DockerFailure> {
        let file_failure = |err: IoError| DockerFailure {
            operation: DockerOperation::Create,
            message: format!("cannot write a temporary file: {err}"),
        };
        let env_file = write_key_value_file(
            spec.env
                .iter()
                .map(|variable| (variable.name.as_str(), variable.value.as_str())),
        )
        .map_err(file_failure)?;
        let label_file = write_key_value_file(spec.labels.iter().map(|(key, value)| (key.as_str(), value.as_str())))
            .map_err(file_failure)?;
        let env_file_path = env_file.path().to_string_lossy();
        let label_file_path = label_file.path().to_string_lossy();

        let arguments = create_arguments(spec, &env_file_path, &label_file_path);
        let args: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let output = self.run(DockerOperation::Create, &args, DOCKER_COMMAND_TIMEOUT)?;
        match output.lines().last().map(str::trim) {
            Some(id) if !id.is_empty() => Ok(ContainerId::from(id)),
            _ => Err(DockerFailure {
                operation: DockerOperation::Create,
                message: "docker create returned no container id".to_string(),
            }),
        }
    }

    fn update_restart_policy(&self, id: &ContainerId, policy: RestartPolicy) -> Result<(), DockerFailure> {
        let policy = policy.to_string();
        self.run(
            DockerOperation::UpdateRestartPolicy,
            &["container", "update", "--restart", &policy, id.as_str()],
            DOCKER_COMMAND_TIMEOUT,
        )
        .map(|_| ())
    }

    fn stop_container(&self, id: &ContainerId) -> Result<(), DockerFailure> {
        self.run(
            DockerOperation::Stop,
            &[
                "container",
                "stop",
                "--time",
                DOCKER_STOP_GRACE_PERIOD_SECONDS,
                id.as_str(),
            ],
            DOCKER_COMMAND_TIMEOUT,
        )
        .map(|_| ())
    }

    fn rename_container(&self, id: &ContainerId, name: &ContainerName) -> Result<(), DockerFailure> {
        self.run(
            DockerOperation::Rename,
            &["container", "rename", id.as_str(), name.as_str()],
            DOCKER_COMMAND_TIMEOUT,
        )
        .map(|_| ())
    }

    fn start_container(&self, id: &ContainerId) -> Result<(), DockerFailure> {
        self.run(
            DockerOperation::Start,
            &["container", "start", id.as_str()],
            DOCKER_COMMAND_TIMEOUT,
        )
        .map(|_| ())
    }

    fn remove_container(&self, id: &ContainerId) -> Result<(), DockerFailure> {
        self.run(
            DockerOperation::Remove,
            &["container", "rm", "--force", id.as_str()],
            DOCKER_COMMAND_TIMEOUT,
        )
        .map(|_| ())
    }
}

fn exact_name_filter(names: &[ContainerName]) -> String {
    let alternatives: Vec<String> = names.iter().map(|name| escape(name.as_str())).collect();
    format!("name=^/(?:{})$", alternatives.join("|"))
}

fn write_key_value_file<'a>(entries: impl Iterator<Item = (&'a str, &'a str)>) -> Result<NamedTempFile, IoError> {
    let mut file = NamedTempFile::new()?;
    for (key, value) in entries {
        writeln!(file, "{key}={value}")?;
    }
    file.flush()?;
    Ok(file)
}

fn create_arguments(spec: &ContainerSpec, env_file: &str, label_file: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "container",
        "create",
        "--name",
        spec.name.as_str(),
        "--env-file",
        env_file,
        "--label-file",
        label_file,
        "--restart",
    ]
    .map(str::to_string)
    .to_vec();
    args.push(spec.restart_policy.to_string());
    for bind in &spec.binds {
        args.extend(["--volume".to_string(), bind.clone()]);
    }
    for group in &spec.group_add {
        args.extend(["--group-add".to_string(), group.clone()]);
    }
    if let Some(network) = spec.network_mode.cli_value() {
        args.extend(["--network".to_string(), network]);
    }
    if let Some(driver) = &spec.log_config.driver {
        args.extend(["--log-driver".to_string(), driver.as_str().to_string()]);
    }
    for (key, value) in &spec.log_config.config {
        args.extend(["--log-opt".to_string(), format!("{key}={value}")]);
    }
    let resources = &spec.resources;
    let mut push_non_zero = |flag: &str, value: i64| {
        if value != 0 {
            args.extend([flag.to_string(), value.to_string()]);
        }
    };
    push_non_zero("--memory", resources.memory);
    push_non_zero("--memory-reservation", resources.memory_reservation);
    push_non_zero("--memory-swap", resources.memory_swap);
    push_non_zero("--cpu-shares", resources.cpu_shares);
    push_non_zero("--pids-limit", resources.pids_limit.unwrap_or_default());
    if resources.nano_cpus != 0 {
        args.extend([
            "--cpus".to_string(),
            format!(
                "{}.{:09}",
                resources.nano_cpus / 1_000_000_000,
                resources.nano_cpus % 1_000_000_000
            ),
        ]);
    }
    if !resources.cpuset_cpus.is_empty() {
        args.extend(["--cpuset-cpus".to_string(), resources.cpuset_cpus.clone()]);
    }
    args.extend(["--".to_string(), spec.image.as_str().to_string()]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::action::operator_container_update::model::{
        EnvVar, LogConfig, LogDriver, NetworkMode, PinnedImage, Resources, RestartPolicyName,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::slice;

    #[test]
    fn create_arguments_reproduce_the_host_config_and_keep_secrets_out_of_the_command_line() {
        let digest = format!("localhost:5000/operator@sha256:{}", "a".repeat(64));
        let image = ImageReference::parse(&digest).unwrap();
        let spec = ContainerSpec {
            name: ContainerName::from("qovery-operator-next"),
            image: PinnedImage::resolve(&image, slice::from_ref(&digest)).unwrap(),
            env: vec![EnvVar {
                name: "CLUSTER_JWT_TOKEN".to_string(),
                value: "secret".to_string(),
            }],
            labels: BTreeMap::from([("qovery.com/operator-update".to_string(), "exec".to_string())]),
            binds: vec!["/var/run/docker.sock:/var/run/docker.sock".to_string()],
            group_add: vec!["998".to_string()],
            restart_policy: RestartPolicy {
                name: RestartPolicyName::OnFailure,
                maximum_retry_count: 3,
            },
            network_mode: NetworkMode::UserDefined("qovery-bastion".to_string()),
            log_config: LogConfig {
                driver: Some(LogDriver::from("local")),
                config: BTreeMap::from([("max-size".to_string(), "10m".to_string())]),
            },
            resources: Resources {
                memory: 268_435_456,
                memory_reservation: 134_217_728,
                memory_swap: 536_870_912,
                nano_cpus: 1_500_000_000,
                cpu_shares: 512,
                cpuset_cpus: "0-1".to_string(),
                pids_limit: Some(256),
            },
        };

        let args = create_arguments(&spec, "/tmp/env", "/tmp/labels");

        assert_eq!(
            args,
            [
                "container",
                "create",
                "--name",
                "qovery-operator-next",
                "--env-file",
                "/tmp/env",
                "--label-file",
                "/tmp/labels",
                "--restart",
                "on-failure:3",
                "--volume",
                "/var/run/docker.sock:/var/run/docker.sock",
                "--group-add",
                "998",
                "--network",
                "qovery-bastion",
                "--log-driver",
                "local",
                "--log-opt",
                "max-size=10m",
                "--memory",
                "268435456",
                "--memory-reservation",
                "134217728",
                "--memory-swap",
                "536870912",
                "--cpu-shares",
                "512",
                "--pids-limit",
                "256",
                "--cpus",
                "1.500000000",
                "--cpuset-cpus",
                "0-1",
                "--",
                digest.as_str(),
            ]
            .map(str::to_string)
            .to_vec()
        );
        assert!(!args.iter().any(|arg| arg.contains("secret")));
    }

    #[test]
    fn default_network_and_log_driver_add_no_flag() {
        let digest = format!("localhost:5000/operator@sha256:{}", "a".repeat(64));
        let spec = ContainerSpec {
            name: ContainerName::from("qovery-operator-next"),
            image: PinnedImage::resolve(&ImageReference::parse(&digest).unwrap(), slice::from_ref(&digest)).unwrap(),
            env: Vec::new(),
            labels: BTreeMap::new(),
            binds: Vec::new(),
            group_add: Vec::new(),
            restart_policy: RestartPolicy::NO,
            network_mode: NetworkMode::Default,
            log_config: LogConfig::default(),
            resources: Resources::default(),
        };

        let args = create_arguments(&spec, "/tmp/env", "/tmp/labels");

        assert!(!args.iter().any(|arg| arg == "--network" || arg == "--log-driver"), "{args:?}");
    }

    #[test]
    fn leftover_lookup_matches_exact_names_only() {
        assert_eq!(
            exact_name_filter(&[
                ContainerName::from("qovery.operator-next"),
                ContainerName::from("qovery.operator-previous")
            ]),
            r"name=^/(?:qovery\.operator\-next|qovery\.operator\-previous)$"
        );
    }

    #[test]
    fn key_value_file_writes_one_line_per_entry() {
        let file = write_key_value_file([("A", "1"), ("B", "x=y")].into_iter()).unwrap();
        assert_eq!(fs::read_to_string(file.path()).unwrap(), "A=1\nB=x=y\n");
    }
}
