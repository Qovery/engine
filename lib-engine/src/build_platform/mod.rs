use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cmd::command::CommandError;
use crate::cmd::docker::DockerError;
use crate::deployment_report::logger::EnvLogger;
use crate::errors::EngineError;
use crate::events::EventDetails;

use crate::cloud_provider::models::CpuArchitecture;
use crate::utilities::compute_image_tag;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::hash::Hash;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub mod dockerfile_utils;
pub mod local_docker;

#[derive(thiserror::Error, Debug)]
pub enum BuildError {
    #[error("Cannot build Application {application:?} due to an invalid config: {raw_error_message:?}")]
    InvalidConfig {
        application: String,
        raw_error_message: String,
    },

    #[error("Cannot build Application {application:?} due to an error with git: {raw_error:?}")]
    GitError {
        application: String,
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

    #[error("Cannot build Application {application:?} due to an error with buildpack: {raw_error:?}")]
    BuildpackError {
        application: String,
        raw_error: CommandError,
    },
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
        is_task_canceled: &dyn Fn() -> bool,
    ) -> Result<(), BuildError>;
}

pub struct Build {
    pub git_repository: GitRepository,
    pub image: Image,
    pub environment_variables: BTreeMap<String, String>,
    pub disable_cache: bool,
    pub timeout: Duration,
    pub architectures: Vec<CpuArchitecture>,
}

impl Build {
    pub fn compute_image_tag(&mut self) {
        self.image.tag = compute_image_tag(
            &self.git_repository.root_path,
            &self.git_repository.dockerfile_path,
            &self.environment_variables,
            &self.git_repository.commit_id,
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

pub struct GitRepository {
    pub url: Url,
    pub get_credentials: Option<Box<dyn Fn() -> anyhow::Result<Credentials> + Send>>,
    pub ssh_keys: Vec<SshKey>,
    pub commit_id: String,
    pub dockerfile_path: Option<PathBuf>,
    pub root_path: PathBuf,
    pub buildpack_language: Option<String>,
}
impl GitRepository {
    fn credentials(&self) -> Option<anyhow::Result<Credentials>> {
        self.get_credentials.as_ref().map(|f| f())
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Image {
    pub application_id: String,
    pub application_long_id: Uuid,
    pub application_name: String,
    pub name: String,
    pub tag: String,
    pub commit_id: String,
    // registry name where the image has been pushed
    pub registry_name: String,
    // registry docker json config: Optional
    pub registry_docker_json_config: Option<String>,
    // complete registry URL where the image has been pushed
    pub registry_url: Url,
    pub repository_name: String,
}

impl Image {
    pub fn registry_host(&self) -> &str {
        self.registry_url.host_str().unwrap()
    }
    pub fn registry_secret_name(&self, kubernetes_kind: KubernetesKind) -> &str {
        match kubernetes_kind {
            KubernetesKind::Ec2 => "awsecr-cred", // required for registry-creds
            _ => self.registry_host(),
        }
    }
    pub fn repository_name(&self) -> &str {
        &self.repository_name
    }
    pub fn full_image_name_with_tag(&self) -> String {
        format!(
            "{}/{}:{}",
            self.registry_url.host_str().unwrap_or_default(),
            self.name,
            self.tag
        )
    }

    pub fn full_image_name(&self) -> String {
        format!("{}/{}", self.registry_url.host_str().unwrap_or_default(), self.name,)
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn name_with_tag(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }

    pub fn name_without_repository(&self) -> &str {
        self.name
            .strip_prefix(&format!("{}/", self.repository_name()))
            .unwrap_or(&self.name)
    }
}

impl Default for Image {
    fn default() -> Self {
        Image {
            application_id: "".to_string(),
            application_long_id: Default::default(),
            application_name: "".to_string(),
            name: "".to_string(),
            tag: "".to_string(),
            commit_id: "".to_string(),
            registry_name: "".to_string(),
            registry_docker_json_config: None,
            registry_url: Url::parse("https://default.com").unwrap(),
            repository_name: "".to_string(),
        }
    }
}

impl Display for Image {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(
            f,
            "Image (name={}, tag={}, commit_id={}, application_id={}, registry_name={:?}, registry_url={:?})",
            self.name, self.tag, self.commit_id, self.application_id, self.registry_name, self.registry_url
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    LocalDocker,
}
