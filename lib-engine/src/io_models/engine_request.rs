use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

use crate::environment::models::azure::Credentials;
use crate::environment::models::domain::Domain;
use crate::environment::models::gcp::JsonCredentials;
use crate::environment::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use crate::environment::models::scaleway::{ScwRegion, ScwZone};
use crate::errors::{CommandError, EngineError as IoEngineError, EngineError};
use crate::events::{BlueprintStep, EventDetails, InfrastructureStep, Stage, Transmitter};
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
use crate::infrastructure::models::dns_provider::route53::Route53;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::infrastructure::models::kubernetes::azure::AksOptions;
use crate::infrastructure::models::kubernetes::azure::node::AzureInstancesType;
use crate::infrastructure::models::kubernetes::azure::node_group::{AzureNodeGroup, AzureNodeGroups};
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhereOptions;
use crate::infrastructure::models::kubernetes::gcp::GkeOptions;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::Kapsule;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesVersion, event_details};
use crate::infrastructure::models::{build_platform, cloud_provider, container_registry, dns_provider, kubernetes};
use crate::io_models;
use crate::io_models::blueprint::BlueprintRequest;
use crate::io_models::context::{Context, Features, Metadata};
use crate::io_models::environment::EnvironmentRequest;
use crate::io_models::models::NodeGroups;
use crate::io_models::{Action, QoveryIdentifier};
use crate::log_utils::send_progress_on_long_task_with_message;
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use crate::services::azure::azure_auth_service::AzureAuthService;
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
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub type EnvironmentEngineRequest = EngineRequest<EnvironmentRequest>;
pub type BlueprintEngineRequest = EngineRequest<BlueprintRequest>;
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
            .to_engine_container_registry(context.clone(), logger.clone(), event_details.clone(), tags)
            .map_err(|err| {
                IoEngineError::new_error_on_container_registry_information(
                    event_details.clone(),
                    CommandError::new(
                        "Invalid container registry information".to_string(),
                        Some(format!("Invalid container registry information: {err}")),
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
            kubernetes::Kind::EksAnywhere => true,
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

impl BlueprintEngineRequest {
    pub fn event_details(&self) -> EventDetails {
        EventDetails::new(
            Some(self.cloud_provider.kind.clone()),
            QoveryIdentifier::new(self.organization_long_id),
            QoveryIdentifier::new(self.kubernetes.long_id),
            self.id.to_string(),
            Stage::Blueprint(BlueprintStep::from(cloud_provider::service::Action::from(self.action))),
            Transmitter::Environment(self.target_environment.long_id, self.target_environment.name.clone()),
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
            Stage::Environment(cloud_provider::service::Action::from(self.action).to_environment_step()),
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
    pub options: Value,
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

#[derive(Serialize, Clone, Debug)]
pub struct CloudProvider {
    pub kind: cloud_provider::Kind,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub zones: Vec<String>,
    pub options: CloudProviderOptions,
    pub terraform_state_credentials: TerraformStateCredentials,
}

#[derive(Deserialize)]
struct CloudProviderWire {
    kind: cloud_provider::Kind,
    id: String,
    long_id: Uuid,
    name: String,
    zones: Vec<String>,
    options: Value,
    terraform_state_credentials: TerraformStateCredentials,
}

impl<'de> Deserialize<'de> for CloudProvider {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CloudProviderWire::deserialize(deserializer)?;
        let options =
            deserialize_cloud_provider_options_by_kind(wire.kind.clone(), wire.options).map_err(de::Error::custom)?;

        Ok(Self {
            kind: wire.kind,
            id: wire.id,
            long_id: wire.long_id,
            name: wire.name,
            zones: wire.zones,
            options,
            terraform_state_credentials: wire.terraform_state_credentials,
        })
    }
}

fn deserialize_cloud_provider_options_by_kind(
    kind: cloud_provider::Kind,
    options: Value,
) -> Result<CloudProviderOptions, String> {
    match kind {
        cloud_provider::Kind::OnPremise => deserialize_onprem_options(options),
        cloud_provider::Kind::Aws => {
            let parsed = serde_json::from_value::<CloudProviderOptions>(options)
                .map_err(|e| format!("Cannot deserialize AWS cloud provider options: {e}"))?;
            match parsed {
                CloudProviderOptions::Aws { .. } | CloudProviderOptions::AwsVsphere { .. } => Ok(parsed),
                _ => Err("Invalid AWS cloud provider options payload".to_string()),
            }
        }
        cloud_provider::Kind::Azure => {
            let parsed = serde_json::from_value::<CloudProviderOptions>(options)
                .map_err(|e| format!("Cannot deserialize Azure cloud provider options: {e}"))?;
            match parsed {
                CloudProviderOptions::Azure { .. } => Ok(parsed),
                _ => Err("Invalid Azure cloud provider options payload".to_string()),
            }
        }
        cloud_provider::Kind::Scw => {
            let parsed = serde_json::from_value::<CloudProviderOptions>(options)
                .map_err(|e| format!("Cannot deserialize Scaleway cloud provider options: {e}"))?;
            match parsed {
                CloudProviderOptions::Scaleway { .. } => Ok(parsed),
                _ => Err("Invalid Scaleway cloud provider options payload".to_string()),
            }
        }
        cloud_provider::Kind::Gcp => {
            let parsed = serde_json::from_value::<CloudProviderOptions>(options)
                .map_err(|e| format!("Cannot deserialize GCP cloud provider options: {e}"))?;
            match parsed {
                CloudProviderOptions::Gcp { .. } => Ok(parsed),
                _ => Err("Invalid GCP cloud provider options payload".to_string()),
            }
        }
    }
}

fn deserialize_onprem_options(options: Value) -> Result<CloudProviderOptions, String> {
    let Some(options_object) = options.as_object() else {
        return Err("Invalid OnPremise cloud provider options payload: expected an object".to_string());
    };

    let vsphere_user = option_string_alias(options_object, &["vsphere_user", "vsphere_username"]);
    let vsphere_password = option_string_alias(options_object, &["vsphere_password"]);
    let access_key_id = option_string_alias(options_object, &["access_key_id", "aws_access_key_id"]);
    let secret_access_key = option_string_alias(options_object, &["secret_access_key", "aws_secret_access_key"]);
    let session_token = option_string_alias(options_object, &["session_token", "aws_session_token"]);

    if let (Some(vsphere_user), Some(vsphere_password)) = (vsphere_user.clone(), vsphere_password.clone()) {
        return Ok(CloudProviderOptions::AwsVsphere {
            access_key_id,
            secret_access_key,
            session_token,
            vsphere_user,
            vsphere_password,
        });
    }

    if let (Some(access_key_id), Some(secret_access_key)) = (access_key_id, secret_access_key) {
        return Ok(CloudProviderOptions::Aws {
            access_key_id,
            secret_access_key,
            session_token,
            vsphere_user,
            vsphere_password,
        });
    }

    Ok(CloudProviderOptions::OnPremise(OnPremiseOptions {}))
}

fn option_string_alias(options: &serde_json::Map<String, Value>, aliases: &[&str]) -> Option<String> {
    aliases.iter().find_map(|alias| {
        options
            .get(*alias)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
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
                let (access_key_id, secret_access_key, session_token, vsphere_user, vsphere_password) =
                    match &self.options {
                        CloudProviderOptions::Aws {
                            access_key_id,
                            secret_access_key,
                            session_token,
                            vsphere_user,
                            vsphere_password,
                        } => (
                            access_key_id.clone(),
                            secret_access_key.clone(),
                            session_token.clone(),
                            vsphere_user.clone(),
                            vsphere_password.clone(),
                        ),
                        CloudProviderOptions::AwsVsphere {
                            access_key_id,
                            secret_access_key,
                            session_token,
                            vsphere_user,
                            vsphere_password,
                        } => (
                            access_key_id
                                .clone()
                                .filter(|value| !value.trim().is_empty())
                                .unwrap_or_else(|| self.terraform_state_credentials.access_key_id.clone()),
                            secret_access_key
                                .clone()
                                .filter(|value| !value.trim().is_empty())
                                .unwrap_or_else(|| self.terraform_state_credentials.secret_access_key.clone()),
                            session_token.clone(),
                            Some(vsphere_user.clone()),
                            Some(vsphere_password.clone()),
                        ),
                        _ => return None,
                    };
                let credentials = AwsCredentials::new(access_key_id, secret_access_key, session_token);
                Some(Box::new(AWS::new(
                    self.long_id,
                    credentials,
                    region,
                    self.zones.clone(),
                    vsphere_user,
                    vsphere_password,
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
            cloud_provider::Kind::OnPremise => {
                let (vsphere_user, vsphere_password) = match &self.options {
                    CloudProviderOptions::Aws {
                        vsphere_user,
                        vsphere_password,
                        ..
                    } => (vsphere_user.clone(), vsphere_password.clone()),
                    CloudProviderOptions::AwsVsphere {
                        vsphere_user,
                        vsphere_password,
                        ..
                    } => (Some(vsphere_user.clone()), Some(vsphere_password.clone())),
                    CloudProviderOptions::OnPremise(_) => (None, None),
                    _ => return None,
                };

                Some(Box::new(SelfManaged::new(self.long_id, vsphere_user, vsphere_password)))
            }
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
                                        format!("Failed to decode chart override {name}: {e:?}").as_str(),
                                    ))
                                })?,
                        );
                    }
                    Some(decoded_customer_helm_charts_override)
                }
                None => None,
            };

        match self.kind {
            kubernetes::Kind::Eks => {
                let options = serde_json::from_value::<kubernetes::aws::Options>(self.options.clone())
                    .expect("What's wronnnnng -- JSON Options payload is not the expected one");

                match EKS::new(
                    context.clone(),
                    self.long_id,
                    self.name.as_str(),
                    KubernetesVersion::from_str(&self.version)
                        .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                    AwsRegion::from_str(self.region.as_str()).expect("This AWS region is not supported"),
                    zones.to_vec(),
                    cloud_provider,
                    self.created_at,
                    options.clone(),
                    self.nodes_groups.clone(),
                    logger,
                    self.advanced_settings.clone(),
                    decoded_helm_charts_override,
                    self.kubeconfig.clone(),
                    temp_dir,
                    self.qovery_allowed_public_access_cidrs.clone(),
                    options.resource_tags.clone(),
                ) {
                    Ok(res) => Ok(Box::new(res)),
                    Err(e) => Err(e),
                }
            }
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
                    self.qovery_allowed_public_access_cidrs.clone(),
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
                                        panic!("cannot parse `{zone}`, it doesn't seem to be a valid Azure zone",)
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
            kubernetes::Kind::EksAnywhere => {
                let kubeconfig = match self.kubeconfig.clone() {
                    None => return Err(Box::new(EngineError::new_missing_kubeconfig_error(event_details.clone()))),
                    Some(value) => value,
                };
                let options = serde_json::from_value::<EksAnywhereOptions>(self.options.clone()).map_err(
                    |e: serde_json::Error| {
                        Box::new(EngineError::new_invalid_engine_payload(
                            event_details.clone(),
                            &e.to_string(),
                            None,
                        ))
                    },
                )?;
                match kubernetes::eksanywhere::EksAnywhere::new(
                    context.clone(),
                    self.long_id,
                    self.name.to_string(),
                    cloud_provider,
                    self.kind,
                    self.region.to_string(),
                    KubernetesVersion::from_str(&self.version)
                        .unwrap_or_else(|_| panic!("Kubernetes version `{}` is not supported", &self.version)),
                    options,
                    logger,
                    self.advanced_settings.clone(),
                    kubeconfig,
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
impl ContainerRegistry {
    pub fn to_engine_container_registry(
        &self,
        context: Context,
        logger: Box<dyn Logger>,
        event_details: EventDetails,
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
            ContainerRegistry::AzureCr { long_id, name, options } => {
                // check credentials
                // azure credentials propagation can take some time, so we need to ensure that the credentials are valid before proceeding
                send_progress_on_long_task_with_message(
                    logger,
                    event_details.clone(),
                    Some("Checking Azure credentials, those can take some time to propagate...".to_string()),
                    || {
                        AzureAuthService::login_with_retry(
                            &options.client_id,
                            &options.client_secret,
                            &options.azure_tenant_id,
                        )
                    },
                    Duration::from_secs(10),
                    Some(Duration::from_secs(60 * 10)), // 10 minutes max
                )?;

                Ok(container_registry::ContainerRegistry::AzureContainerRegistry(
                    AzureContainerRegistry::new(
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
                    )?,
                ))
            }
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
            Kind::Route53 => {
                let aws_access_key_id = self.options.get("aws_access_key_id")?;
                let aws_secret_access_key = self.options.get("aws_secret_access_key")?;
                let aws_region = self.options.get("aws_region")?;
                let hosted_zone_id = self.options.get("hosted_zone_id").cloned();

                Some(Box::new(Route53::new(
                    context,
                    self.long_id,
                    self.name.as_str(),
                    Domain::new(self.domain.clone()),
                    aws_access_key_id.as_str(),
                    aws_secret_access_key.as_str(),
                    aws_region.as_str(),
                    hosted_zone_id,
                )))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
#[derivative(Debug)]
#[serde(untagged)]
pub enum CloudProviderOptions {
    Aws {
        #[serde(alias = "aws_access_key_id")]
        access_key_id: String,
        #[serde(alias = "aws_secret_access_key")]
        #[derivative(Debug = "ignore")]
        secret_access_key: String,
        #[serde(default, alias = "aws_session_token")]
        session_token: Option<String>,
        #[serde(default, alias = "vsphere_user", alias = "vsphere_username")]
        #[derivative(Debug = "ignore")]
        vsphere_user: Option<String>,
        #[serde(default, alias = "vsphere_password")]
        #[derivative(Debug = "ignore")]
        vsphere_password: Option<String>,
    },
    AwsVsphere {
        #[serde(default, alias = "aws_access_key_id")]
        access_key_id: Option<String>,
        #[serde(default, alias = "aws_secret_access_key")]
        #[derivative(Debug = "ignore")]
        secret_access_key: Option<String>,
        #[serde(default, alias = "aws_session_token")]
        session_token: Option<String>,
        #[serde(alias = "vsphere_user", alias = "vsphere_username")]
        #[derivative(Debug = "ignore")]
        vsphere_user: String,
        #[serde(alias = "vsphere_password")]
        #[derivative(Debug = "ignore")]
        vsphere_password: String,
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
    OnPremise(OnPremiseOptions),
}

#[derive(Serialize, Deserialize, Clone, Derivative)]
#[derivative(Debug)]
#[serde(deny_unknown_fields)]
pub struct OnPremiseOptions {}

#[cfg(test)]
mod tests {
    use super::{CloudProvider, CloudProviderOptions, TerraformStateCredentials};
    use crate::infrastructure::models::{cloud_provider, kubernetes};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn should_build_onprem_provider_from_mixed_options_with_vsphere_credentials() {
        let options = serde_json::from_value::<CloudProviderOptions>(json!({
            "access_key_id": "AKIAxxxxxxxxxxxx",
            "secret_access_key": "xxxxxxxxxxxxxxxxxxxxxxxx",
            "vsphere_user": "svc_vsphere",
            "vsphere_password": "super-secret"
        }))
        .expect("cloud provider options should parse");

        let cloud_provider = CloudProvider {
            kind: cloud_provider::Kind::OnPremise,
            id: "onprem-id".to_string(),
            long_id: Uuid::new_v4(),
            name: "onprem".to_string(),
            zones: vec![],
            options,
            terraform_state_credentials: TerraformStateCredentials {
                access_key_id: "AKIAxxxxxxxxxxxx".to_string(),
                secret_access_key: "xxxxxxxxxxxxxxxxxxxxxxxx".to_string(),
                region: "eu-west-3".to_string(),
                s3_bucket: "terraform-state".to_string(),
                dynamodb_table: "terraform-locks".to_string(),
            },
        };

        let engine_provider = cloud_provider
            .to_engine_cloud_provider("eu-west-3", kubernetes::Kind::OnPremiseSelfManaged)
            .expect("onprem cloud provider should be built");

        let envs = engine_provider.credentials_environment_variables();
        assert!(envs.contains(&("GOVC_USERNAME", "svc_vsphere")));
        assert!(envs.contains(&("GOVC_PASSWORD", "super-secret")));
    }

    #[test]
    fn should_parse_empty_onprem_options_payload() {
        let options =
            serde_json::from_value::<CloudProviderOptions>(json!({})).expect("cloud provider options should parse");

        match options {
            CloudProviderOptions::OnPremise(_) => {}
            _ => panic!("expected OnPremise variant"),
        }
    }

    #[test]
    fn should_build_onprem_provider_from_empty_options() {
        let options =
            serde_json::from_value::<CloudProviderOptions>(json!({})).expect("cloud provider options should parse");

        let cloud_provider = CloudProvider {
            kind: cloud_provider::Kind::OnPremise,
            id: "onprem-id".to_string(),
            long_id: Uuid::new_v4(),
            name: "onprem".to_string(),
            zones: vec![],
            options,
            terraform_state_credentials: TerraformStateCredentials {
                access_key_id: "AKIAxxxxxxxxxxxx".to_string(),
                secret_access_key: "xxxxxxxxxxxxxxxxxxxxxxxx".to_string(),
                region: "eu-west-3".to_string(),
                s3_bucket: "terraform-state".to_string(),
                dynamodb_table: "terraform-locks".to_string(),
            },
        };

        let engine_provider = cloud_provider
            .to_engine_cloud_provider("eu-west-3", kubernetes::Kind::OnPremiseSelfManaged)
            .expect("onprem cloud provider should be built");

        let envs = engine_provider.credentials_environment_variables();
        assert!(!envs.iter().any(|(k, _)| *k == "GOVC_USERNAME"));
        assert!(!envs.iter().any(|(k, _)| *k == "GOVC_PASSWORD"));
    }

    #[test]
    fn should_deserialize_onprem_cloud_provider_with_legacy_extra_options_and_vsphere_credentials() {
        let long_id = Uuid::new_v4();
        let cloud_provider = serde_json::from_value::<CloudProvider>(json!({
            "kind": "ON_PREMISE",
            "id": "onprem-id",
            "long_id": long_id,
            "name": "onprem",
            "zones": [],
            "options": {
                "legacy_field": "legacy-value",
                "vsphere_username": "svc_vsphere",
                "vsphere_password": "super-secret"
            },
            "terraform_state_credentials": {
                "access_key_id": "AKIAxxxxxxxxxxxx",
                "secret_access_key": "xxxxxxxxxxxxxxxxxxxxxxxx",
                "region": "eu-west-3",
                "s3_bucket": "terraform-state",
                "dynamodb_table": "terraform-locks"
            }
        }))
        .expect("cloud provider should parse");

        let engine_provider = cloud_provider
            .to_engine_cloud_provider("eu-west-3", kubernetes::Kind::OnPremiseSelfManaged)
            .expect("onprem cloud provider should be built");

        let envs = engine_provider.credentials_environment_variables();
        assert!(envs.contains(&("GOVC_USERNAME", "svc_vsphere")));
        assert!(envs.contains(&("GOVC_PASSWORD", "super-secret")));
    }

    #[test]
    fn should_deserialize_onprem_cloud_provider_with_unrelated_legacy_options() {
        let long_id = Uuid::new_v4();
        let cloud_provider = serde_json::from_value::<CloudProvider>(json!({
            "kind": "ON_PREMISE",
            "id": "onprem-id",
            "long_id": long_id,
            "name": "onprem",
            "zones": [],
            "options": {
                "legacy_field": "legacy-value",
                "another_legacy_field": "another-value"
            },
            "terraform_state_credentials": {
                "access_key_id": "AKIAxxxxxxxxxxxx",
                "secret_access_key": "xxxxxxxxxxxxxxxxxxxxxxxx",
                "region": "eu-west-3",
                "s3_bucket": "terraform-state",
                "dynamodb_table": "terraform-locks"
            }
        }))
        .expect("cloud provider should parse");

        let engine_provider = cloud_provider
            .to_engine_cloud_provider("eu-west-3", kubernetes::Kind::OnPremiseSelfManaged)
            .expect("onprem cloud provider should be built");

        let envs = engine_provider.credentials_environment_variables();
        assert!(!envs.iter().any(|(k, _)| *k == "GOVC_USERNAME"));
        assert!(!envs.iter().any(|(k, _)| *k == "GOVC_PASSWORD"));
    }

    #[test]
    fn should_build_eks_anywhere_aws_provider_with_vsphere_credentials() {
        let options = serde_json::from_value::<CloudProviderOptions>(json!({
            "access_key_id": "AKIAxxxxxxxxxxxx",
            "secret_access_key": "xxxxxxxxxxxxxxxxxxxxxxxx",
            "vsphere_user": "svc_vsphere",
            "vsphere_password": "super-secret"
        }))
        .expect("cloud provider options should parse");

        let cloud_provider = CloudProvider {
            kind: cloud_provider::Kind::Aws,
            id: "aws-id".to_string(),
            long_id: Uuid::new_v4(),
            name: "aws".to_string(),
            zones: vec!["eu-west-3a".to_string()],
            options,
            terraform_state_credentials: TerraformStateCredentials {
                access_key_id: "AKIAxxxxxxxxxxxx".to_string(),
                secret_access_key: "xxxxxxxxxxxxxxxxxxxxxxxx".to_string(),
                region: "eu-west-3".to_string(),
                s3_bucket: "terraform-state".to_string(),
                dynamodb_table: "terraform-locks".to_string(),
            },
        };

        let engine_provider = cloud_provider
            .to_engine_cloud_provider("eu-west-3", kubernetes::Kind::EksAnywhere)
            .expect("eks anywhere cloud provider should be built");

        let envs = engine_provider.credentials_environment_variables();
        assert!(envs.contains(&("AWS_ACCESS_KEY_ID", "AKIAxxxxxxxxxxxx")));
        assert!(envs.contains(&("AWS_SECRET_ACCESS_KEY", "xxxxxxxxxxxxxxxxxxxxxxxx")));
        assert!(envs.contains(&("GOVC_USERNAME", "svc_vsphere")));
        assert!(envs.contains(&("GOVC_PASSWORD", "super-secret")));
    }

    #[test]
    fn should_build_eks_anywhere_aws_provider_from_vsphere_only_options() {
        let options = serde_json::from_value::<CloudProviderOptions>(json!({
            "vsphere_user": "svc_vsphere",
            "vsphere_password": "super-secret"
        }))
        .expect("cloud provider options should parse");

        let cloud_provider = CloudProvider {
            kind: cloud_provider::Kind::Aws,
            id: "aws-id".to_string(),
            long_id: Uuid::new_v4(),
            name: "aws".to_string(),
            zones: vec!["eu-west-3a".to_string()],
            options,
            terraform_state_credentials: TerraformStateCredentials {
                access_key_id: "AKIA_TFSTATE_FALLBACK".to_string(),
                secret_access_key: "TFSTATE_SECRET_FALLBACK".to_string(),
                region: "eu-west-3".to_string(),
                s3_bucket: "terraform-state".to_string(),
                dynamodb_table: "terraform-locks".to_string(),
            },
        };

        let engine_provider = cloud_provider
            .to_engine_cloud_provider("eu-west-3", kubernetes::Kind::EksAnywhere)
            .expect("eks anywhere cloud provider should be built");

        let envs = engine_provider.credentials_environment_variables();
        assert!(envs.contains(&("AWS_ACCESS_KEY_ID", "AKIA_TFSTATE_FALLBACK")));
        assert!(envs.contains(&("AWS_SECRET_ACCESS_KEY", "TFSTATE_SECRET_FALLBACK")));
        assert!(envs.contains(&("GOVC_USERNAME", "svc_vsphere")));
        assert!(envs.contains(&("GOVC_PASSWORD", "super-secret")));
    }

    #[test]
    fn should_parse_aws_options_with_prefixed_aliases() {
        let options = serde_json::from_value::<CloudProviderOptions>(json!({
            "aws_access_key_id": "AKIA_ALIAS",
            "aws_secret_access_key": "ALIAS_SECRET",
            "aws_session_token": "ALIAS_TOKEN"
        }))
        .expect("cloud provider options should parse");

        match options {
            CloudProviderOptions::Aws {
                access_key_id,
                secret_access_key,
                session_token,
                ..
            } => {
                assert_eq!(access_key_id, "AKIA_ALIAS");
                assert_eq!(secret_access_key, "ALIAS_SECRET");
                assert_eq!(session_token.as_deref(), Some("ALIAS_TOKEN"));
            }
            _ => panic!("expected Aws variant"),
        }
    }
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
    #[serde(deserialize_with = "try_gcp_credentials_from_str")]
    // Allow to deserialize string field to its struct counterpart
    pub gcp_credentials: Option<JsonCredentialsIo>,
    region: String,
}

/// Allow to properly deserialize JSON credentials from string, making sure to escape \n from keys strings
fn try_gcp_credentials_from_str<'de, D>(
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

///  Deserializes JSON credentials from string,and escapes '\n'
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
