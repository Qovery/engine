use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

use crate::environment::models::azure::Credentials;
use crate::environment::models::domain::Domain;
use crate::environment::models::gcp::JsonCredentials;
use crate::environment::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use crate::environment::models::scaleway::{ScwRegion, ScwZone};
use crate::errors::{CommandError, EngineError as IoEngineError, EngineError};
use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
use crate::fs::workspace_directory;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::build_platform::local_docker::LocalDocker;
use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use crate::infrastructure::models::cloud_provider::aws::{AWS, AwsCredentials};
use crate::infrastructure::models::cloud_provider::azure::Azure;
use crate::infrastructure::models::cloud_provider::azure::locations::{AzureLocation, AzureZone};
use crate::infrastructure::models::cloud_provider::gcp::Google;
use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use crate::infrastructure::models::cloud_provider::io::{ClusterAdvancedSettings, CustomerHelmChartsOverrideEncoded};
use crate::infrastructure::models::cloud_provider::scaleway::Scaleway;
use crate::infrastructure::models::cloud_provider::self_managed::SelfManaged;
use crate::infrastructure::models::container_registry::azure_container_registry::AzureContainerRegistry;
use crate::infrastructure::models::container_registry::ecr::ECR;
use crate::infrastructure::models::container_registry::generic_cr::GenericCr;
use crate::infrastructure::models::container_registry::github_cr::{GithubCr, RegistryType};
use crate::infrastructure::models::container_registry::google_artifact_registry::GoogleArtifactRegistry;
use crate::infrastructure::models::container_registry::scaleway_container_registry::ScalewayCR;
use crate::infrastructure::models::dns_provider::cloudflare::Cloudflare;
use crate::infrastructure::models::dns_provider::io::Kind;
use crate::infrastructure::models::dns_provider::qoverydns::QoveryDns;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::infrastructure::models::kubernetes::azure::AksOptions;
use crate::infrastructure::models::kubernetes::azure::node::AzureInstancesType;
use crate::infrastructure::models::kubernetes::azure::node_group::{AzureNodeGroup, AzureNodeGroups};
use crate::infrastructure::models::kubernetes::gcp::GkeOptions;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::Kapsule;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesVersion, event_details};
use crate::infrastructure::models::{build_platform, cloud_provider, container_registry, dns_provider, kubernetes};
use crate::io_models;
use crate::io_models::context::{Context, Features, Metadata};
use crate::io_models::environment::EnvironmentRequest;
use crate::io_models::models::NodeGroups;
use crate::io_models::{Action, QoveryIdentifier};
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use crate::services::azure::container_registry_service::AzureContainerRegistryService;
use crate::services::gcp::artifact_registry_service::ArtifactRegistryService;
use crate::utilities::to_short_id;
use anyhow::{Context as OtherContext, anyhow};
use derivative::Derivative;
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use rusoto_signature::Region;
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
    pub kubernetes: KubernetesDto,
    pub target_environment: T,
    pub metadata: Option<Metadata>,
    pub archive: Option<Archive>,
}

impl<T> EngineRequest<T> {
    pub fn to_infrastructure_context(
        &self,
        context: &Context,
        event_details: EventDetails,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        is_infra_deployment: bool,
    ) -> Result<InfrastructureContext, Box<EngineError>> {
        let build_platform = self
            .build_platform
            .to_engine_build_platform(context, metrics_registry.clone_dyn());
        let cloud_provider = self
            .cloud_provider
            .to_engine_cloud_provider(&self.kubernetes.region, self.kubernetes.kind)
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

        let qovery_tags = HashMap::from([
            ("ClusterId".to_string(), context.cluster_short_id().to_string()),
            ("ClusterLongId".to_string(), context.cluster_long_id().to_string()),
            ("OrganizationId".to_string(), context.organization_short_id().to_string()),
            ("OrganizationLongId".to_string(), context.organization_long_id().to_string()),
            ("Region".to_string(), self.kubernetes.region.clone()),
        ]);
        let mut tags = self
            .kubernetes
            .advanced_settings
            .cloud_provider_container_registry_tags
            .clone();
        tags.extend(qovery_tags);
        if let Some(ttl) = self.kubernetes.advanced_settings.resource_ttl() {
            tags.insert("ttl".to_string(), ttl.as_secs().to_string());
        };

        let container_registry = self
            .container_registry
            .to_engine_container_registry(context.clone(), logger.clone(), tags)
            .map_err(|err| {
                IoEngineError::new_error_on_container_registry_information(
                    event_details.clone(),
                    CommandError::new(
                        "Invalid container registry information".to_string(),
                        Some(format!("Invalid container registry information: {:?}", err)),
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

        let kubernetes = match self.kubernetes.to_engine_kubernetes(
            context,
            cloud_provider.as_ref(),
            &self.cloud_provider.zones,
            logger.clone(),
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
            metrics_registry,
            is_infra_deployment,
        ))
    }

    pub fn is_self_managed(&self) -> bool {
        match self.kubernetes.kind {
            kubernetes::Kind::Eks => false,
            kubernetes::Kind::ScwKapsule => false,
            kubernetes::Kind::Gke => false,
            kubernetes::Kind::Aks => false,
            kubernetes::Kind::EksSelfManaged => true,
            kubernetes::Kind::GkeSelfManaged => true,
            kubernetes::Kind::AksSelfManaged => true,
            kubernetes::Kind::ScwSelfManaged => true,
            kubernetes::Kind::OnPremiseSelfManaged => true,
        }
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
    pub options: CloudProviderOptions,
}

impl BuildPlatform {
    pub fn to_engine_build_platform(
        &self,
        context: &Context,
        metrics_registry: Box<dyn MetricsRegistry>,
    ) -> Box<dyn build_platform::BuildPlatform> {
        Box::new(match self.kind {
            build_platform::Kind::LocalDocker => {
                // FIXME: Remove the unwrap by propagating errors above
                LocalDocker::new(context.clone(), self.long_id, self.name.as_str(), metrics_registry).unwrap()
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
    pub options: CloudProviderOptions,
    pub terraform_state_credentials: TerraformStateCredentials,
}

impl CloudProvider {
    pub fn to_engine_cloud_provider(
        &self,
        region: &str,
        cluster_kind: kubernetes::Kind,
    ) -> Option<Box<dyn cloud_provider::CloudProvider>> {
        let terraform_state_credentials = cloud_provider::TerraformStateCredentials {
            access_key_id: self.terraform_state_credentials.access_key_id.clone(),
            secret_access_key: self.terraform_state_credentials.secret_access_key.clone(),
            region: self.terraform_state_credentials.region.clone(),
            s3_bucket: self.terraform_state_credentials.s3_bucket.clone(),
            dynamodb_table: self.terraform_state_credentials.dynamodb_table.clone(),
        };

        match self.kind {
            cloud_provider::Kind::Aws => {
                let CloudProviderOptions::Aws {
                    access_key_id,
                    secret_access_key,
                    session_token,
                } = &self.options
                else {
                    return None;
                };
                let credentials =
                    AwsCredentials::new(access_key_id.clone(), secret_access_key.clone(), session_token.clone());
                Some(Box::new(AWS::new(
                    self.long_id,
                    credentials,
                    region,
                    self.zones.clone(),
                    cluster_kind,
                    terraform_state_credentials,
                )))
            }
            cloud_provider::Kind::Azure => {
                let CloudProviderOptions::Azure {
                    client_id,
                    client_secret,
                    tenant_id,
                    subscription_id,
                } = &self.options
                else {
                    return None;
                };

                let Ok(region) = AzureLocation::from_str(region) else {
                    return None;
                };

                Some(Box::new(Azure::new(
                    self.long_id,
                    region,
                    Credentials {
                        client_id: client_id.to_string(),
                        client_secret: client_secret.to_string(),
                        tenant_id: tenant_id.to_string(),
                        subscription_id: subscription_id.to_string(),
                    },
                    terraform_state_credentials,
                )))
            }
            cloud_provider::Kind::Scw => {
                let CloudProviderOptions::Scaleway {
                    scaleway_access_key,
                    scaleway_secret_key,
                    scaleway_project_id,
                } = &self.options
                else {
                    return None;
                };
                Some(Box::new(Scaleway::new(
                    self.long_id,
                    scaleway_access_key,
                    scaleway_secret_key,
                    scaleway_project_id,
                    terraform_state_credentials,
                )))
            }
            cloud_provider::Kind::Gcp => {
                let CloudProviderOptions::Gcp { gcp_credentials } = &self.options else {
                    return None;
                };
                let Ok(credentials) = JsonCredentials::try_from(gcp_credentials.clone()) else {
                    return None;
                };
                let Ok(region) = GcpRegion::from_str(region) else {
                    return None;
                };

                Some(Box::new(Google::new(
                    self.long_id,
                    credentials,
                    region,
                    terraform_state_credentials,
                )))
            }
            cloud_provider::Kind::OnPremise => Some(Box::new(SelfManaged::new(self.long_id))),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TerraformStateCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    pub s3_bucket: String,
    #[serde(alias = "dynamo_db_table")]
    pub dynamodb_table: String,
}

pub type ChartValuesOverrideName = String;
pub type ChartValuesOverrideValues = String;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KubernetesConnection {
    pub kubeconfig: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KubernetesDto {
    pub kind: kubernetes::Kind,
    pub long_id: Uuid,
    pub name: String,
    pub version: String,
    pub region: String,
    pub options: Value,
    pub created_at: DateTime<Utc>,
    pub nodes_groups: Vec<NodeGroups>,
    pub advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    pub kubeconfig: Option<String>,
    pub qovery_allowed_public_access_cidrs: Option<Vec<String>>,
}

impl KubernetesDto {
    pub fn to_engine_kubernetes<'a>(
        &self,
        context: &Context,
        cloud_provider: &dyn cloud_provider::CloudProvider,
        zones: &[String],
        logger: Box<dyn Logger>,
    ) -> Result<Box<dyn Kubernetes + 'a>, Box<EngineError>> {
        let event_details = event_details(cloud_provider, *context.cluster_long_id(), self.name.to_string(), context);

        let temp_dir = workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("bootstrap/{}", to_short_id(&self.long_id)),
        )
        .map_err(|err| {
            Box::new(EngineError::new_cannot_get_workspace_directory(
                event_details.clone(),
                CommandError::new("Error creating workspace directory.".to_string(), Some(err.to_string()), None),
            ))
        })?;

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
            kubernetes::Kind::Eks => match EKS::new(
                context.clone(),
                self.long_id,
                self.name.as_str(),
                KubernetesVersion::from_str(&self.version)
                    .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                AwsRegion::from_str(self.region.as_str()).expect("This AWS region is not supported"),
                zones.to_vec(),
                cloud_provider,
                self.created_at,
                serde_json::from_value::<kubernetes::aws::Options>(self.options.clone())
                    .expect("What's wronnnnng -- JSON Options payload is not the expected one"),
                self.nodes_groups.clone(),
                logger,
                self.advanced_settings.clone(),
                decoded_helm_charts_override,
                self.kubeconfig.clone(),
                temp_dir,
                self.qovery_allowed_public_access_cidrs.clone(),
            ) {
                Ok(res) => Ok(Box::new(res)),
                Err(e) => Err(e),
            },
            kubernetes::Kind::ScwKapsule => match Kapsule::new(
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
                self.created_at,
                self.nodes_groups.clone(),
                serde_json::from_value::<kubernetes::scaleway::kapsule::KapsuleOptions>(self.options.clone())
                    .expect("What's wronnnnng -- JSON Options payload for Scaleway is not the expected one"),
                logger,
                self.advanced_settings.clone(),
                decoded_helm_charts_override,
                self.kubeconfig.clone(),
                temp_dir,
            ) {
                Ok(res) => Ok(Box::new(res)),
                Err(e) => Err(e),
            },
            kubernetes::Kind::Gke => {
                let options = serde_json::from_value::<io_models::gke::GkeOptions>(self.options.clone()).map_err(
                    |e: serde_json::Error| {
                        Box::new(EngineError::new_invalid_engine_payload(
                            event_details.clone(),
                            &e.to_string(),
                            None,
                        ))
                    },
                )?;
                let options = GkeOptions::try_from(options).map_err(|e: String| {
                    Box::new(EngineError::new_invalid_engine_payload(event_details.clone(), e.as_str(), None))
                })?;
                match kubernetes::gcp::Gke::new(
                    context.clone(),
                    self.long_id,
                    &self.name,
                    cloud_provider,
                    KubernetesVersion::from_str(&self.version)
                        .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                    GcpRegion::from_str(self.region.as_str()).unwrap_or_else(|_| {
                        panic!(
                            "cannot parse `{}`, it doesn't seem to be a valid GCP region",
                            self.region.as_str()
                        )
                    }),
                    self.created_at,
                    options,
                    logger,
                    self.advanced_settings.clone(),
                    decoded_helm_charts_override,
                    self.kubeconfig.clone(),
                    temp_dir,
                ) {
                    Ok(res) => Ok(Box::new(res)),
                    Err(e) => Err(e),
                }
            }
            kubernetes::Kind::Aks => {
                let options = serde_json::from_value::<io_models::azure::AksOptions>(self.options.clone()).map_err(
                    |e: serde_json::Error| {
                        Box::new(EngineError::new_invalid_engine_payload(
                            event_details.clone(),
                            &e.to_string(),
                            None,
                        ))
                    },
                )?;
                let mut options = AksOptions::try_from(options).map_err(|e: String| {
                    Box::new(EngineError::new_invalid_engine_payload(event_details.clone(), e.as_str(), None))
                })?;

                // TODO(benjaminch): for the time being, resource group name is hardcoded to the cluster name
                // this will be updated once we will let user specify the resource group name
                options.azure_resource_group_name = QoveryIdentifier::new(*context.cluster_long_id())
                    .qovery_resource_name()
                    .to_string();

                match kubernetes::azure::aks::AKS::new(
                    context.clone(),
                    self.long_id,
                    &self.name,
                    KubernetesVersion::from_str(&self.version)
                        .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                    AzureLocation::from_str(self.region.as_str()).unwrap_or_else(|_| {
                        panic!(
                            "cannot parse `{}`, it doesn't seem to be a valid Azure location",
                            self.region.as_str()
                        )
                    }),
                    cloud_provider,
                    self.created_at,
                    options,
                    AzureNodeGroups::new(
                        self.nodes_groups
                            .iter()
                            .map(|ng| {
                                let zone = ng.zone.clone().unwrap_or_default();
                                AzureNodeGroup {
                                    name: ng.name.clone(),
                                    min_nodes: ng.min_nodes,
                                    max_nodes: ng.max_nodes,
                                    instance_type: AzureInstancesType::from_str(&ng.instance_type).unwrap_or_else(
                                        |_| {
                                            panic!(
                                                "cannot parse `{}`, it doesn't seem to be a valid Azure instance type",
                                                &ng.instance_type,
                                            )
                                        },
                                    ),
                                    disk_size_in_gib: ng.disk_size_in_gib,
                                    instance_architecture: ng.instance_architecture,
                                    zone: AzureZone::from_str(&zone).unwrap_or_else(|_| {
                                        panic!("cannot parse `{}`, it doesn't seem to be a valid Azure zone", zone,)
                                    }),
                                }
                            })
                            .collect(),
                    ),
                    logger,
                    self.advanced_settings.clone(),
                    decoded_helm_charts_override,
                    self.kubeconfig.clone(),
                    temp_dir,
                    self.qovery_allowed_public_access_cidrs.clone(),
                ) {
                    Ok(res) => Ok(Box::new(res)),
                    Err(e) => Err(e),
                }
            }
            kubernetes::Kind::OnPremiseSelfManaged
            | kubernetes::Kind::EksSelfManaged
            | kubernetes::Kind::GkeSelfManaged
            | kubernetes::Kind::AksSelfManaged
            | kubernetes::Kind::ScwSelfManaged => {
                match kubernetes::self_managed::on_premise::SelfManaged::new(
                    context.clone(),
                    self.long_id,
                    self.name.to_string(),
                    self.kind,
                    self.region.to_string(),
                    KubernetesVersion::from_str(&self.version)
                        .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                    serde_json::from_value::<kubernetes::self_managed::on_premise::SelfManagedOptions>(
                        self.options.clone(),
                    )
                    .expect("What's wronnnnng -- JSON Options payload is not the expected one"),
                    logger,
                    ClusterAdvancedSettings::default(),
                    self.kubeconfig.clone(),
                    temp_dir,
                ) {
                    Ok(res) => Ok(Box::new(res)),
                    Err(e) => Err(e),
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContainerRegistry {
    Ecr {
        long_id: Uuid,
        name: String,
        options: EcrOptions,
    },
    ScalewayCr {
        long_id: Uuid,
        name: String,
        options: ScwCrOptions,
    },
    GcpArtifactRegistry {
        long_id: Uuid,
        name: String,
        options: GcpCrOptions,
    },
    AzureCr {
        long_id: Uuid,
        name: String,
        options: AzureCrOptions,
    },
    GenericCr {
        long_id: Uuid,
        name: String,
        options: GenericCrOptions,
    },
    GithubCr {
        long_id: Uuid,
        name: String,
        options: GithubCrOptions,
    },
}
impl ContainerRegistry {}

impl ContainerRegistry {
    pub fn to_engine_container_registry(
        &self,
        context: Context,
        logger: Box<dyn Logger>,
        tags: HashMap<String, String>,
    ) -> Result<container_registry::ContainerRegistry, anyhow::Error> {
        match self.clone() {
            ContainerRegistry::Ecr { long_id, name, options } => {
                let credentials =
                    AwsCredentials::new(options.access_key_id, options.secret_access_key, options.session_token);
                Ok(container_registry::ContainerRegistry::Ecr(ECR::new(
                    context,
                    long_id,
                    name.as_str(),
                    credentials,
                    Region::from_str(&options.region)
                        .with_context(|| format!("invalid rusoto region {}", &options.region))?,
                    logger,
                    tags,
                )?))
            }
            ContainerRegistry::ScalewayCr { long_id, name, options } => {
                Ok(container_registry::ContainerRegistry::ScalewayCr(ScalewayCR::new(
                    context,
                    long_id,
                    &name,
                    &options.scaleway_secret_key,
                    &options.scaleway_project_id,
                    ScwRegion::from_str(&options.region).map_err(|_| {
                        anyhow!("cannot parse `{}`, it doesn't seem to be a valid SCW zone", options.region)
                    })?,
                )?))
            }
            ContainerRegistry::GcpArtifactRegistry { long_id, name, options } => {
                let credentials = JsonCredentials::try_from(
                    options
                        .gcp_credentials
                        .clone()
                        .ok_or_else(|| anyhow!("cannot find gcp credentials"))?,
                )
                .map_err(|err| anyhow!("cannot deserialize gcp credentials: {:?}", err))?;

                Ok(container_registry::ContainerRegistry::GcpArtifactRegistry(
                    GoogleArtifactRegistry::new(
                        context,
                        long_id,
                        &name,
                        &credentials.project_id,
                        GcpRegion::from_str(&options.region)
                            .map_err(|err| anyhow!("cannot deserialize gcp region: {:?}", err))?,
                        credentials.clone(),
                        Arc::new(
                            ArtifactRegistryService::new(
                                credentials.clone(),
                                Some(Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32))))),
                                Some(Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32))))),
                            )
                            .with_context(|| "cannot instantiate ArtifactRegistryService")?,
                        ),
                    )?,
                ))
            }
            ContainerRegistry::AzureCr { long_id, name, options } => Ok(
                container_registry::ContainerRegistry::AzureContainerRegistry(AzureContainerRegistry::new(
                    context.clone(),
                    long_id,
                    &name,
                    &options.azure_subscription_id,
                    QoveryIdentifier::new(*context.cluster_long_id()).qovery_resource_name(),
                    &options.client_id,
                    &options.client_secret,
                    options.location.clone(),
                    Arc::new(
                        AzureContainerRegistryService::new(
                            &options.azure_tenant_id,
                            &options.client_id,
                            &options.client_secret,
                            Some(Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32))))),
                            Some(Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32))))),
                        )
                        .with_context(|| "cannot instantiate AzureContainerRegistryService")?,
                    ),
                )?),
            ),
            ContainerRegistry::GenericCr { long_id, name, options } => {
                Ok(container_registry::ContainerRegistry::GenericCr(GenericCr::new(
                    context,
                    long_id,
                    &name,
                    options.url.clone(),
                    options.skip_tls_verify,
                    options.repository_name,
                    options.username.and_then(|l| options.password.map(|p| (l, p))),
                    options.url.host_str().unwrap_or("") != "qovery-registry.lan",
                )?))
            }
            ContainerRegistry::GithubCr { long_id, name, options } => {
                Ok(container_registry::ContainerRegistry::GithubCr(GithubCr::new(
                    context,
                    long_id,
                    &name,
                    options.url,
                    options.username,
                    options.token,
                )?))
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
#[serde(untagged)]
pub enum CloudProviderOptions {
    Aws {
        access_key_id: String,
        #[derivative(Debug = "ignore")]
        secret_access_key: String,
        #[serde(default)]
        session_token: Option<String>,
    },
    Azure {
        client_id: String,
        #[derivative(Debug = "ignore")]
        client_secret: String,
        tenant_id: String,
        subscription_id: String,
    },
    Scaleway {
        scaleway_access_key: String,
        #[derivative(Debug = "ignore")]
        scaleway_secret_key: String,
        scaleway_project_id: String,
    },
    Gcp {
        #[derivative(Debug = "ignore")]
        #[serde(alias = "json_credentials")]
        #[serde(deserialize_with = "gcp_credentials_from")]
        // Allow to deserialize string field to its struct counterpart
        gcp_credentials: JsonCredentialsIo,
    },
    OnPremise {},
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
pub struct EcrOptions {
    access_key_id: String,
    #[derivative(Debug = "ignore")]
    secret_access_key: String,
    #[derivative(Debug = "ignore")]
    #[serde(default)]
    session_token: Option<String>,
    region: String,
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
pub struct ScwCrOptions {
    scaleway_project_id: String,
    #[derivative(Debug = "ignore")]
    pub scaleway_secret_key: String,
    region: String,
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
pub struct AzureCrOptions {
    #[serde(alias = "region")]
    location: AzureLocation,
    azure_subscription_id: String,
    azure_tenant_id: String,
    #[serde(alias = "username")]
    client_id: String,
    #[derivative(Debug = "ignore")]
    #[serde(alias = "password")]
    client_secret: String,
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
pub struct GenericCrOptions {
    pub url: Url,
    pub username: Option<String>,
    #[derivative(Debug = "ignore")]
    pub password: Option<String>,
    pub skip_tls_verify: bool,
    repository_name: String,
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
pub struct GithubCrOptions {
    pub url: Url,
    pub username: String,
    #[derivative(Debug = "ignore")]
    #[serde(alias = "password")]
    pub token: String,
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
pub enum GithubCrRepoType {
    User(String),
    Organization(String),
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
pub struct GcpCrOptions {
    #[derivative(Debug = "ignore")]
    #[serde(alias = "json_credentials")]
    #[serde(deserialize_with = "gcp_credentials_from_str")]
    // Allow to deserialize string field to its struct counterpart
    pub gcp_credentials: Option<JsonCredentialsIo>,
    region: String,
}

/// Allow to properly deserialize JSON credentials from string, making sure to escape \n from keys strings
fn gcp_credentials_from_str<'de, D>(
    deserializer: D,
) -> Result<Option<crate::environment::models::gcp::io::JsonCredentials>, D::Error>
where
    D: Deserializer<'de>,
{
    let gcp_credentials_option: Option<String> = Option::deserialize(deserializer)?;
    match gcp_credentials_option {
        Some(c) => match crate::environment::models::gcp::io::JsonCredentials::try_new_from_json_str(&c) {
            Ok(credentials) => Ok(Some(credentials)),
            Err(e) => Err(de::Error::custom(e.to_string())),
        },
        None => Ok(None),
    }
}

fn gcp_credentials_from<'de, D>(
    deserializer: D,
) -> Result<crate::environment::models::gcp::io::JsonCredentials, D::Error>
where
    D: Deserializer<'de>,
{
    let gcp_credentials = String::deserialize(deserializer)?;
    match crate::environment::models::gcp::io::JsonCredentials::try_new_from_json_str(&gcp_credentials) {
        Ok(credentials) => Ok(credentials),
        Err(e) => Err(de::Error::custom(e.to_string())),
    }
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
#[derivative(Debug)]
pub struct Archive {
    pub upload_url: Url,
}

impl From<GithubCrRepoType> for RegistryType {
    fn from(value: GithubCrRepoType) -> Self {
        match value {
            GithubCrRepoType::User(user) => RegistryType::User(user),
            GithubCrRepoType::Organization(orga) => RegistryType::Organization(orga),
        }
    }
}
