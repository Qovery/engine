use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter};
use crate::git;
use crate::logger::Logger;
use crate::models::{Context, Listen, QoveryIdentifier};
use crate::utilities::get_image_tag;
use git2::{Cred, CredentialType};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::path::Path;

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
    fn has_cache(&self, build: &Build) -> Result<CacheResult, EngineError>;
    fn build(
        &self,
        build: Build,
        force_build: bool,
        is_task_canceled: &dyn Fn() -> bool,
    ) -> Result<BuildResult, EngineError>;
    fn build_error(&self, build: Build) -> Result<BuildResult, EngineError>;
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

impl Build {
    pub fn to_previous_build<P>(&self, clone_repo_into_dir: P) -> Result<Option<Build>, CommandError>
    where
        P: AsRef<Path>,
    {
        let parent_commit_id = git::get_parent_commit_id(
            self.git_repository.url.as_str(),
            self.git_repository.commit_id.as_str(),
            clone_repo_into_dir,
            &|_| match &self.git_repository.credentials {
                None => vec![],
                Some(creds) => vec![(
                    CredentialType::USER_PASS_PLAINTEXT,
                    Cred::userpass_plaintext(creds.login.as_str(), creds.password.as_str()).unwrap(),
                )],
            },
        )
        .map_err(|err| CommandError::new(err.to_string(), Some("Cannot get parent commit ID.".to_string())))?;

        let parent_commit_id = match parent_commit_id {
            None => return Ok(None),
            Some(parent_commit_id) => parent_commit_id,
        };

        let mut environment_variables_map = BTreeMap::<String, String>::new();
        for env in &self.options.environment_variables {
            environment_variables_map.insert(env.key.clone(), env.value.clone());
        }

        let mut image = self.image.clone();
        image.tag = get_image_tag(
            &self.git_repository.root_path,
            &self.git_repository.dockerfile_path,
            &environment_variables_map,
            &parent_commit_id,
        );

        image.commit_id = parent_commit_id.clone();

        Ok(Some(Build {
            git_repository: GitRepository {
                url: self.git_repository.url.clone(),
                credentials: self.git_repository.credentials.clone(),
                ssh_keys: self.git_repository.ssh_keys.clone(),
                commit_id: parent_commit_id,
                dockerfile_path: self.git_repository.dockerfile_path.clone(),
                root_path: self.git_repository.root_path.clone(),
                buildpack_language: self.git_repository.buildpack_language.clone(),
            },
            image,
            options: BuildOptions {
                environment_variables: self.options.environment_variables.clone(),
            },
        }))
    }
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
    pub url: String,
    pub credentials: Option<Credentials>,
    pub ssh_keys: Vec<SshKey>,
    pub commit_id: String,
    pub dockerfile_path: Option<String>,
    pub root_path: String,
    pub buildpack_language: Option<String>,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Image {
    pub application_id: String,
    pub name: String,
    pub tag: String,
    pub commit_id: String,
    // registry name where the image has been pushed: Optional
    pub registry_name: Option<String>,
    // registry docker json config: Optional
    pub registry_docker_json_config: Option<String>,
    // registry secret to pull image: Optional
    pub registry_secret: Option<String>,
    // complete registry URL where the image has been pushed
    pub registry_url: Option<String>,
}

impl Image {
    pub fn name_with_tag(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }
}

impl Default for Image {
    fn default() -> Self {
        Image {
            application_id: "".to_string(),
            name: "".to_string(),
            tag: "".to_string(),
            commit_id: "".to_string(),
            registry_name: None,
            registry_docker_json_config: None,
            registry_secret: None,
            registry_url: None,
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

pub struct BuildResult {
    pub build: Build,
}

impl BuildResult {
    pub fn new(build: Build) -> Self {
        BuildResult { build }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    LocalDocker,
}

type ParentBuild = Build;

pub enum CacheResult {
    MissWithoutParentBuild,
    Miss(ParentBuild),
    Hit,
}
