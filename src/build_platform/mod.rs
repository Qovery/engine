use serde::{Deserialize, Serialize};

use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter};
use crate::logger::Logger;
use crate::models::{Context, Listen, QoveryIdentifier};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::path::PathBuf;
use url::Url;

pub mod docker;
pub mod local_docker;

pub trait BuildPlatform: ToTransmitter + Listen {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), EngineError>;
    fn build(&self, build: &Build, is_task_canceled: &dyn Fn() -> bool) -> Result<(), EngineError>;
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
    pub options: BuildOptions,
}

pub struct BuildOptions {
    pub environment_variables: Vec<EnvironmentVariable>,
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
    // registry name where the image has been pushed: Optional
    pub registry_name: String,
    // registry docker json config: Optional
    pub registry_docker_json_config: Option<String>,
    // complete registry URL where the image has been pushed
    pub registry_url: Url,
}

impl Image {
    pub fn registry_host(&self) -> &str {
        self.registry_url.host_str().unwrap()
    }

    pub fn repository_name(&self) -> &str {
        self.name.split('/').collect::<Vec<&str>>()[0]
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
