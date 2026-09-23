use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::cmd::docker::{CacheCompression, DockerError};
use crate::environment::report::logger::EnvLogger;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::infrastructure::models::container_registry::Kind as RegistryKind;

use crate::environment::models::abort::Abort;
use crate::io_models::container::Registry;
use crate::io_models::models::CpuArchitecture;
use crate::metrics_registry::MetricsRegistry;
use crate::utilities::{compute_cache_tag, compute_image_tag};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub mod dockerfile_utils;
pub mod local_docker;

#[derive(Debug)]
pub enum GitCmd {
    Fetch,
    Checkout,
    Submodule,
    SubmoduleUpdate,
}

impl Display for GitCmd {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let msg = match self {
            GitCmd::Fetch => "git fetch",
            GitCmd::Checkout => "git checkout",
            GitCmd::Submodule => "git submodule",
            GitCmd::SubmoduleUpdate => "git submodule update",
        };
        f.write_str(msg)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum BuildError {
    #[error("Cannot build Application {application:?} due to an invalid config: {raw_error_message:?}")]
    InvalidConfig {
        application: String,
        raw_error_message: String,
    },

    #[error(
        "Git error, the cmd '{git_cmd}' done for {context} has failed for {application} due to error: {raw_error:?}"
    )]
    GitError {
        application: String,
        git_cmd: GitCmd,
        context: String,
        raw_error: git2::Error,
    },

    #[error("Build of Application {application:?} have been aborted at user request")]
    Aborted { application: String },

    #[error("Cannot build Application {application:?} due to an io error: {action_description:?} {raw_error:?}")]
    IoError {
        application: String,
        action_description: String,
        raw_error: std::io::Error,
    },

    #[error("Cannot build Application {application:?} due to an error with docker: {raw_error:?}")]
    DockerError {
        application: String,
        raw_error: DockerError,
    },

    #[error("Cannot get credentials error.")]
    CannotGetCredentials { raw_error_message: String },
}

pub fn to_build_error(service_id: String, err: DockerError) -> BuildError {
    match err {
        DockerError::Aborted { .. } => BuildError::Aborted {
            application: service_id,
        },
        _ => BuildError::DockerError {
            application: service_id,
            raw_error: err,
        },
    }
}

pub fn to_engine_error(event_details: EventDetails, err: BuildError, user_message: String) -> EngineError {
    match err {
        BuildError::Aborted { .. } => EngineError::new_task_cancellation_requested(event_details),
        _ => EngineError::new_build_error(event_details, err, user_message),
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum DockerfileFragment {
    /// Fragment content read from a file in the Git repository.
    /// The path is relative to the root_module_path.
    File { path: String },
    /// Fragment content provided directly via API.
    Inline { content: String },
}

pub trait BuildPlatform: Send + Sync {
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn build(
        &self,
        build: &mut Build,
        cache_compression: CacheCompression,
        logger: &EnvLogger,
        metrics_registry: Arc<dyn MetricsRegistry>,
        cancellation_requested: &dyn Abort,
    ) -> Result<(), BuildError>;
}

/// Where the Docker build context and Dockerfile come from.
pub enum BuildSource {
    /// Context cloned from a Git repository. Boxed because it dwarfs the other variant, which would
    /// otherwise make every source-less `Build` carry its footprint.
    Git(Box<GitRepository>),
    /// No source repository: the engine synthesises the whole Dockerfile and builds it against an
    /// empty context. Used for images that only layer extra instructions onto a base image.
    Dockerfile { content: String },
}

/// Dockerfile name written in the build context of a [`BuildSource::Dockerfile`] build.
pub const SYNTHESIZED_DOCKERFILE_NAME: &str = "Dockerfile.qovery";

/// Placeholder a Dockerfile must carry for a [`DockerfileFragment`] to be spliced into it at build
/// time. A Dockerfile without it is rejected when a fragment is configured.
pub const CUSTOM_FRAGMENT_PLACEHOLDER: &str = "#{{custom_fragment}}";

pub struct Build {
    pub source: BuildSource,
    pub image: Image,
    pub environment_variables: BTreeMap<String, String>,
    pub disable_buildkit_cache: bool,
    pub timeout: Duration,
    pub architectures: Vec<CpuArchitecture>,
    pub max_cpu_in_milli: u32,
    pub max_ram_in_gib: u32,
    pub ephemeral_storage_in_gib: Option<u32>,
    // registries used by the build where we need to login to pull image
    pub registries: Vec<Registry>,
    pub dockerfile_fragment: Option<DockerfileFragment>,
    /// Dockerfile `ARG` names as parsed by the core, when it knows them. Lets the tag be computed
    /// before the repository is cloned; `None` falls back to hashing every variable.
    pub tag_build_args: Option<BTreeSet<String>>,
}

/// zstd only for registries known to accept zstd blobs: a rejected cache export fails the whole build,
/// while gzip costs nothing but speed. Move a registry to zstd once a zstd cache export is verified on it.
pub fn cache_compression_for_registry(kind: RegistryKind) -> CacheCompression {
    match kind {
        RegistryKind::Ecr
        | RegistryKind::AzureContainerRegistry
        | RegistryKind::GcpArtifactRegistry
        | RegistryKind::DockerHub
        | RegistryKind::GithubCr
        | RegistryKind::ScalewayCr => CacheCompression::Zstd,
        // Covers self-hosted registries of any product and version: support cannot be assumed.
        RegistryKind::GenericCr => CacheCompression::Gzip,
    }
}

impl Build {
    pub fn git_repository(&self) -> Option<&GitRepository> {
        match &self.source {
            BuildSource::Git(repository) => Some(repository.as_ref()),
            BuildSource::Dockerfile { .. } => None,
        }
    }

    /// Commit the image was built from, empty for builds without a source repository.
    pub fn commit_id(&self) -> &str {
        match &self.source {
            BuildSource::Git(repository) => repository.commit_id.as_str(),
            BuildSource::Dockerfile { .. } => "",
        }
    }

    /// Variables the tag is hashed from. `tag_build_args` narrows the hash only; it never removes
    /// a variable from the build itself, so getting it wrong costs a cache miss, not a broken build.
    fn variables_to_hash(&self) -> BTreeMap<String, String> {
        match &self.tag_build_args {
            None => self.environment_variables.clone(),
            Some(tag_build_args) => self
                .environment_variables
                .iter()
                .filter(|(name, _)| tag_build_args.contains(*name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        }
    }

    /// Adopt the Dockerfile the engine just read as the source of truth for the tag. One call so
    /// the three steps can never drift apart and leave the tag describing a state that never was.
    /// Secret mount ids are kept: their values are hashed, so a rotated secret forces a rebuild.
    pub fn resolve_image_tag_from_dockerfile(
        &mut self,
        arg_names: &HashSet<String>,
        secret_mount_ids: &HashSet<String>,
    ) {
        self.environment_variables
            .retain(|name, _| arg_names.contains(name) || secret_mount_ids.contains(name));
        self.tag_build_args = None;
        self.compute_image_tag();
    }

    pub fn compute_image_tag(&mut self) {
        let variables = self.variables_to_hash();
        self.image.tag = match &self.source {
            BuildSource::Git(repository) => compute_image_tag(
                &repository.root_path,
                &repository.dockerfile_path,
                &repository.dockerfile_content,
                &repository.extra_files_to_inject,
                &variables,
                &repository.commit_id,
                &repository.docker_target_build_stage,
                &self.dockerfile_fragment,
                repository.skip_submodules,
            ),
            // No repository, so no commit id to key on: the hash of the Dockerfile content and of
            // the fragment is what makes the tag unique.
            BuildSource::Dockerfile { content } => compute_image_tag(
                PathBuf::from("."),
                &Some(PathBuf::from(SYNTHESIZED_DOCKERFILE_NAME)),
                &Some(content.clone()),
                &[],
                &variables,
                "",
                &None,
                &self.dockerfile_fragment,
                true,
            ),
        };
    }

    pub fn compute_cache_tag(&self) -> String {
        match &self.source {
            BuildSource::Git(repository) => compute_cache_tag(
                &repository.root_path,
                &repository.dockerfile_path,
                &repository.extra_files_to_inject,
                &repository.docker_target_build_stage,
            ),
            BuildSource::Dockerfile { .. } => compute_cache_tag(
                PathBuf::from("."),
                &Some(PathBuf::from(SYNTHESIZED_DOCKERFILE_NAME)),
                &[],
                &None,
            ),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Credentials {
    pub login: String,
    pub password: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct SshKey {
    pub private_key: String,
    pub passphrase: Option<String>,
    pub public_key: Option<String>,
}

#[derive(Clone)]
pub struct GitRepositoryExtraFile {
    pub path: PathBuf,
    pub content: String,
}

pub struct GitRepository {
    pub url: Url,
    pub get_credentials: Option<Box<dyn Fn() -> anyhow::Result<Credentials> + Send + Sync>>,
    pub ssh_keys: Vec<SshKey>,
    pub commit_id: String,
    pub dockerfile_path: Option<PathBuf>,
    pub dockerfile_content: Option<String>,
    pub root_path: PathBuf,
    pub extra_files_to_inject: Vec<GitRepositoryExtraFile>,
    pub docker_target_build_stage: Option<String>,
    pub skip_submodules: bool,
}
impl GitRepository {
    fn credentials(&self) -> Option<anyhow::Result<Credentials>> {
        self.get_credentials.as_ref().map(|f| f())
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Image {
    pub service_id: String,
    pub service_long_id: Uuid,
    pub service_name: String,
    pub name: String,
    pub tag: String,
    pub commit_id: String,
    // registry name where the image has been pushed
    pub registry_name: String,
    // registry docker json config: Optional
    pub registry_docker_json_config: Option<String>,
    // complete registry URL where the image has been pushed
    pub registry_url: Url,
    pub registry_insecure: bool,
    pub repository_name: String,
    pub shared_repository_name: String,
    pub shared_image_feature_enabled: bool,
}

impl Image {
    pub fn registry_host(&self) -> String {
        self.registry_url.host_str().unwrap_or_default().to_string()
    }
    pub fn registry_secret_name(&self) -> String {
        self.registry_host()
    }

    pub fn registry_name(&self) -> String {
        self.registry_name.to_string()
    }

    pub fn repository_name(&self) -> &str {
        match self.shared_image_feature_enabled {
            true => self.shared_repository_name(),
            false => self.legacy_repository_name(),
        }
    }

    pub fn shared_repository_name(&self) -> &str {
        &self.shared_repository_name
    }

    pub fn legacy_repository_name(&self) -> &str {
        &self.repository_name
    }

    pub fn full_image_name_with_tag(&self) -> String {
        match self.registry_url.port_or_known_default() {
            None | Some(443) => {
                format!("{}/{}:{}", self.registry_host(), self.name, self.tag)
            }
            Some(port) => {
                format!("{}:{}/{}:{}", self.registry_host(), port, self.name, self.tag)
            }
        }
    }

    pub fn full_image_name(&self) -> String {
        match self.registry_url.port_or_known_default() {
            None | Some(443) => {
                format!("{}/{}", self.registry_host(), self.name)
            }
            Some(port) => {
                format!("{}:{}/{}", self.registry_host(), port, self.name)
            }
        }
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn name_with_tag(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }

    pub fn name_without_repository(&self) -> &str {
        self.name.split_once('/').map(|(_, name)| name).unwrap_or(&self.name)
    }
}

impl Default for Image {
    fn default() -> Self {
        Image {
            service_id: "".to_string(),
            service_long_id: Default::default(),
            service_name: "".to_string(),
            name: "".to_string(),
            tag: "".to_string(),
            commit_id: "".to_string(),
            registry_name: "".to_string(),
            registry_docker_json_config: None,
            registry_url: Url::parse("https://default.com").unwrap(),
            registry_insecure: false,
            repository_name: "".to_string(),
            shared_repository_name: "".to_string(),
            shared_image_feature_enabled: false,
        }
    }
}

impl Display for Image {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(
            f,
            "Image (name={}, tag={}, commit_id={}, application_id={}, registry_name={:?}, registry_url={:?})",
            self.name, self.tag, self.commit_id, self.service_id, self.registry_name, self.registry_url
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    LocalDocker,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_registries_verified_with_zstd_get_a_zstd_cache() {
        for (kind, expected) in [
            (RegistryKind::Ecr, CacheCompression::Zstd),
            (RegistryKind::AzureContainerRegistry, CacheCompression::Zstd),
            (RegistryKind::ScalewayCr, CacheCompression::Zstd),
            (RegistryKind::GcpArtifactRegistry, CacheCompression::Zstd),
            (RegistryKind::GithubCr, CacheCompression::Zstd),
            (RegistryKind::DockerHub, CacheCompression::Zstd),
            (RegistryKind::GenericCr, CacheCompression::Gzip),
        ] {
            assert_eq!(cache_compression_for_registry(kind), expected, "{kind:?}");
        }
    }

    fn build_with(environment_variables: BTreeMap<String, String>) -> Build {
        Build {
            source: BuildSource::Dockerfile {
                content: "FROM node\nRUN --mount=type=secret,id=A_SECRET npm ci".to_string(),
            },
            image: Image {
                service_id: "my_service_id".to_string(),
                service_long_id: Uuid::nil(),
                service_name: "my-service".to_string(),
                name: "my-service".to_string(),
                tag: String::new(),
                commit_id: String::new(),
                registry_name: "my-registry".to_string(),
                registry_docker_json_config: None,
                registry_url: Url::parse("https://registry.qovery.com").expect("url should be valid"),
                registry_insecure: false,
                repository_name: "my-repository".to_string(),
                shared_repository_name: "my-repository".to_string(),
                shared_image_feature_enabled: false,
            },
            environment_variables,
            disable_buildkit_cache: false,
            timeout: Duration::from_secs(60),
            architectures: vec![CpuArchitecture::AMD64],
            max_cpu_in_milli: 1000,
            max_ram_in_gib: 1,
            ephemeral_storage_in_gib: None,
            registries: vec![],
            dockerfile_fragment: None,
            tag_build_args: None,
        }
    }

    fn image_tag_for(environment_variables: &[(&str, &str)]) -> String {
        let mut build = build_with(
            environment_variables
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        );
        build.compute_image_tag();
        build.image.tag
    }

    /// The whole point of keeping the secret mount ids in `Build::environment_variables`: their
    /// values reach the hashed map, so rotating a secret produces a new tag and therefore a
    /// rebuild, instead of the build being skipped as already-present in the registry.
    #[test]
    fn test_rotating_a_build_variable_changes_the_image_tag() {
        let before = image_tag_for(&[("A_SECRET", "old-value")]);
        let after = image_tag_for(&[("A_SECRET", "new-value")]);

        assert_ne!(before, after);
        assert_eq!(before, image_tag_for(&[("A_SECRET", "old-value")]));
    }

    #[test]
    fn test_dropping_a_build_variable_changes_the_image_tag() {
        assert_ne!(image_tag_for(&[("A_SECRET", "value")]), image_tag_for(&[]));
    }

    fn image_tag_for_narrowed(environment_variables: &[(&str, &str)], tag_build_args: &[&str]) -> String {
        let mut build = build_with(
            environment_variables
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        );
        build.tag_build_args = Some(tag_build_args.iter().map(|name| name.to_string()).collect());
        build.compute_image_tag();
        build.image.tag
    }

    /// The reason `tag_build_args` exists: with it the tag can be computed before the repository is
    /// cloned, and it must equal the tag `build_image_with_docker` computes after narrowing the
    /// variables itself. Any other outcome leaves the early registry check useless.
    #[test]
    fn test_correct_tag_build_args_predict_the_tag_computed_after_the_clone() {
        let predicted =
            image_tag_for_narrowed(&[("A_SECRET", "value"), ("DATABASE_URL", "postgres://x")], &["A_SECRET"]);
        let computed_after_narrowing = image_tag_for(&[("A_SECRET", "value")]);

        assert_eq!(predicted, computed_after_narrowing);
    }

    #[test]
    fn test_a_narrowed_tag_ignores_variables_the_dockerfile_never_reads() {
        let before =
            image_tag_for_narrowed(&[("A_SECRET", "value"), ("DATABASE_URL", "postgres://old")], &["A_SECRET"]);
        let after = image_tag_for_narrowed(&[("A_SECRET", "value"), ("DATABASE_URL", "postgres://new")], &["A_SECRET"]);

        assert_eq!(before, after);
    }

    #[test]
    fn test_a_narrowed_tag_still_changes_when_a_narrowed_variable_is_rotated() {
        let before = image_tag_for_narrowed(&[("A_SECRET", "old-value")], &["A_SECRET"]);
        let after = image_tag_for_narrowed(&[("A_SECRET", "new-value")], &["A_SECRET"]);

        assert_ne!(before, after);
    }

    /// `tag_build_args` disagreeing with the Dockerfile costs a cache miss and nothing else.
    #[test]
    fn test_wrong_tag_build_args_predict_a_tag_that_no_build_pushes() {
        let predicted =
            image_tag_for_narrowed(&[("A_SECRET", "value"), ("DATABASE_URL", "postgres://x")], &["DATABASE_URL"]);
        let computed_after_narrowing = image_tag_for(&[("A_SECRET", "value")]);

        assert_ne!(predicted, computed_after_narrowing);
    }

    /// External secrets are injected into `environment_variables` after the request is parsed, so
    /// a tag computed at parse time describes an env map that no longer exists. Whoever relies on
    /// the tag has to recompute it first.
    #[test]
    fn test_a_variable_injected_after_the_first_tag_changes_it() {
        let mut build = build_with(BTreeMap::new());
        build.tag_build_args = Some(BTreeSet::from(["A_SECRET".to_string()]));
        build.compute_image_tag();
        let before_injection = build.image.tag.clone();

        build
            .environment_variables
            .insert("A_SECRET".to_string(), "resolved-later".to_string());
        build.compute_image_tag();

        assert_ne!(before_injection, build.image.tag);
    }

    #[test]
    fn test_empty_tag_build_args_hash_no_variable() {
        let with_a_value = image_tag_for_narrowed(&[("A_SECRET", "value")], &[]);
        let with_another_value = image_tag_for_narrowed(&[("A_SECRET", "other")], &[]);

        assert_eq!(with_a_value, with_another_value);
    }

    /// `Some(empty)` and `None` are different answers: the core parsed and found no `ARG`, versus
    /// the core could not tell us. Only the second one keeps every variable in the hash.
    #[test]
    fn test_absent_tag_build_args_hash_every_variable() {
        let absent_with_a_value = image_tag_for(&[("A_SECRET", "value")]);
        let absent_with_another_value = image_tag_for(&[("A_SECRET", "other")]);
        let empty_set = image_tag_for_narrowed(&[("A_SECRET", "value")], &[]);

        assert_ne!(absent_with_a_value, absent_with_another_value);
        assert_ne!(absent_with_a_value, empty_set);
    }
}
