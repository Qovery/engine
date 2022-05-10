use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::cmd::command::CommandError;
use crate::cmd::docker::{BuildResult, DockerError};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter};
use crate::io_models::{Context, Listen, QoveryIdentifier};
use crate::logger::Logger;
use crate::utilities::compute_image_tag;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::hash::Hash;
use std::path::PathBuf;
use url::Url;

pub mod dockerfile_utils;
pub mod local_docker;

#[derive(thiserror::Error, Debug)]
pub enum BuildError {
    #[error("Cannot build Application {0} due to an invalid config: {1}")]
    InvalidConfig(String, String),

    #[error("Cannot build Application {0} due to an error with git: {1}")]
    GitError(String, git2::Error),

    #[error("Build of Application {0} have been aborted at user request")]
    Aborted(String),

    #[error("Cannot build Application {0} due to an io error: {1} {2}")]
    IoError(String, String, std::io::Error),

    #[error("Cannot build Application {0} due to an error with docker: {1}")]
    DockerError(String, DockerError),

    #[error("Cannot build Application {0} due to an error with buildpack: {1}")]
    BuildpackError(String, CommandError),
}

pub fn to_engine_error(event_details: EventDetails, err: BuildError) -> EngineError {
    match err {
        BuildError::Aborted(_) => EngineError::new_task_cancellation_requested(event_details),
        _ => EngineError::new_build_error(event_details, err),
    }
}

pub trait BuildPlatform: ToTransmitter + Listen {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn build(&self, build: &mut Build, is_task_canceled: &dyn Fn() -> bool) -> Result<BuildResult, BuildError>;
    fn logger(&self) -> Box<dyn Logger>;
    fn get_event_details(&self) -> EventDetails {
        let context = self.context();
        EventDetails::new(
            None,
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            None,
            Stage::Environment(EnvironmentStep::Build),
            self.to_transmitter(),
        )
    }
}

pub struct Build {
    pub git_repository: GitRepository,
    pub image: Image,
    pub environment_variables: BTreeMap<String, String>,
    pub disable_cache: bool,
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
    pub credentials: Option<Credentials>,
    pub ssh_keys: Vec<SshKey>,
    pub commit_id: String,
    pub dockerfile_path: Option<PathBuf>,
    pub root_path: PathBuf,
    pub buildpack_language: Option<String>,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Image {
    pub application_id: String,
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
