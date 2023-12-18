use chrono::{DateTime, Utc};
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::build_platform::local_docker::LocalDocker;
use crate::cloud_provider::aws::kubernetes::{ec2::EC2, eks::EKS};
use crate::cloud_provider::aws::regions::AwsRegion;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::gcp::kubernetes::Gke;
use crate::cloud_provider::gcp::locations::GcpRegion;
use crate::cloud_provider::gcp::Google;
use crate::cloud_provider::io::{ClusterAdvancedSettings, CustomerHelmChartsOverrideEncoded};
use crate::cloud_provider::kubernetes::{event_details, KubernetesVersion};
use crate::cloud_provider::models::NodeGroups;
use crate::cloud_provider::scaleway::kubernetes::Kapsule;
use crate::cloud_provider::scaleway::Scaleway;
use crate::container_registry::ecr::ECR;
use crate::container_registry::google_artifact_registry::GoogleArtifactRegistry;
use crate::container_registry::scaleway_container_registry::ScalewayCR;
use crate::dns_provider::cloudflare::Cloudflare;
use crate::dns_provider::io::Kind;
use crate::dns_provider::qoverydns::QoveryDns;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError as IoEngineError, EngineError};
use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
use crate::io_models::context::{Context, Features, Metadata};
use crate::io_models::environment::EnvironmentRequest;
use crate::io_models::{Action, QoveryIdentifier};
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use crate::models::domain::Domain;
use crate::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use crate::models::gcp::JsonCredentials;
use crate::models::scaleway::ScwZone;
use crate::services::gcp::artifact_registry_service::ArtifactRegistryService;
use crate::{build_platform, cloud_provider, container_registry, dns_provider};
use derivative::Derivative;
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

pub type EnvironmentEngineRequest = EngineRequest<EnvironmentRequest>;
pub type InfrastructureEngineRequest = EngineRequest<Option<()>>;

#[derive(Serialize, Deserialize, Clone)]
pub struct EngineRequest<T> {
    pub id: String,
    pub organization_id: String,
    pub organization_long_id: Uuid,
    pub deployment_jwt_token: String,
    pub created_at: DateTime<Utc>,
    pub action: Action,
    pub features: Vec<Features>,
    pub test_cluster: bool,
    pub build_platform: BuildPlatform,
    pub cloud_provider: CloudProvider,
    pub dns_provider: DnsProvider,
    pub container_registry: ContainerRegistry,
    pub kubernetes: Kubernetes,
    pub target_environment: T,
    pub metadata: Option<Metadata>,
    pub archive: Option<Archive>,
}

impl<T> EngineRequest<T> {
    pub fn engine(
        &self,
        context: &Context,
        event_details: EventDetails,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
    ) -> Result<InfrastructureContext, Box<EngineError>> {
        let build_platform = self.build_platform.to_engine_build_platform(context);
        let cloud_provider = self
            .cloud_provider
            .to_engine_cloud_provider(context.clone(), &self.kubernetes.region, self.kubernetes.kind.clone())
            .ok_or_else(|| {
                Box::new(IoEngineError::new_error_on_cloud_provider_information(
                    event_details.clone(),
                    CommandError::new(
                        "Invalid cloud provider information".to_string(),
                        Some(format!("Invalid cloud provider information: {:?}", self.cloud_provider)),
                        None,
                    ),
                ))
            })?;
        let cloud_provider: Arc<dyn cloud_provider::CloudProvider> = Arc::from(cloud_provider);

        let mut tags = self
            .kubernetes
            .advanced_settings
            .cloud_provider_container_registry_tags
            .clone();
        if let Some(ttl) = self.kubernetes.advanced_settings.resource_ttl() {
            tags.insert("ttl".to_string(), ttl.as_secs().to_string());
        };

        let container_registry = self
            .container_registry
            .to_engine_container_registry(context.clone(), logger.clone(), tags)
            .ok_or_else(|| {
                IoEngineError::new_error_on_container_registry_information(
                    event_details.clone(),
                    CommandError::new(
                        "Invalid container registry information".to_string(),
                        Some(format!("Invalid container registry information: {:?}", self.container_registry)),
                        None,
                    ),
                )
            })?;

        let cluster_jwt_token: String = self
            .kubernetes
            .options
            .get("jwt_token")
            .iter()
            .flat_map(|v| v.as_str())
            .collect();

        let dns_provider = self
            .dns_provider
            .to_engine_dns_provider(context.clone(), cluster_jwt_token)
            .ok_or_else(|| {
                IoEngineError::new_error_on_dns_provider_information(
                    event_details,
                    CommandError::new(
                        "Invalid DNS provider information".to_string(),
                        Some(format!("Invalid DNS provider information: {:?}", self.dns_provider)),
                        None,
                    ),
                )
            })?;
        let dns_provider: Arc<dyn dns_provider::DnsProvider> = Arc::from(dns_provider);

        let kubernetes = match self.kubernetes.to_engine_kubernetes(
            context,
            cloud_provider.clone(),
            dns_provider.clone(),
            logger.clone(),
            metrics_registry.clone(),
        ) {
            Ok(x) => x,
            Err(e) => {
                error!("{:?}", e);
                return Err(e);
            }
        };

        Ok(InfrastructureContext::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
        ))
    }
}

impl InfrastructureEngineRequest {
    pub fn event_details(&self) -> EventDetails {
        let kubernetes = &self.kubernetes;
        let stage = match self.action {
            Action::Create => Stage::Infrastructure(InfrastructureStep::Create),
            Action::Pause => Stage::Infrastructure(InfrastructureStep::Pause),
            Action::Delete => Stage::Infrastructure(InfrastructureStep::Delete),
            Action::Restart => Stage::Infrastructure(InfrastructureStep::Restart),
        };

        EventDetails::new(
            Some(self.cloud_provider.kind.clone()),
            QoveryIdentifier::new(self.organization_long_id),
            QoveryIdentifier::new(kubernetes.long_id),
            self.id.to_string(),
            stage,
            Transmitter::Kubernetes(kubernetes.long_id, kubernetes.name.to_string()),
        )
    }
}

impl EnvironmentEngineRequest {
    pub fn event_details(&self) -> EventDetails {
        let kubernetes = &self.kubernetes;
        // It means it is an environment deployment request
        EventDetails::new(
            Some(self.cloud_provider.kind.clone()),
            QoveryIdentifier::new(self.organization_long_id),
            QoveryIdentifier::new(kubernetes.long_id),
            self.id.to_string(),
            Stage::Environment(self.action.to_service_action().to_environment_step()),
            Transmitter::Environment(self.target_environment.long_id, self.target_environment.name.clone()),
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BuildPlatform {
    pub kind: build_platform::Kind,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub options: Options,
}

impl BuildPlatform {
    pub fn to_engine_build_platform(&self, context: &Context) -> Box<dyn build_platform::BuildPlatform> {
        Box::new(match self.kind {
            build_platform::Kind::LocalDocker => {
                // FIXME: Remove the unwrap by propagating errors above
                LocalDocker::new(context.clone(), self.long_id, self.name.as_str()).unwrap()
            }
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CloudProvider {
    pub kind: cloud_provider::Kind,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub zones: Vec<String>,
    pub options: Options,
    pub terraform_state_credentials: TerraformStateCredentials,
}

impl CloudProvider {
    pub fn to_engine_cloud_provider(
        &self,
        context: Context,
        region: &str,
        cluster_kind: cloud_provider::kubernetes::Kind,
    ) -> Option<Box<dyn cloud_provider::CloudProvider>> {
        let terraform_state_credentials = cloud_provider::TerraformStateCredentials {
            access_key_id: self.terraform_state_credentials.access_key_id.clone(),
            secret_access_key: self.terraform_state_credentials.secret_access_key.clone(),
            region: self.terraform_state_credentials.region.clone(),
        };

        match self.kind {
            cloud_provider::Kind::Aws => Some(Box::new(AWS::new(
                context,
                self.long_id,
                self.name.as_str(),
                self.options.access_key_id.as_ref()?.as_str(),
                self.options.secret_access_key.as_ref()?.as_str(),
                region,
                self.zones.clone(),
                cluster_kind,
                terraform_state_credentials,
            ))),
            cloud_provider::Kind::Scw => Some(Box::new(Scaleway::new(
                context,
                self.long_id,
                self.name.as_str(),
                self.options.scaleway_access_key.as_ref()?.as_str(),
                self.options.scaleway_secret_key.as_ref()?.as_str(),
                self.options.scaleway_project_id.as_ref()?.as_str(),
                region,
                terraform_state_credentials,
            ))),
            cloud_provider::Kind::Gcp => {
                let credentials = match &self.options.gcp_credentials {
                    Some(creds) => match JsonCredentials::try_from(creds.clone()) {
                        Ok(c) => c,
                        Err(_e) => return None,
                    },
                    None => return None,
                };
                let region = match GcpRegion::from_str(region) {
                    Ok(r) => r,
                    Err(_e) => return None,
                };
                Some(Box::new(Google::new(
                    context,
                    self.long_id,
                    self.name.as_str(),
                    credentials,
                    region,
                    terraform_state_credentials,
                )))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TerraformStateCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
}

pub type ChartValuesOverrideName = String;
pub type ChartValuesOverrideValues = String;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Kubernetes {
    pub kind: cloud_provider::kubernetes::Kind,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub version: String,
    pub region: String,
    pub options: Value,
    pub nodes_groups: Vec<NodeGroups>,
    pub advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    pub kubeconfig: Option<String>,
}

impl Kubernetes {
    pub fn to_engine_kubernetes<'a>(
        &self,
        context: &Context,
        cloud_provider: Arc<dyn cloud_provider::CloudProvider>,
        dns_provider: Arc<dyn dns_provider::DnsProvider>,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
    ) -> Result<Box<dyn cloud_provider::kubernetes::Kubernetes + 'a>, Box<EngineError>> {
        let event_details = event_details(&*cloud_provider, *context.cluster_long_id(), self.name.to_string(), context);

        let decoded_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>> =
            match &self.customer_helm_charts_override {
                Some(customer_helm_charts_override) => {
                    let mut decoded_customer_helm_charts_override: HashMap<
                        ChartValuesOverrideName,
                        ChartValuesOverrideValues,
                    > = HashMap::new();
                    for (name, values) in customer_helm_charts_override.iter() {
                        decoded_customer_helm_charts_override.insert(
                            name.clone(),
                            CustomerHelmChartsOverrideEncoded::to_decoded_customer_helm_chart_override(values.clone())
                                .map_err(|e| {
                                    Box::new(EngineError::new_base64_decode_issue(
                                        event_details.clone(),
                                        format!("Failed to decode chart override {name}: {:?}", e).as_str(),
                                    ))
                                })?,
                        );
                    }
                    Some(decoded_customer_helm_charts_override)
                }
                None => None,
            };

        match self.kind {
            cloud_provider::kubernetes::Kind::Eks => match EKS::new(
                context.clone(),
                self.id.as_str(),
                self.long_id,
                self.name.as_str(),
                KubernetesVersion::from_str(&self.version)
                    .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                AwsRegion::from_str(self.region.as_str()).expect("This AWS region is not supported"),
                cloud_provider.zones().clone(),
                cloud_provider,
                dns_provider,
                serde_json::from_value::<cloud_provider::aws::kubernetes::Options>(self.options.clone())
                    .expect("What's wronnnnng -- JSON Options payload is not the expected one"),
                self.nodes_groups.clone(),
                logger,
                metrics_registry,
                self.advanced_settings.clone(),
                decoded_helm_charts_override,
            ) {
                Ok(res) => Ok(Box::new(res)),
                Err(e) => Err(e),
            },
            cloud_provider::kubernetes::Kind::ScwKapsule => match Kapsule::new(
                context.clone(),
                self.long_id,
                self.name.clone(),
                KubernetesVersion::from_str(&self.version)
                    .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                ScwZone::from_str(self.region.as_str()).unwrap_or_else(|_| {
                    panic!(
                        "cannot parse `{}`, it doesn't seem to be a valid SCW zone",
                        self.region.as_str()
                    )
                }),
                cloud_provider,
                dns_provider,
                self.nodes_groups.clone(),
                serde_json::from_value::<cloud_provider::scaleway::kubernetes::KapsuleOptions>(self.options.clone())
                    .expect("What's wronnnnng -- JSON Options payload for Scaleway is not the expected one"),
                logger,
                metrics_registry,
                self.advanced_settings.clone(),
                decoded_helm_charts_override,
            ) {
                Ok(res) => Ok(Box::new(res)),
                Err(e) => Err(e),
            },
            cloud_provider::kubernetes::Kind::Ec2 => {
                let ec2_instance = match self.nodes_groups.len() != 1 {
                    true => {
                        return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                            cloud_provider
                                .get_event_details(Stage::Infrastructure(InfrastructureStep::RetrieveClusterResources)),
                            "unknown for EC2 nodegroup".to_string(),
                        )))
                    }
                    false => self.nodes_groups[0].to_ec2_instance(),
                };
                match EC2::new(
                    context.clone(),
                    self.id.as_str(),
                    self.long_id,
                    self.name.as_str(),
                    KubernetesVersion::from_str(&self.version)
                        .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                    AwsRegion::from_str(self.region.as_str()).expect("This AWS region is not supported"),
                    cloud_provider.zones().clone(),
                    cloud_provider,
                    dns_provider,
                    serde_json::from_value::<cloud_provider::aws::kubernetes::Options>(self.options.clone())
                        .expect("What's wronnnnng -- JSON Options payload is not the expected one"),
                    ec2_instance,
                    logger,
                    metrics_registry,
                    self.advanced_settings.clone(),
                    decoded_helm_charts_override,
                ) {
                    Ok(res) => Ok(Box::new(res)),
                    Err(e) => Err(e),
                }
            }
            cloud_provider::kubernetes::Kind::Gke => {
                let options =
                    serde_json::from_value::<cloud_provider::gcp::kubernetes::io::GkeOptions>(self.options.clone())
                        .expect("What's wronnnnng -- JSON Options payload for GCP is not the expected one")
                        .try_into()
                        .map_err(|e: String| {
                            Box::new(EngineError::new_invalid_engine_payload(event_details.clone(), e.as_str(), None))
                        })?;
                match Gke::new(
                    context.clone(),
                    &self.id,
                    self.long_id,
                    &self.name,
                    KubernetesVersion::from_str(&self.version)
                        .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                    GcpRegion::from_str(self.region.as_str()).unwrap_or_else(|_| {
                        panic!(
                            "cannot parse `{}`, it doesn't seem to be a valid GCP region",
                            self.region.as_str()
                        )
                    }),
                    cloud_provider,
                    dns_provider,
                    options,
                    logger,
                    metrics_registry,
                    self.advanced_settings.clone(),
                    decoded_helm_charts_override,
                ) {
                    Ok(res) => Ok(Box::new(res)),
                    Err(e) => Err(e),
                }
            }
            cloud_provider::kubernetes::Kind::EksSelfManaged => todo!(), // TODO: BYOK integration
            cloud_provider::kubernetes::Kind::GkeSelfManaged => todo!(), // TODO: BYOK integration
            cloud_provider::kubernetes::Kind::ScwSelfManaged => todo!(), // TODO: BYOK integration
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContainerRegistry {
    pub kind: container_registry::Kind,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub options: Options,
}

impl ContainerRegistry {
    pub fn to_engine_container_registry(
        &self,
        context: Context,
        logger: Box<dyn Logger>,
        tags: HashMap<String, String>,
    ) -> Option<Box<dyn container_registry::ContainerRegistry>> {
        match self.kind {
            container_registry::Kind::Ecr => Some(Box::new(
                ECR::new(
                    context,
                    self.id.as_str(),
                    self.long_id,
                    self.name.as_str(),
                    self.options.access_key_id.as_ref()?.as_str(),
                    self.options.secret_access_key.as_ref()?.as_str(),
                    self.options.region.as_ref()?.as_str(),
                    logger,
                    tags,
                )
                .ok()?,
            )),
            container_registry::Kind::ScalewayCr => Some(Box::new(
                ScalewayCR::new(
                    context,
                    self.id.as_str(),
                    self.long_id,
                    self.name.as_str(),
                    self.options.scaleway_secret_key.as_ref()?.as_str(),
                    self.options.scaleway_project_id.as_ref()?.as_str(),
                    ScwZone::from_str(self.options.region.as_ref()?.as_str()).unwrap_or_else(|_| {
                        panic!(
                            "cannot parse `{}`, it doesn't seem to be a valid SCW zone",
                            self.options.region.as_deref().unwrap_or_default()
                        )
                    }),
                )
                .ok()?,
            )),
            container_registry::Kind::GcpArtifactRegistry => {
                let credentials = JsonCredentials::try_from(
                    self.options
                        .gcp_credentials
                        .clone()
                        .unwrap_or_else(|| panic!("gcp_credentials not set",)),
                )
                .unwrap_or_else(|_| panic!("Cannot parse gcp_credentials",));

                Some(Box::new(
                    GoogleArtifactRegistry::new(
                        context,
                        self.id.as_str(),
                        self.long_id,
                        self.name.as_str(),
                        credentials.project_id.as_str(),
                        GcpRegion::from_str(self.options.region.as_ref()?.as_str()).unwrap_or_else(|_| {
                            panic!(
                                "cannot parse `{}`, it doesn't seem to be a valid GCP region",
                                self.options.region.as_deref().unwrap_or_default()
                            )
                        }),
                        credentials.clone(),
                        Arc::new(
                            ArtifactRegistryService::new(
                                credentials.clone(),
                                Some(Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32))))),
                                Some(Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32))))),
                            )
                            .unwrap_or_else(|_| panic!("cannot instantiate ArtifactRegistryService",)),
                        ),
                    )
                    .ok()?,
                ))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DnsProvider {
    pub kind: Kind,
    pub long_id: Uuid,
    pub name: String,
    pub domain: String,
    pub options: HashMap<String, String>,
}

impl DnsProvider {
    pub fn to_engine_dns_provider(
        &self,
        context: Context,
        cluster_jwt_token: String,
    ) -> Option<Box<dyn dns_provider::DnsProvider>> {
        match self.kind {
            Kind::Cloudflare => {
                let token = self.options.get("cloudflare_api_token")?;
                let email = self.options.get("cloudflare_email")?;
                let proxied: bool = self
                    .options
                    .get("cloudflare_proxied")
                    .map(|s| s.parse::<bool>().unwrap_or(false))
                    .unwrap_or(false);

                Some(Box::new(Cloudflare::new(
                    context,
                    self.long_id,
                    self.name.as_str(),
                    Domain::new(self.domain.clone()),
                    token.as_str(),
                    email.as_str(),
                    proxied,
                )))
            }
            Kind::QoveryDns => {
                let qoverydns_api_url = self.options.get("qoverydns_api_url")?;

                if let Ok(api_url) = Url::parse(qoverydns_api_url) {
                    return Some(Box::new(QoveryDns::new(
                        context,
                        self.long_id,
                        api_url,
                        &cluster_jwt_token,
                        self.name.as_str(),
                        Domain::new(self.domain.clone()),
                    )));
                }

                None
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
#[derivative(Debug)]
pub struct Options {
    // TODO(benjaminch): Refactor this struct properly, each providers might have their own options
    login: Option<String>,
    #[derivative(Debug = "ignore")]
    password: Option<String>,
    access_key_id: Option<String>,
    #[derivative(Debug = "ignore")]
    secret_access_key: Option<String>,
    spaces_access_id: Option<String>,
    #[derivative(Debug = "ignore")]
    spaces_secret_key: Option<String>,
    scaleway_project_id: Option<String>,
    scaleway_access_key: Option<String>,
    #[derivative(Debug = "ignore")]
    scaleway_secret_key: Option<String>,
    #[derivative(Debug = "ignore")]
    #[serde(alias = "json_credentials")]
    #[serde(deserialize_with = "gcp_credentials_from_str")] // Allow to deserialize string field to its struct counterpart
    #[serde(default)]
    gcp_credentials: Option<JsonCredentialsIo>,
    #[derivative(Debug = "ignore")]
    token: Option<String>,
    region: Option<String>,
}

/// Allow to properly deserialize JSON credentials from string, making sure to escape \n from keys strings
fn gcp_credentials_from_str<'de, D>(
    deserializer: D,
) -> Result<Option<crate::models::gcp::io::JsonCredentials>, D::Error>
where
    D: Deserializer<'de>,
{
    let gcp_credentials_option: Option<String> = Option::deserialize(deserializer)?;
    match gcp_credentials_option {
        Some(c) => match crate::models::gcp::io::JsonCredentials::try_new_from_json_str(&c) {
            Ok(credentials) => Ok(Some(credentials)),
            Err(e) => Err(de::Error::custom(e.to_string())),
        },
        None => Ok(None),
    }
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
#[derivative(Debug)]
pub struct Archive {
    pub bucket_name: String,
    pub access_key_id: String,
    #[derivative(Debug = "ignore")]
    pub secret_access_key: String,
}
