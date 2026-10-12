use crate::cmd::docker::Architecture;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize)]
#[serde(transparent)]
pub struct ContainerId(String);

impl ContainerId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn short(&self) -> &str {
        self.0.get(..12).unwrap_or(&self.0)
    }
}

impl From<&str> for ContainerId {
    fn from(value: &str) -> Self {
        ContainerId(value.to_string())
    }
}

impl Display for ContainerId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ContainerName(String);

impl ContainerName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn next(&self) -> ContainerName {
        ContainerName(format!("{}-next", self.0))
    }

    pub fn previous(&self) -> ContainerName {
        ContainerName(format!("{}-previous", self.0))
    }
}

impl From<&str> for ContainerName {
    fn from(value: &str) -> Self {
        ContainerName(value.trim_start_matches('/').to_string())
    }
}

impl<'de> Deserialize<'de> for ContainerName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(ContainerName::from(raw.as_str()))
    }
}

impl Display for ContainerName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct ImageId(String);

impl ImageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ImageReferenceError {
    Empty,
    InvalidCharacter,
    MissingTagOrDigest,
    LatestTag,
    InvalidDigest,
}

impl Display for ImageReferenceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ImageReferenceError::Empty => "image reference is empty",
            ImageReferenceError::InvalidCharacter => "image reference contains an invalid character",
            ImageReferenceError::MissingTagOrDigest => "image reference must carry an explicit tag or digest",
            ImageReferenceError::LatestTag => "image reference must not use the `latest` tag",
            ImageReferenceError::InvalidDigest => "image digest must be `sha256:` followed by 64 hex characters",
        })
    }
}

/// A validated `<repository>:<tag>` or `<repository>@sha256:<hex>` reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageReference {
    reference: String,
    repository_end: usize,
}

impl ImageReference {
    pub fn parse(reference: &str) -> Result<Self, ImageReferenceError> {
        if reference.is_empty() {
            return Err(ImageReferenceError::Empty);
        }
        let is_allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '@');
        if reference.starts_with(['-', '/', ':', '@']) || !reference.chars().all(is_allowed) {
            return Err(ImageReferenceError::InvalidCharacter);
        }

        if let Some((repository, digest)) = reference.split_once('@') {
            let hex = digest
                .strip_prefix("sha256:")
                .ok_or(ImageReferenceError::InvalidDigest)?;
            if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
                return Err(ImageReferenceError::InvalidDigest);
            }
            let repository_end = repository_end_without_tag(repository);
            if repository_end == 0 {
                return Err(ImageReferenceError::Empty);
            }
            return Ok(ImageReference {
                reference: reference.to_string(),
                repository_end,
            });
        }

        let repository_end = repository_end_without_tag(reference);
        if repository_end == reference.len() {
            return Err(ImageReferenceError::MissingTagOrDigest);
        }
        let tag = &reference[repository_end + 1..];
        if tag.is_empty() || tag.contains(['/', ':']) {
            return Err(ImageReferenceError::MissingTagOrDigest);
        }
        if tag == "latest" {
            return Err(ImageReferenceError::LatestTag);
        }
        Ok(ImageReference {
            reference: reference.to_string(),
            repository_end,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.reference
    }

    pub fn repository(&self) -> &str {
        &self.reference[..self.repository_end]
    }

    pub fn digest(&self) -> Option<&str> {
        self.reference.split_once('@').map(|(_, digest)| digest)
    }
}

fn repository_end_without_tag(reference: &str) -> usize {
    let last_slash = reference.rfind('/').map(|index| index + 1).unwrap_or(0);
    match reference[last_slash..].find(':') {
        Some(colon) => last_slash + colon,
        None => reference.len(),
    }
}

impl Display for ImageReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reference)
    }
}

/// The digest reference (`<repository>@sha256:<hex>`) the new container is created from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinnedImage(String);

impl PinnedImage {
    /// Pins a tag to the first digest Docker records for its repository, and a digest request to
    /// exactly that digest.
    pub fn resolve(requested: &ImageReference, repo_digests: &[String]) -> Option<PinnedImage> {
        let wanted = familiar_repository(requested.repository());
        repo_digests
            .iter()
            .filter_map(|repo_digest| ImageReference::parse(repo_digest).ok())
            .filter(|candidate| familiar_repository(candidate.repository()) == wanted)
            .find(|candidate| match (requested.digest(), candidate.digest()) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(wanted_digest), Some(digest)) => wanted_digest == digest,
            })
            .map(|candidate| PinnedImage(candidate.reference))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for PinnedImage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn familiar_repository(repository: &str) -> &str {
    let without_registry = repository.strip_prefix("docker.io/").unwrap_or(repository);
    without_registry.strip_prefix("library/").unwrap_or(without_registry)
}

/// How an image is looked up in the local daemon.
#[derive(Clone, Copy, Debug)]
pub enum ImageLookup<'a> {
    Id(&'a ImageId),
    Reference(&'a ImageReference),
}

impl ImageLookup<'_> {
    pub fn as_str(&self) -> &str {
        match self {
            ImageLookup::Id(id) => id.as_str(),
            ImageLookup::Reference(reference) => reference.as_str(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OperatorVersionError;

impl Display for OperatorVersionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("operator_version must be 1-128 characters among letters, digits, `.`, `_`, `+` and `-`")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorVersion(String);

impl OperatorVersion {
    pub fn parse(version: &str) -> Result<Self, OperatorVersionError> {
        let is_allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-');
        if version.is_empty() || version.len() > 128 || !version.chars().all(is_allowed) {
            return Err(OperatorVersionError);
        }
        Ok(OperatorVersion(version.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

impl Debug for EnvVar {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}=<redacted>", self.name)
    }
}

/// `NAME=value` entries as Docker reports them; values may hold secrets and never reach `Debug`.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct Environment(pub(super) Vec<String>);

impl Environment {
    pub fn entries(&self) -> &[String] {
        &self.0
    }

    /// Returns the value of the last entry named `name`, as Docker applies the last one.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .filter_map(|entry| entry.split_once('='))
            .find(|(entry_name, _)| *entry_name == name)
            .map(|(_, value)| value)
    }
}

impl Debug for Environment {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(
                self.0
                    .iter()
                    .map(|entry| format!("{}=<redacted>", entry.split('=').next().unwrap_or_default())),
            )
            .finish()
    }
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Environment(
            Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default(),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
pub enum RestartPolicyName {
    #[serde(rename = "no", alias = "")]
    No,
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "unless-stopped")]
    UnlessStopped,
    #[serde(rename = "on-failure")]
    OnFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RestartPolicy {
    pub name: RestartPolicyName,
    #[serde(default)]
    pub maximum_retry_count: u32,
}

impl RestartPolicy {
    pub const NO: RestartPolicy = RestartPolicy {
        name: RestartPolicyName::No,
        maximum_retry_count: 0,
    };

    pub fn keeps_a_stopped_container_stopped(&self) -> bool {
        matches!(self.name, RestartPolicyName::No | RestartPolicyName::UnlessStopped)
    }
}

impl Display for RestartPolicy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match (self.name, self.maximum_retry_count) {
            (RestartPolicyName::No, _) => f.write_str("no"),
            (RestartPolicyName::Always, _) => f.write_str("always"),
            (RestartPolicyName::UnlessStopped, _) => f.write_str("unless-stopped"),
            (RestartPolicyName::OnFailure, 0) => f.write_str("on-failure"),
            (RestartPolicyName::OnFailure, count) => write!(f, "on-failure:{count}"),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RestartPolicyParseError(pub String);

impl Display for RestartPolicyParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` is not a Docker restart policy", self.0)
    }
}

impl FromStr for RestartPolicy {
    type Err = RestartPolicyParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let policy = |name| RestartPolicy {
            name,
            maximum_retry_count: 0,
        };
        match value.split_once(':') {
            None => match value {
                "no" => Ok(RestartPolicy::NO),
                "always" => Ok(policy(RestartPolicyName::Always)),
                "unless-stopped" => Ok(policy(RestartPolicyName::UnlessStopped)),
                "on-failure" => Ok(policy(RestartPolicyName::OnFailure)),
                _ => Err(RestartPolicyParseError(value.to_string())),
            },
            Some(("on-failure", count)) => count
                .parse()
                .map(|maximum_retry_count| RestartPolicy {
                    name: RestartPolicyName::OnFailure,
                    maximum_retry_count,
                })
                .map_err(|_| RestartPolicyParseError(value.to_string())),
            Some(_) => Err(RestartPolicyParseError(value.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerStatus {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerState {
    pub status: ContainerStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MountType {
    Bind,
    Volume,
    Tmpfs,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MountPoint {
    #[serde(rename = "Type")]
    pub kind: MountType,
    pub destination: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkMode {
    Default,
    Bridge,
    Host,
    None,
    Container(String),
    UserDefined(String),
}

impl NetworkMode {
    pub fn cli_value(&self) -> Option<String> {
        match self {
            NetworkMode::Default => None,
            NetworkMode::Bridge => Some("bridge".to_string()),
            NetworkMode::Host => Some("host".to_string()),
            NetworkMode::None => Some("none".to_string()),
            NetworkMode::Container(container) => Some(format!("container:{container}")),
            NetworkMode::UserDefined(network) => Some(network.clone()),
        }
    }
}

impl From<&str> for NetworkMode {
    fn from(value: &str) -> Self {
        match value {
            "" | "default" => NetworkMode::Default,
            "bridge" => NetworkMode::Bridge,
            "host" => NetworkMode::Host,
            "none" => NetworkMode::None,
            other => match other.strip_prefix("container:") {
                Some(container) => NetworkMode::Container(container.to_string()),
                None => NetworkMode::UserDefined(other.to_string()),
            },
        }
    }
}

impl<'de> Deserialize<'de> for NetworkMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Option::<String>::deserialize(deserializer)?.unwrap_or_default();
        Ok(NetworkMode::from(raw.as_str()))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EndpointSettings {
    #[serde(default, deserialize_with = "null_as_default")]
    pub aliases: Vec<String>,
    #[serde(rename = "IPAMConfig", default)]
    pub ipam_config: Value,
    #[serde(default, deserialize_with = "null_as_default")]
    pub links: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub driver_opts: BTreeMap<String, String>,
    #[serde(default)]
    pub gw_priority: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NetworkSettings {
    #[serde(default, deserialize_with = "null_as_default")]
    pub networks: BTreeMap<String, EndpointSettings>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogDriver(String);

impl LogDriver {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for LogDriver {
    fn from(value: &str) -> Self {
        LogDriver(value.to_string())
    }
}

#[derive(Clone, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LogConfig {
    #[serde(rename = "Type", default, deserialize_with = "non_empty_log_driver")]
    pub driver: Option<LogDriver>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub config: BTreeMap<String, String>,
}

impl Debug for LogConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogConfig")
            .field("driver", &self.driver)
            .field("options", &self.config.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Resources {
    pub memory: i64,
    pub memory_reservation: i64,
    pub memory_swap: i64,
    pub nano_cpus: i64,
    pub cpu_shares: i64,
    pub cpuset_cpus: String,
    pub pids_limit: Option<i64>,
}

/// The `HostConfig` fields the update reproduces; every other field lands in `other`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct HostConfig {
    #[serde(default, deserialize_with = "null_as_default")]
    pub binds: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub group_add: Vec<String>,
    pub restart_policy: RestartPolicy,
    pub network_mode: NetworkMode,
    #[serde(default)]
    pub log_config: LogConfig,
    #[serde(default)]
    pub memory: i64,
    #[serde(default)]
    pub memory_reservation: i64,
    #[serde(default)]
    pub memory_swap: i64,
    #[serde(default)]
    pub nano_cpus: i64,
    #[serde(default)]
    pub cpu_shares: i64,
    #[serde(default)]
    pub cpuset_cpus: String,
    #[serde(default)]
    pub pids_limit: Option<i64>,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

impl HostConfig {
    pub fn resources(&self) -> Resources {
        Resources {
            memory: self.memory,
            memory_reservation: self.memory_reservation,
            memory_swap: self.memory_swap,
            nano_cpus: self.nano_cpus,
            cpu_shares: self.cpu_shares,
            cpuset_cpus: self.cpuset_cpus.clone(),
            pids_limit: self.pids_limit,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerConfig {
    #[serde(default)]
    pub env: Environment,
    #[serde(default, deserialize_with = "null_as_default")]
    pub cmd: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub entrypoint: Vec<String>,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub working_dir: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub hostname: String,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerInspect {
    pub id: ContainerId,
    pub name: ContainerName,
    pub image: ImageId,
    pub state: ContainerState,
    #[serde(default)]
    pub restart_count: u64,
    pub config: ContainerConfig,
    pub host_config: HostConfig,
    #[serde(default, deserialize_with = "null_as_default")]
    pub mounts: Vec<MountPoint>,
    #[serde(default)]
    pub network_settings: NetworkSettings,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ImageConfig {
    #[serde(default)]
    pub env: Environment,
    #[serde(default, deserialize_with = "null_as_default")]
    pub cmd: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub entrypoint: Vec<String>,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub working_dir: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub labels: BTreeMap<String, String>,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ImageInspect {
    #[serde(default, deserialize_with = "null_as_default")]
    pub repo_digests: Vec<String>,
    pub os: String,
    pub architecture: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub config: ImageConfig,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformOs {
    Linux,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PlatformError {
    UnsupportedOs(String),
    UnsupportedArchitecture(String),
}

impl Display for PlatformError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformError::UnsupportedOs(os) => write!(f, "unsupported operating system `{os}`"),
            PlatformError::UnsupportedArchitecture(architecture) => {
                write!(f, "unsupported architecture `{architecture}`")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Platform {
    pub os: PlatformOs,
    pub architecture: Architecture,
}

impl Platform {
    pub fn parse(os: &str, architecture: &str) -> Result<Platform, PlatformError> {
        let os = match os {
            "linux" => PlatformOs::Linux,
            other => return Err(PlatformError::UnsupportedOs(other.to_string())),
        };
        let architecture = Architecture::from_str(architecture)
            .map_err(|_| PlatformError::UnsupportedArchitecture(architecture.to_string()))?;
        Ok(Platform { os, architecture })
    }
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.os {
            PlatformOs::Linux => write!(f, "linux/{}", self.architecture),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ServerVersion {
    pub os: String,
    pub arch: String,
}

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn non_empty_log_driver<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<LogDriver>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?
        .filter(|driver| !driver.is_empty())
        .map(LogDriver))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::slice;

    #[test]
    fn image_reference_accepts_tags_and_digests() {
        let digest = format!("public.ecr.aws/r3m4q3r9/qovery-operator@sha256:{}", "a".repeat(64));
        for (reference, repository) in [
            (
                "public.ecr.aws/r3m4q3r9/qovery-operator:v1.222.0",
                "public.ecr.aws/r3m4q3r9/qovery-operator",
            ),
            ("localhost:5000/operator:n1", "localhost:5000/operator"),
            (digest.as_str(), "public.ecr.aws/r3m4q3r9/qovery-operator"),
        ] {
            let parsed = ImageReference::parse(reference).unwrap();
            assert_eq!(parsed.repository(), repository, "{reference}");
            assert_eq!(parsed.as_str(), reference);
        }
    }

    #[test]
    fn image_reference_refuses_ambiguous_or_unsafe_values() {
        for (reference, expected) in [
            ("", ImageReferenceError::Empty),
            ("--privileged", ImageReferenceError::InvalidCharacter),
            ("repo:tag with-space", ImageReferenceError::InvalidCharacter),
            ("localhost:5000/operator", ImageReferenceError::MissingTagOrDigest),
            ("qovery/operator", ImageReferenceError::MissingTagOrDigest),
            ("qovery/operator:latest", ImageReferenceError::LatestTag),
            ("qovery/operator@sha256:abc", ImageReferenceError::InvalidDigest),
            ("qovery/operator@md5:abc", ImageReferenceError::InvalidDigest),
        ] {
            assert_eq!(ImageReference::parse(reference), Err(expected), "{reference}");
        }
    }

    #[test]
    fn pinned_image_matches_the_requested_repository() {
        let digest = "b".repeat(64);
        let requested = ImageReference::parse("qovery/operator:v2").unwrap();
        let pinned = PinnedImage::resolve(
            &requested,
            &[
                format!("other/image@sha256:{}", "c".repeat(64)),
                format!("docker.io/qovery/operator@sha256:{digest}"),
            ],
        )
        .unwrap();
        assert_eq!(pinned.as_str(), format!("docker.io/qovery/operator@sha256:{digest}"));
        assert_eq!(PinnedImage::resolve(&requested, &[]), None);
    }

    #[test]
    fn digest_request_pins_exactly_the_requested_digest() {
        let digest_a = format!("public.ecr.aws/qovery/operator@sha256:{}", "a".repeat(64));
        let digest_b = format!("public.ecr.aws/qovery/operator@sha256:{}", "b".repeat(64));
        let requested = ImageReference::parse(&digest_a).unwrap();

        let pinned = PinnedImage::resolve(&requested, &[digest_b.clone(), digest_a.clone()]).unwrap();

        assert_eq!(pinned.as_str(), digest_a);
        assert_eq!(PinnedImage::resolve(&requested, slice::from_ref(&digest_b)), None);
    }

    #[test]
    fn operator_version_refuses_values_that_could_escape_an_env_file_line() {
        assert!(OperatorVersion::parse("v1.223.0").is_ok());
        for version in ["", "v1\nCLUSTER_ID=x", "v 1", "v1=2", &"v".repeat(129)] {
            assert_eq!(OperatorVersion::parse(version), Err(OperatorVersionError), "{version:?}");
        }
    }

    #[test]
    fn restart_policy_round_trips_between_inspect_cli_and_label() {
        for (json, cli) in [
            (r#"{"Name":"","MaximumRetryCount":0}"#, "no"),
            (r#"{"Name":"always","MaximumRetryCount":0}"#, "always"),
            (r#"{"Name":"unless-stopped","MaximumRetryCount":0}"#, "unless-stopped"),
            (r#"{"Name":"on-failure","MaximumRetryCount":0}"#, "on-failure"),
            (r#"{"Name":"on-failure","MaximumRetryCount":5}"#, "on-failure:5"),
        ] {
            let policy: RestartPolicy = serde_json::from_str(json).unwrap();
            assert_eq!(policy.to_string(), cli);
            assert_eq!(cli.parse::<RestartPolicy>(), Ok(policy));
        }
        for invalid in ["", "sometimes", "on-failure:x", "always:3"] {
            assert_eq!(
                invalid.parse::<RestartPolicy>(),
                Err(RestartPolicyParseError(invalid.to_string()))
            );
        }
    }

    #[test]
    fn network_mode_reads_every_docker_spelling() {
        for (raw, expected) in [
            ("", NetworkMode::Default),
            ("default", NetworkMode::Default),
            ("bridge", NetworkMode::Bridge),
            ("host", NetworkMode::Host),
            ("none", NetworkMode::None),
            ("container:sidecar", NetworkMode::Container("sidecar".to_string())),
            ("qovery-bastion", NetworkMode::UserDefined("qovery-bastion".to_string())),
        ] {
            assert_eq!(NetworkMode::from(raw), expected, "{raw}");
        }
    }

    #[test]
    fn platform_parse_reports_what_is_unsupported() {
        assert_eq!(
            Platform::parse("windows", "amd64"),
            Err(PlatformError::UnsupportedOs("windows".to_string()))
        );
        assert_eq!(
            Platform::parse("linux", "s390x"),
            Err(PlatformError::UnsupportedArchitecture("s390x".to_string()))
        );
    }

    #[test]
    fn secrets_never_reach_debug_output() {
        let env = EnvVar {
            name: "CLUSTER_JWT_TOKEN".to_string(),
            value: "secret".to_string(),
        };
        assert_eq!(format!("{env:?}"), "CLUSTER_JWT_TOKEN=<redacted>");
        let config: ContainerConfig =
            serde_json::from_str(r#"{"Env":["CLUSTER_JWT_TOKEN=secret"],"Hostname":"abc"}"#).unwrap();
        let debug = format!("{config:?}");
        assert!(debug.contains("CLUSTER_JWT_TOKEN=<redacted>"), "{debug}");
        assert!(!debug.contains("secret"), "{debug}");
        let log_config: LogConfig =
            serde_json::from_str(r#"{"Type":"splunk","Config":{"splunk-token":"secret"}}"#).unwrap();
        let debug = format!("{log_config:?}");
        assert!(debug.contains("splunk-token"), "{debug}");
        assert!(!debug.contains("\"secret\""), "{debug}");
    }

    #[test]
    fn environment_value_is_the_last_entry_of_a_duplicated_name() {
        let env = Environment(
            ["RUST_LOG=info", "CLUSTER_ID=a", "RUST_LOG=debug", "EMPTY="]
                .map(str::to_string)
                .to_vec(),
        );

        assert_eq!(env.value("RUST_LOG"), Some("debug"));
        assert_eq!(env.value("CLUSTER_ID"), Some("a"));
        assert_eq!(env.value("EMPTY"), Some(""));
        assert_eq!(env.value("MISSING"), None);
    }
}
