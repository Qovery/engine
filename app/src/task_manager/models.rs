use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::docr::DOCR;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::container_registry::scaleway_container_registry::ScalewayCR;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::engine::Engine;
use qovery_engine::error::EngineError;
use qovery_engine::logger::Logger;
use qovery_engine::models::{Context, Domain, Environment, EnvironmentAction, Features, Listener, Metadata};
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Serialize, Deserialize, Clone)]
pub struct EngineRequest {
    pub id: String,
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub created_at: DateTime<Utc>,
    pub action: Action,
    pub features: Vec<Features>,
    pub test_cluster: bool,
    pub build_platform: BuildPlatform,
    pub cloud_provider: CloudProvider,
    pub dns_provider: DnsProvider,
    pub container_registry: ContainerRegistry,
    pub target_environment: Option<Environment>,
    pub failover_environment: Option<Environment>,
    pub metadata: Option<Metadata>,
    pub archive: Option<Archive>,
    // this field is used to store the data bytes from the current request send through NATS.
    #[serde(skip_serializing, skip_deserializing)]
    pub bytes_payload: Vec<u8>,
}

#[derive(Debug)]
pub enum RequestError {
    CloudProvider(String),
    ContainerRegistry(String),
    DnsProvider(String),
}

impl EngineRequest {
    pub fn engine(
        &self,
        context: &Context,
        progress_listener: Listener,
        logger: Box<dyn Logger>,
        is_task_canceled: Box<dyn Fn() -> bool>,
    ) -> Result<Engine, RequestError> {
        let mut build_platform = self.build_platform.to_engine_build_platform(&context, logger.clone());
        build_platform.add_listener(progress_listener.clone());

        let mut cloud_provider = self
            .cloud_provider
            .to_engine_cloud_provider(
                context.clone(),
                self.organization_id.as_str(),
                self.organization_long_id,
            )
            .ok_or_else(|| {
                RequestError::CloudProvider(format!("Invalid cloud provider info: {:?}", self.cloud_provider))
            })?;

        cloud_provider.add_listener(progress_listener.clone());

        let mut container_registry = self
            .container_registry
            .to_engine_container_registry(context.clone())
            .ok_or_else(|| {
                RequestError::ContainerRegistry(format!(
                    "Invalid container registry info: {:?}",
                    self.container_registry
                ))
            })?;

        container_registry.add_listener(progress_listener.clone());

        let dns_provider = self
            .dns_provider
            .to_engine_dns_provider(context.clone())
            .ok_or_else(|| RequestError::DnsProvider(format!("Invalid DNS provider: {:?}", self.dns_provider)))?;

        Ok(Engine::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            logger,
            is_task_canceled,
        ))
    }

    pub fn environment_action(&self) -> Option<EnvironmentAction> {
        self.target_environment.as_ref()?;
        let environment = self.target_environment.as_ref().unwrap().clone();
        Some(EnvironmentAction::Environment(environment))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Action {
    Create,
    Pause,
    Delete,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BuildPlatform {
    pub kind: qovery_engine::build_platform::Kind,
    pub id: String,
    pub name: String,
    pub options: Options,
}

impl BuildPlatform {
    pub fn to_engine_build_platform(
        &self,
        context: &Context,
        logger: Box<dyn Logger>,
    ) -> Box<dyn qovery_engine::build_platform::BuildPlatform> {
        Box::new(match self.kind {
            qovery_engine::build_platform::Kind::LocalDocker => {
                LocalDocker::new(context.clone(), self.id.as_str(), self.name.as_str(), logger)
            }
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CloudProvider {
    pub kind: qovery_engine::cloud_provider::Kind,
    pub id: String,
    pub name: String,
    pub zones: Vec<String>,
    pub options: Options,
    pub kubernetes: Kubernetes,
    pub terraform_state_credentials: TerraformStateCredentials,
}

impl CloudProvider {
    pub fn to_engine_cloud_provider(
        &self,
        context: Context,
        organization_id: &str,
        organization_long_id: uuid::Uuid,
    ) -> Option<Box<dyn qovery_engine::cloud_provider::CloudProvider>> {
        let terraform_state_credentials = qovery_engine::cloud_provider::TerraformStateCredentials {
            access_key_id: self.terraform_state_credentials.access_key_id.clone(),
            secret_access_key: self.terraform_state_credentials.secret_access_key.clone(),
            region: self.terraform_state_credentials.region.clone(),
        };

        match self.kind {
            qovery_engine::cloud_provider::Kind::Aws => Some(Box::new(AWS::new(
                context,
                self.id.as_str(),
                organization_id,
                organization_long_id,
                self.name.as_str(),
                self.options.access_key_id.as_ref()?.as_str(),
                self.options.secret_access_key.as_ref()?.as_str(),
                self.zones.clone(),
                terraform_state_credentials,
            ))),
            qovery_engine::cloud_provider::Kind::Do => Some(Box::new(DO::new(
                context,
                self.id.as_str(),
                organization_id,
                organization_long_id,
                self.options.token.as_ref()?.as_str(),
                self.options.spaces_access_id.as_ref()?.as_str(),
                self.options.spaces_secret_key.as_ref()?.as_str(),
                self.name.as_str(),
                terraform_state_credentials,
            ))),
            qovery_engine::cloud_provider::Kind::Scw => Some(Box::new(Scaleway::new(
                context,
                self.id.as_str(),
                organization_id,
                organization_long_id,
                self.name.as_str(),
                self.options.scaleway_access_key.as_ref()?.as_str(),
                self.options.scaleway_secret_key.as_ref()?.as_str(),
                self.options.scaleway_project_id.as_ref()?.as_str(),
                terraform_state_credentials,
            ))),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TerraformStateCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Kubernetes {
    pub kind: qovery_engine::cloud_provider::kubernetes::Kind,
    pub id: String,
    pub long_id: uuid::Uuid,
    pub name: String,
    pub version: String,
    pub region: String,
    pub options: Value,
    pub nodes_groups: Vec<NodeGroups>,
}

impl Kubernetes {
    pub fn to_engine_kubernetes<'a>(
        &self,
        context: &Context,
        cloud_provider: &'a dyn qovery_engine::cloud_provider::CloudProvider,
        dns_provider: &'a dyn qovery_engine::dns_provider::DnsProvider,
        logger: &'a dyn qovery_engine::logger::Logger,
    ) -> Result<Box<dyn qovery_engine::cloud_provider::kubernetes::Kubernetes + 'a>, EngineError> {
        match self.kind {
            qovery_engine::cloud_provider::kubernetes::Kind::Eks => match EKS::new(
                context.clone(),
                self.id.as_str(),
                self.long_id,
                self.name.as_str(),
                self.version.as_str(),
                AwsRegion::from_str(self.region.as_str()).expect("This AWS region is not supported"),
                cloud_provider.zones().clone(),
                cloud_provider.as_any().downcast_ref::<AWS>().unwrap(),
                dns_provider,
                serde_json::from_value::<qovery_engine::cloud_provider::aws::kubernetes::Options>(self.options.clone())
                    .expect("What's wronnnnng -- JSON Options payload is not the expected one"),
                self.nodes_groups.clone(),
                logger,
            ) {
                Ok(res) => Ok(Box::new(res)),
                Err(e) => Err(e.to_legacy_engine_error()),
            },
            qovery_engine::cloud_provider::kubernetes::Kind::Doks => match DOKS::new(
                context.clone(),
                self.id.clone(),
                self.long_id,
                self.name.clone(),
                self.version.clone(),
                qovery_engine::cloud_provider::digitalocean::application::DoRegion::from_str(self.region.as_str())
                    .unwrap(),
                cloud_provider.as_any().downcast_ref::<DO>().unwrap(),
                dns_provider,
                self.nodes_groups.clone(),
                serde_json::from_value::<qovery_engine::cloud_provider::digitalocean::kubernetes::DoksOptions>(
                    self.options.clone(),
                )
                .expect("What's wronnnnng -- JSON Options for digital ocean DOKS payload is not the expected one"),
                logger,
            ) {
                Ok(res) => Ok(Box::new(res)),
                Err(e) => Err(e.to_legacy_engine_error()),
            },
            qovery_engine::cloud_provider::kubernetes::Kind::ScwKapsule => match Kapsule::new(
                context.clone(),
                self.id.clone(),
                self.long_id,
                self.name.clone(),
                self.version.clone(),
                qovery_engine::cloud_provider::scaleway::application::ScwZone::from_str(self.region.as_str()).expect(
                    format!(
                        "cannot parse `{}`, it doesn't seem to be a valid SCW zone",
                        self.region.as_str()
                    )
                    .as_str(),
                ),
                cloud_provider.as_any().downcast_ref::<Scaleway>().unwrap(),
                dns_provider,
                self.nodes_groups.clone(),
                serde_json::from_value::<qovery_engine::cloud_provider::scaleway::kubernetes::KapsuleOptions>(
                    self.options.clone(),
                )
                .expect("What's wronnnnng -- JSON Options payload for Scaleway is not the expected one"),
                logger,
            ) {
                Ok(res) => Ok(Box::new(res)),
                Err(e) => Err(e.to_legacy_engine_error()),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContainerRegistry {
    pub kind: qovery_engine::container_registry::Kind,
    pub id: String,
    pub name: String,
    pub options: Options,
}

impl ContainerRegistry {
    pub fn to_engine_container_registry(
        &self,
        context: Context,
    ) -> Option<Box<dyn qovery_engine::container_registry::ContainerRegistry>> {
        match self.kind {
            qovery_engine::container_registry::Kind::DockerHub => Some(Box::new(DockerHub::new(
                context,
                self.id.as_str(),
                self.name.as_str(),
                self.options.login.as_ref()?.as_str(),
                self.options.password.as_ref()?.as_str(),
            ))),
            qovery_engine::container_registry::Kind::Ecr => Some(Box::new(ECR::new(
                context,
                self.id.as_str(),
                self.name.as_str(),
                self.options.access_key_id.as_ref()?.as_str(),
                self.options.secret_access_key.as_ref()?.as_str(),
                self.options.region.as_ref()?.as_str(),
            ))),
            qovery_engine::container_registry::Kind::Docr => Some(Box::new(DOCR::new(
                context,
                self.id.as_str(),
                self.name.as_str(),
                self.options.token.as_ref()?.as_str(),
            ))),
            qovery_engine::container_registry::Kind::ScalewayCr => Some(Box::new(ScalewayCR::new(
                context,
                self.id.as_str(),
                self.name.as_str(),
                self.options.scaleway_secret_key.as_ref()?.as_str(),
                self.options.scaleway_project_id.as_ref()?.as_str(),
                qovery_engine::cloud_provider::scaleway::application::ScwZone::from_str(
                    self.options.region.as_ref()?.as_str(),
                )
                .expect(
                    format!(
                        "cannot parse `{}`, it doesn't seem to be a valid SCW zone",
                        self.options.region.as_ref()?.as_str(),
                    )
                    .as_str(),
                ),
            ))),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DnsProvider {
    pub kind: qovery_engine::dns_provider::Kind,
    pub id: String,
    pub name: String,
    pub domain: String,
    pub options: HashMap<String, String>,
}

impl DnsProvider {
    pub fn to_engine_dns_provider(
        &self,
        context: Context,
    ) -> Option<Box<dyn qovery_engine::dns_provider::DnsProvider>> {
        match self.kind {
            qovery_engine::dns_provider::Kind::Cloudflare => {
                let token = self.options.get("cloudflare_api_token")?;
                let email = self.options.get("cloudflare_email")?;

                Some(Box::new(Cloudflare::new(
                    context,
                    self.id.as_str(),
                    self.name.as_str(),
                    Domain::new(self.domain.clone()),
                    token.as_str(),
                    email.as_str(),
                )))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Options {
    // TODO(benjaminch): Refactor this struct properly, each providers might have their own options
    login: Option<String>,
    password: Option<String>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    spaces_access_id: Option<String>,
    spaces_secret_key: Option<String>,
    scaleway_project_id: Option<String>,
    scaleway_access_key: Option<String>,
    scaleway_secret_key: Option<String>,
    token: Option<String>,
    region: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Archive {
    pub bucket_name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}
