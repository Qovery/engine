use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::cmd::docker::DockerError;
use crate::environment::report::logger::EnvLogger;
use crate::errors::EngineError;
use crate::events::EventDetails;

use crate::environment::models::abort::Abort;
use crate::io_models::container::Registry;
use crate::io_models::models::CpuArchitecture;
use crate::metrics_registry::MetricsRegistry;
use crate::utilities::compute_image_tag;
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

pub trait BuildPlatform: Send + Sync {
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn build(
        &self,
        build: &mut Build,
        logger: &EnvLogger,
        metrics_registry: Arc<dyn MetricsRegistry>,
        cancellation_requested: &dyn Abort,
    ) -> Result<(), BuildError>;
}

pub struct Build {
    pub git_repository: GitRepository,
    pub image: Image,
    pub environment_variables: BTreeMap<String, String>,
    pub disable_cache: bool,
    pub timeout: Duration,
    pub architectures: Vec<CpuArchitecture>,
    pub max_cpu_in_milli: u32,
    pub max_ram_in_gib: u32,
    pub ephemeral_storage_in_gib: Option<u32>,
    // registries used by the build where we need to login to pull image
    pub registries: Vec<Registry>,
}

impl Build {
    pub fn compute_image_tag(&mut self) {
        self.image.tag = compute_image_tag(
            &self.git_repository.root_path,
            &self.git_repository.dockerfile_path,
            &self.git_repository.dockerfile_content,
            &self.git_repository.extra_files_to_inject,
            &self.environment_variables,
            &self.git_repository.commit_id,
            &self.git_repository.docker_target_build_stage,
        );
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
    pub fn registry_host(&self) -> &str {
        self.registry_url.host_str().unwrap()
    }
    pub fn registry_secret_name(&self) -> &str {
        self.registry_host()
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
                let host = self.registry_url.host_str().unwrap_or_default();
                format!("{}/{}:{}", host, self.name, self.tag)
            }
            Some(port) => {
                let host = self.registry_url.host_str().unwrap_or_default();
                format!("{}:{}/{}:{}", host, port, self.name, self.tag)
            }
        }
    }

    pub fn full_image_name(&self) -> String {
        match self.registry_url.port_or_known_default() {
            None | Some(443) => {
                let host = self.registry_url.host_str().unwrap_or_default();
                format!("{}/{}", host, self.name)
            }
            Some(port) => {
                let host = self.registry_url.host_str().unwrap_or_default();
                format!("{}:{}/{}", host, port, self.name)
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
