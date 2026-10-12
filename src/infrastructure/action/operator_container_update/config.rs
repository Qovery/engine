use super::model::{
    ContainerInspect, ContainerName, ContainerStatus, EnvVar, ImageConfig, LogConfig, MountType, NetworkMode,
    OperatorVersion, PinnedImage, Resources, RestartPolicy,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use uuid::Uuid;

pub const OPERATOR_RUNTIME_ENV: &str = "QOVERY_OPERATOR_RUNTIME";
pub const OPERATOR_RUNTIME_DOCKER: &str = "DOCKER";
pub const CLUSTER_ID_ENV: &str = "CLUSTER_ID";
pub const ORGANIZATION_ID_ENV: &str = "ORGANIZATION_ID";
pub const OPERATOR_VERSION_ENV: &str = "OPERATOR_VERSION";
const DOCKER_SOCKET_PATH_ENV: &str = "DOCKER_SOCKET_PATH";
const DEFAULT_DOCKER_SOCKET_PATH: &str = "/var/run/docker.sock";

pub const UPDATE_LABEL: &str = "qovery.com/operator-update";
pub const PREVIOUS_CONTAINER_LABEL: &str = "qovery.com/operator-previous";
pub const RESTART_POLICY_LABEL: &str = "qovery.com/operator-restart-policy";
const UPDATE_LABELS: [&str; 3] = [UPDATE_LABEL, PREVIOUS_CONTAINER_LABEL, RESTART_POLICY_LABEL];

const NON_SECRET_LOG_OPTIONS: [&str; 10] = [
    "max-size",
    "max-file",
    "compress",
    "mode",
    "max-buffer-size",
    "tag",
    "labels",
    "labels-regex",
    "env",
    "env-regex",
];
const NULL_DEFAULT_FIELDS: [&str; 3] = ["MemorySwappiness", "Init", "StopTimeout"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperatorIdentity {
    pub cluster_id: Uuid,
    pub organization_id: Uuid,
}

impl OperatorIdentity {
    pub fn matches(&self, container: &ContainerInspect) -> bool {
        let env = &container.config.env;
        let env_uuid = |name: &str| env.value(name).and_then(|value| Uuid::parse_str(value).ok());
        env.value(OPERATOR_RUNTIME_ENV) == Some(OPERATOR_RUNTIME_DOCKER)
            && env_uuid(CLUSTER_ID_ENV) == Some(self.cluster_id)
            && env_uuid(ORGANIZATION_ID_ENV) == Some(self.organization_id)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum DiscoveryError {
    NotFound,
    Ambiguous(Vec<ContainerName>),
}

pub fn find_operator(
    running_containers: Vec<ContainerInspect>,
    identity: &OperatorIdentity,
) -> Result<ContainerInspect, DiscoveryError> {
    let mut matching: Vec<ContainerInspect> = running_containers
        .into_iter()
        .filter(|container| container.state.status == ContainerStatus::Running && identity.matches(container))
        .collect();
    match matching.len() {
        0 => Err(DiscoveryError::NotFound),
        1 => Ok(matching.remove(0)),
        _ => Err(DiscoveryError::Ambiguous(
            matching.into_iter().map(|container| container.name).collect(),
        )),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UnsupportedConfiguration {
    CommandOverride,
    EntrypointOverride,
    UserOverride,
    WorkingDirOverride,
    MissingDockerSocketBind,
    ExtraVolume(String),
    SharedNetworkNamespace(String),
    MultipleNetworks,
    NetworkAlias(String),
    NetworkEndpointSetting(String),
    LogOption(String),
    CustomHostname,
    UnsupportedContainerSetting(String),
    UnsupportedHostSetting(String),
    InvalidEnvironmentEntry,
    MultilineValue(String),
}

impl Display for UnsupportedConfiguration {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            UnsupportedConfiguration::CommandOverride => f.write_str("the container overrides the image command"),
            UnsupportedConfiguration::EntrypointOverride => f.write_str("the container overrides the image entrypoint"),
            UnsupportedConfiguration::UserOverride => f.write_str("the container overrides the image user"),
            UnsupportedConfiguration::WorkingDirOverride => {
                f.write_str("the container overrides the image working directory")
            }
            UnsupportedConfiguration::MissingDockerSocketBind => {
                f.write_str("the container does not bind the Docker socket at its DOCKER_SOCKET_PATH")
            }
            UnsupportedConfiguration::ExtraVolume(destination) => {
                write!(f, "the container mounts an extra volume at `{destination}`")
            }
            UnsupportedConfiguration::SharedNetworkNamespace(container) => {
                write!(f, "the container shares the network namespace of `{container}`")
            }
            UnsupportedConfiguration::MultipleNetworks => f.write_str("the container is attached to several networks"),
            UnsupportedConfiguration::NetworkAlias(alias) => {
                write!(f, "the container sets the network alias `{alias}`")
            }
            UnsupportedConfiguration::NetworkEndpointSetting(field) => {
                write!(f, "the container sets the network endpoint setting `{field}`")
            }
            UnsupportedConfiguration::LogOption(key) => {
                write!(f, "the container sets the log option `{key}`")
            }
            UnsupportedConfiguration::CustomHostname => f.write_str("the container sets a custom hostname"),
            UnsupportedConfiguration::UnsupportedContainerSetting(field) => {
                write!(f, "the container sets the unsupported setting `{field}`")
            }
            UnsupportedConfiguration::UnsupportedHostSetting(field) => {
                write!(f, "the container sets the unsupported host setting `{field}`")
            }
            UnsupportedConfiguration::InvalidEnvironmentEntry => {
                f.write_str("the container has an environment entry without `=`")
            }
            UnsupportedConfiguration::MultilineValue(name) => {
                write!(f, "`{name}` holds a multi-line value")
            }
        }
    }
}

/// Everything `docker container create` needs to recreate the Operator on the new image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerSpec {
    pub name: ContainerName,
    pub image: PinnedImage,
    pub env: Vec<EnvVar>,
    pub labels: BTreeMap<String, String>,
    pub binds: Vec<String>,
    pub group_add: Vec<String>,
    pub restart_policy: RestartPolicy,
    pub network_mode: NetworkMode,
    pub log_config: LogConfig,
    pub resources: Resources,
}

/// An Operator container whose configuration the update can reproduce on a new image.
#[derive(Debug)]
pub struct SupportedOperator {
    container: ContainerInspect,
    old_image: ImageConfig,
}

impl SupportedOperator {
    pub fn container(&self) -> &ContainerInspect {
        &self.container
    }

    pub fn restore_restart_policy(&mut self, policy: RestartPolicy) {
        self.container.host_config.restart_policy = policy;
    }

    pub fn container_spec(
        &self,
        image: &PinnedImage,
        operator_version: &OperatorVersion,
        execution_id: &str,
    ) -> ContainerSpec {
        let container = &self.container;
        let mut env: Vec<EnvVar> = container
            .config
            .env
            .entries()
            .iter()
            .filter_map(|entry| entry.split_once('='))
            .filter(|(name, value)| *name != OPERATOR_VERSION_ENV && !self.is_image_default_env(name, value))
            .map(|(name, value)| EnvVar {
                name: name.to_string(),
                value: value.to_string(),
            })
            .collect();
        env.push(EnvVar {
            name: OPERATOR_VERSION_ENV.to_string(),
            value: operator_version.as_str().to_string(),
        });

        let mut labels: BTreeMap<String, String> = container
            .config
            .labels
            .iter()
            .filter(|(key, value)| self.is_kept_label(key, value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let host_config = &container.host_config;
        labels.insert(UPDATE_LABEL.to_string(), execution_id.to_string());
        labels.insert(PREVIOUS_CONTAINER_LABEL.to_string(), container.id.to_string());
        labels.insert(RESTART_POLICY_LABEL.to_string(), host_config.restart_policy.to_string());

        ContainerSpec {
            name: container.name.next(),
            image: image.clone(),
            env,
            labels,
            binds: host_config.binds.clone(),
            group_add: host_config.group_add.clone(),
            restart_policy: host_config.restart_policy,
            network_mode: host_config.network_mode.clone(),
            log_config: host_config.log_config.clone(),
            resources: host_config.resources(),
        }
    }

    fn is_image_default_env(&self, name: &str, value: &str) -> bool {
        self.old_image.env.value(name) == Some(value)
    }

    fn is_kept_label(&self, key: &str, value: &str) -> bool {
        !UPDATE_LABELS.contains(&key) && self.old_image.labels.get(key).map(String::as_str) != Some(value)
    }
}

pub fn check_supported_profile(
    container: ContainerInspect,
    old_image: ImageConfig,
) -> Result<SupportedOperator, UnsupportedConfiguration> {
    let operator = SupportedOperator { container, old_image };
    check_command(&operator)?;
    check_environment_and_labels(&operator)?;
    check_network(&operator.container)?;
    check_mounts(&operator.container)?;
    check_log_options(&operator.container.host_config.log_config)?;
    check_other_settings(&operator)?;
    Ok(operator)
}

fn check_command(operator: &SupportedOperator) -> Result<(), UnsupportedConfiguration> {
    let config = &operator.container.config;
    let image = &operator.old_image;
    if config.cmd != image.cmd {
        return Err(UnsupportedConfiguration::CommandOverride);
    }
    if config.entrypoint != image.entrypoint {
        return Err(UnsupportedConfiguration::EntrypointOverride);
    }
    if config.user != image.user {
        return Err(UnsupportedConfiguration::UserOverride);
    }
    if config.working_dir != image.working_dir {
        return Err(UnsupportedConfiguration::WorkingDirOverride);
    }
    Ok(())
}

fn check_environment_and_labels(operator: &SupportedOperator) -> Result<(), UnsupportedConfiguration> {
    let config = &operator.container.config;
    for entry in config.env.entries() {
        let (name, value) = entry
            .split_once('=')
            .ok_or(UnsupportedConfiguration::InvalidEnvironmentEntry)?;
        if !operator.is_image_default_env(name, value) && value.contains(['\n', '\r']) {
            return Err(UnsupportedConfiguration::MultilineValue(name.to_string()));
        }
    }
    for (key, value) in &config.labels {
        if operator.is_kept_label(key, value) && (key.contains(['\n', '\r', '=']) || value.contains(['\n', '\r'])) {
            return Err(UnsupportedConfiguration::MultilineValue(key.clone()));
        }
    }
    Ok(())
}

fn check_network(container: &ContainerInspect) -> Result<(), UnsupportedConfiguration> {
    let network_mode = &container.host_config.network_mode;
    if let NetworkMode::Container(shared) = network_mode {
        return Err(UnsupportedConfiguration::SharedNetworkNamespace(shared.clone()));
    }
    if *network_mode != NetworkMode::Host && container.config.hostname != container.id.short() {
        return Err(UnsupportedConfiguration::CustomHostname);
    }

    let networks = &container.network_settings.networks;
    if networks.len() > 1 {
        return Err(UnsupportedConfiguration::MultipleNetworks);
    }
    for endpoint in networks.values() {
        if let Some(alias) = endpoint
            .aliases
            .iter()
            .find(|alias| alias.as_str() != container.id.short())
        {
            return Err(UnsupportedConfiguration::NetworkAlias(alias.clone()));
        }
        let customised = [
            ("IPAMConfig", !is_empty_value(&endpoint.ipam_config)),
            ("Links", !endpoint.links.is_empty()),
            ("DriverOpts", !endpoint.driver_opts.is_empty()),
            ("GwPriority", endpoint.gw_priority != 0),
        ];
        if let Some((field, _)) = customised.iter().find(|(_, is_set)| *is_set) {
            return Err(UnsupportedConfiguration::NetworkEndpointSetting(field.to_string()));
        }
    }
    Ok(())
}

fn check_mounts(container: &ContainerInspect) -> Result<(), UnsupportedConfiguration> {
    let socket_path = container
        .config
        .env
        .value(DOCKER_SOCKET_PATH_ENV)
        .unwrap_or(DEFAULT_DOCKER_SOCKET_PATH);
    if !container
        .host_config
        .binds
        .iter()
        .any(|bind| bind.split(':').nth(1) == Some(socket_path))
    {
        return Err(UnsupportedConfiguration::MissingDockerSocketBind);
    }
    match container
        .mounts
        .iter()
        .find(|mount| mount.kind != MountType::Bind || mount.destination != socket_path)
    {
        Some(mount) => Err(UnsupportedConfiguration::ExtraVolume(mount.destination.clone())),
        None => Ok(()),
    }
}

fn check_log_options(log_config: &LogConfig) -> Result<(), UnsupportedConfiguration> {
    match log_config
        .config
        .keys()
        .find(|key| !NON_SECRET_LOG_OPTIONS.contains(&key.as_str()))
    {
        Some(key) => Err(UnsupportedConfiguration::LogOption(key.clone())),
        None => Ok(()),
    }
}

fn check_other_settings(operator: &SupportedOperator) -> Result<(), UnsupportedConfiguration> {
    for (field, value) in &operator.container.config.other {
        let is_supported = holds_docker_default(field, value)
            || operator.old_image.other.get(field) == Some(value)
            || matches!(field.as_str(), "AttachStdout" | "AttachStderr" | "Image");
        if !is_supported {
            return Err(UnsupportedConfiguration::UnsupportedContainerSetting(field.clone()));
        }
    }
    for (field, value) in &operator.container.host_config.other {
        if !holds_docker_default(field, value) && !is_daemon_assigned_host_setting(field, value) {
            return Err(UnsupportedConfiguration::UnsupportedHostSetting(field.clone()));
        }
    }
    Ok(())
}

fn holds_docker_default(field: &str, value: &Value) -> bool {
    if NULL_DEFAULT_FIELDS.contains(&field) {
        return value.is_null();
    }
    is_empty_value(value)
}

fn is_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(entries) => entries.is_empty(),
    }
}

fn is_daemon_assigned_host_setting(field: &str, value: &Value) -> bool {
    const DEFAULT_SHM_SIZE: u64 = 64 * 1024 * 1024;
    match field {
        "MaskedPaths" | "ReadonlyPaths" | "ConsoleSize" => true,
        "IpcMode" => matches!(value.as_str(), Some("private" | "shareable")),
        "CgroupnsMode" => matches!(value.as_str(), Some("private" | "host")),
        "Runtime" => value.as_str() == Some("runc"),
        "ShmSize" => value.as_u64() == Some(DEFAULT_SHM_SIZE),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::action::operator_container_update::fixtures::{
        container, extra_env, identity, image, override_differs, override_equal, previous_update, readme, readme_env,
        rootless, running, with_env,
    };
    use crate::infrastructure::action::operator_container_update::model::{
        EndpointSettings, ImageReference, LogDriver, RestartPolicyName,
    };
    use serde_json::json;

    const NEW_DIGEST: &str =
        "localhost:5000/qovery-operator@sha256:1111111111111111111111111111111111111111111111111111111111111111";

    fn check(operator: ContainerInspect) -> Result<SupportedOperator, UnsupportedConfiguration> {
        check_supported_profile(operator, image().config)
    }

    fn build(operator: ContainerInspect) -> Result<ContainerSpec, UnsupportedConfiguration> {
        let pinned = PinnedImage::resolve(&ImageReference::parse(NEW_DIGEST).unwrap(), &[NEW_DIGEST.to_string()]);
        check(operator).map(|operator| {
            operator.container_spec(&pinned.unwrap(), &OperatorVersion::parse("n1").unwrap(), "exec-42")
        })
    }

    fn env_of(spec: &ContainerSpec) -> Vec<String> {
        spec.env
            .iter()
            .map(|variable| format!("{}={}", variable.name, variable.value))
            .collect()
    }

    fn with_version(mut env: Vec<String>) -> Vec<String> {
        env.push("OPERATOR_VERSION=n1".to_string());
        env
    }

    fn appended(extra: &str) -> Vec<String> {
        let mut env = readme_env();
        env.push(extra.to_string());
        env
    }

    fn custom_network() -> ContainerInspect {
        container("container_custom_name_network_log")
    }

    #[test]
    fn readme_install_keeps_qovery_settings_and_host_config() {
        let operator = readme();
        let previous_id = operator.id.to_string();

        let spec = build(operator).unwrap();

        assert_eq!(
            spec,
            ContainerSpec {
                name: ContainerName::from("qovery-operator-next"),
                image: spec.image.clone(),
                env: spec.env.clone(),
                labels: BTreeMap::from([
                    (UPDATE_LABEL.to_string(), "exec-42".to_string()),
                    (PREVIOUS_CONTAINER_LABEL.to_string(), previous_id),
                    (RESTART_POLICY_LABEL.to_string(), "unless-stopped".to_string()),
                ]),
                binds: vec!["/var/run/docker.sock:/var/run/docker.sock".to_string()],
                group_add: vec!["0".to_string()],
                restart_policy: RestartPolicy {
                    name: RestartPolicyName::UnlessStopped,
                    maximum_retry_count: 0,
                },
                network_mode: NetworkMode::Bridge,
                log_config: LogConfig {
                    driver: Some(LogDriver::from("json-file")),
                    config: BTreeMap::new(),
                },
                resources: Resources::default(),
            }
        );
        assert_eq!(spec.image.as_str(), NEW_DIGEST);
        assert_eq!(env_of(&spec), with_version(readme_env()));
    }

    #[test]
    fn configuration_rules_over_recorded_inspect_output() {
        let network_container = container("container_network_container");
        let NetworkMode::Container(shared) = network_container.host_config.network_mode.clone() else {
            panic!("the fixture shares a container network namespace");
        };
        for (name, operator, expected_env) in [
            ("readme", readme(), Ok(with_version(readme_env()))),
            (
                "extra env",
                extra_env(),
                Ok(with_version(appended("HTTPS_PROXY=http://proxy.internal:3128"))),
            ),
            (
                "override equal to image default",
                override_equal(),
                Ok(with_version(readme_env())),
            ),
            (
                "override differs from image default",
                override_differs(),
                Ok(with_version(appended("QOVERY_FIXTURE_DEFAULT=customer-value"))),
            ),
            ("previous update", previous_update(), Ok(with_version(readme_env()))),
            (
                "rootless socket",
                rootless(),
                Ok(with_version(appended("DOCKER_SOCKET_HOST_PATH=/run/user/1000/docker.sock"))),
            ),
            ("custom name, network and log", custom_network(), Ok(with_version(readme_env()))),
            (
                "command override",
                container("container_command_override"),
                Err(UnsupportedConfiguration::CommandOverride),
            ),
            (
                "extra volume",
                container("container_extra_volume"),
                Err(UnsupportedConfiguration::ExtraVolume("/data".to_string())),
            ),
            (
                "network container",
                network_container,
                Err(UnsupportedConfiguration::SharedNetworkNamespace(shared)),
            ),
            (
                "privileged",
                container("container_privileged"),
                Err(UnsupportedConfiguration::UnsupportedHostSetting("Privileged".to_string())),
            ),
        ] {
            assert_eq!(build(operator).map(|spec| env_of(&spec)), expected_env, "{name}");
        }
    }

    #[test]
    fn network_endpoint_settings_outside_the_profile_are_refused() {
        let with_endpoint = |change: fn(&mut EndpointSettings)| {
            let mut operator = custom_network();
            operator.network_settings.networks.values_mut().for_each(change);
            operator
        };
        let mut second_network = custom_network();
        second_network
            .network_settings
            .networks
            .insert("other".to_string(), EndpointSettings::default());
        let mut docker_alias = custom_network();
        let short_id = docker_alias.id.short().to_string();
        docker_alias
            .network_settings
            .networks
            .values_mut()
            .for_each(|endpoint| endpoint.aliases = vec![short_id.clone()]);

        assert!(check(docker_alias).is_ok());
        for (name, operator, expected) in [
            ("two networks", second_network, UnsupportedConfiguration::MultipleNetworks),
            (
                "alias",
                with_endpoint(|endpoint| endpoint.aliases = vec!["operator".to_string()]),
                UnsupportedConfiguration::NetworkAlias("operator".to_string()),
            ),
            (
                "static ip",
                with_endpoint(|endpoint| endpoint.ipam_config = json!({"IPv4Address": "172.30.0.10"})),
                UnsupportedConfiguration::NetworkEndpointSetting("IPAMConfig".to_string()),
            ),
            (
                "link",
                with_endpoint(|endpoint| endpoint.links = vec!["qovery-db:db".to_string()]),
                UnsupportedConfiguration::NetworkEndpointSetting("Links".to_string()),
            ),
            (
                "driver option",
                with_endpoint(|endpoint| {
                    endpoint.driver_opts = BTreeMap::from([("com.example.opt".to_string(), "x".to_string())])
                }),
                UnsupportedConfiguration::NetworkEndpointSetting("DriverOpts".to_string()),
            ),
            (
                "gateway priority",
                with_endpoint(|endpoint| endpoint.gw_priority = 10),
                UnsupportedConfiguration::NetworkEndpointSetting("GwPriority".to_string()),
            ),
        ] {
            assert_eq!(check(operator).unwrap_err(), expected, "{name}");
        }
    }

    #[test]
    fn log_options_that_only_name_fields_are_kept() {
        let mut operator = custom_network();
        operator.host_config.log_config.config.extend(
            [
                ("tag", "{{.Name}}"),
                ("labels", "com.example.team"),
                ("labels-regex", "^com\\.example"),
                ("env", "CLUSTER_ID"),
                ("env-regex", "^QOVERY_"),
            ]
            .map(|(key, value)| (key.to_string(), value.to_string())),
        );
        let expected = operator.host_config.log_config.clone();

        assert_eq!(build(operator).unwrap().log_config, expected);
    }

    #[test]
    fn mac_address_reported_by_older_daemons_is_refused() {
        let mut operator = readme();
        operator
            .config
            .other
            .insert("MacAddress".to_string(), json!("02:42:ac:11:00:99"));

        assert_eq!(
            check(operator).unwrap_err(),
            UnsupportedConfiguration::UnsupportedContainerSetting("MacAddress".to_string())
        );
    }

    #[test]
    fn log_options_that_may_carry_secrets_are_refused() {
        let mut splunk = custom_network();
        splunk.host_config.log_config = LogConfig {
            driver: Some(LogDriver::from("splunk")),
            config: BTreeMap::from([("splunk-token".to_string(), "secret".to_string())]),
        };
        let debug = format!("{splunk:?}");
        assert!(debug.contains("splunk-token") && !debug.contains("\"secret\""), "{debug}");

        assert_eq!(
            check(splunk).unwrap_err(),
            UnsupportedConfiguration::LogOption("splunk-token".to_string())
        );
    }

    #[test]
    fn null_default_settings_are_refused_when_set_to_zero_or_false() {
        for (field, value) in [
            ("MemorySwappiness", json!(0)),
            ("Init", json!(false)),
            ("OomKillDisable", json!(true)),
        ] {
            let mut operator = readme();
            operator.host_config.other.insert(field.to_string(), value);
            assert_eq!(
                check(operator).unwrap_err(),
                UnsupportedConfiguration::UnsupportedHostSetting(field.to_string()),
                "{field}"
            );
        }
        let mut stop_timeout = readme();
        stop_timeout.config.other.insert("StopTimeout".to_string(), json!(0));
        assert_eq!(
            check(stop_timeout).unwrap_err(),
            UnsupportedConfiguration::UnsupportedContainerSetting("StopTimeout".to_string())
        );
        for (field, value) in [("BlkioWeight", json!(0)), ("OomKillDisable", json!(false))] {
            let mut operator = readme();
            operator.host_config.other.insert(field.to_string(), value);
            assert!(check(operator).is_ok(), "{field}");
        }
    }

    #[test]
    fn environment_and_labels_that_cannot_be_written_to_a_file_are_refused() {
        assert_eq!(
            check(with_env(readme(), "BROKEN")).unwrap_err(),
            UnsupportedConfiguration::InvalidEnvironmentEntry
        );
        assert_eq!(
            check(with_env(readme(), "PROXY=a\nCLUSTER_ID=x")).unwrap_err(),
            UnsupportedConfiguration::MultilineValue("PROXY".to_string())
        );
        let mut multiline_label = readme();
        multiline_label
            .config
            .labels
            .insert("com.example.note".to_string(), "a\nb".to_string());
        assert_eq!(
            check(multiline_label).unwrap_err(),
            UnsupportedConfiguration::MultilineValue("com.example.note".to_string())
        );
    }

    #[test]
    fn operator_without_the_docker_socket_is_refused() {
        let mut operator = readme();
        operator.host_config.binds.clear();
        operator.mounts.clear();

        assert_eq!(check(operator).unwrap_err(), UnsupportedConfiguration::MissingDockerSocketBind);
    }

    #[test]
    fn rootless_socket_keeps_the_host_side_path() {
        let spec = build(rootless()).unwrap();
        assert_eq!(spec.binds, vec!["/run/user/1000/docker.sock:/var/run/docker.sock".to_string()]);
    }

    #[test]
    fn custom_name_network_and_log_driver_are_reproduced() {
        let spec = build(custom_network()).unwrap();
        assert_eq!(spec.name, ContainerName::from("bastion-operator-next"));
        assert_eq!(spec.network_mode, NetworkMode::UserDefined("qovery-bastion".to_string()));
        assert_eq!(
            spec.log_config,
            LogConfig {
                driver: Some(LogDriver::from("local")),
                config: BTreeMap::from([("max-size".to_string(), "10m".to_string())]),
            }
        );
        assert_eq!(spec.restart_policy.to_string(), "on-failure:3");
        assert_eq!(spec.resources.memory, 268_435_456);
        assert_eq!(spec.resources.nano_cpus, 500_000_000);
    }

    #[test]
    fn labels_drop_image_defaults_and_previous_update_history() {
        let operator = previous_update();
        let previous_id = operator.id.to_string();

        let spec = build(operator).unwrap();

        assert_eq!(
            spec.labels,
            BTreeMap::from([
                ("com.example.team".to_string(), "platform".to_string()),
                (UPDATE_LABEL.to_string(), "exec-42".to_string()),
                (PREVIOUS_CONTAINER_LABEL.to_string(), previous_id),
                (RESTART_POLICY_LABEL.to_string(), "unless-stopped".to_string()),
            ])
        );
    }

    #[test]
    fn discovery_requires_exactly_one_matching_running_operator() {
        let operator = readme();
        let worker = container("container_worker");
        let other_cluster = with_env(readme(), &format!("{CLUSTER_ID_ENV}={}", Uuid::new_v4()));
        let other_organization = with_env(readme(), &format!("{ORGANIZATION_ID_ENV}={}", Uuid::new_v4()));
        let second = running("container_custom_name_network_log");

        let found = find_operator(vec![worker.clone(), operator.clone()], &identity()).unwrap();
        assert_eq!(found.id, operator.id);

        for (name, containers) in [
            ("worker only", vec![worker.clone()]),
            ("other cluster", vec![other_cluster]),
            ("other organization", vec![other_organization]),
        ] {
            assert_eq!(
                find_operator(containers, &identity()).unwrap_err(),
                DiscoveryError::NotFound,
                "{name}"
            );
        }
        assert_eq!(
            find_operator(vec![operator, worker, second], &identity()).unwrap_err(),
            DiscoveryError::Ambiguous(vec![
                ContainerName::from("qovery-operator"),
                ContainerName::from("bastion-operator")
            ])
        );
    }
}
