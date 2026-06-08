mod cluster_create;
mod cluster_delete;
mod cluster_pause;
mod cluster_upgrade;
pub(crate) mod helm_charts;
mod tera_context;

use crate::environment::models::gcp::GcpCredentials;
use crate::environment::models::gcp::io::JsonCredentials as IoJsonCredentials;
use crate::environment::models::types::VersionsNumber;
use crate::errors::CommandError as EngineCommandError;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::events::InfrastructureStep;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::gke::cluster_create::create_gke_cluster;
use crate::infrastructure::action::gke::cluster_delete::delete_gke_cluster;
use crate::infrastructure::action::gke::cluster_pause::pause_gke_cluster;
use crate::infrastructure::action::gke::cluster_upgrade::upgrade_gke_cluster;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::gcp::Gke;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesUpgradeStatus, send_progress_on_long_task};
use crate::runtime::block_on;
use crate::services::gcp::google_cloud_sdk_types::new_google_auth_credentials_from_access_token;
use google_cloud_auth::credentials::Credentials as GoogleCloudCredentials;
use google_cloud_auth::credentials::service_account::Builder as ServiceAccountCredentialsBuilder;
use google_cloud_compute_v1::client::Firewalls;
use google_cloud_container_v1::client::ClusterManager;
use google_cloud_container_v1::model::operation::Status as GkeOperationStatus;
use google_cloud_container_v1::model::{
    ClusterUpdate, GetClusterRequest, GetOperationRequest, MasterAuthorizedNetworksConfig, UpdateClusterRequest,
};
use serde_derive::{Deserialize, Serialize};
use std::time::{Duration, Instant};

impl InfrastructureAction for Gke {
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        _has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);
        send_progress_on_long_task(self, Action::Create, || create_gke_cluster(self, infra_ctx, logger))
    }

    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Pause);
        send_progress_on_long_task(self, Action::Pause, || pause_gke_cluster(self, infra_ctx, logger))
    }

    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Delete);
        send_progress_on_long_task(self, Action::Delete, || delete_gke_cluster(self, infra_ctx, logger))
    }

    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Upgrade);

        send_progress_on_long_task(self, Action::Create, || {
            upgrade_gke_cluster(self, infra_ctx, kubernetes_upgrade_status, logger)
        })
    }
}

use super::utils::{from_terraform_optional_version_number, from_terraform_value, mk_logger};

// Workaround for a Terraform Google provider bug: switching GKE master authorized networks
// from enabled to disabled is not reliably handled when the desired Terraform config removes
// the list (static IP mode turned off). We explicitly disable MAN via gcloud before Terraform
// apply when static_ip_mode=false to keep the cluster state aligned.
// Reference: https://github.com/hashicorp/terraform-provider-google/issues/10198
fn disable_master_authorized_networks_if_necessary(
    cluster: &Gke,
    logger: &impl InfraLogger,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    if cluster.advanced_settings().qovery_static_ip_mode.unwrap_or(false) {
        return Ok(());
    }

    let cluster_resource_name = gke_cluster_resource_name(cluster);
    let gke_client = gke_client(cluster, event_details.clone())?;
    let is_enabled = match block_on(is_master_authorized_networks_enabled(&gke_client, &cluster_resource_name)) {
        Ok(is_enabled) => is_enabled,
        Err(err) => {
            if is_cluster_not_found_error(&err) {
                return Ok(());
            }

            return Err(Box::new(EngineError::new_cannot_get_cluster_error(
                event_details.clone(),
                EngineCommandError::new_from_safe_message(format!(
                    "Failed to describe GKE cluster before disabling master authorized networks: {err}"
                )),
            )));
        }
    };

    if !is_enabled {
        return Ok(());
    }

    logger.info("Disabling GKE master authorized networks for unrestricted API access.");
    let operation =
        block_on(disable_master_authorized_networks(&gke_client, &cluster_resource_name)).map_err(|err| {
            Box::new(EngineError::new_cannot_get_cluster_error(
                event_details.clone(),
                EngineCommandError::new_from_safe_message(format!(
                    "Failed to disable GKE master authorized networks: {err}"
                )),
            ))
        })?;

    block_on(wait_for_gke_operation(
        &gke_client,
        operation.name.as_str(),
        logger,
        event_details,
    ))?;

    Ok(())
}

fn gke_cluster_resource_name(cluster: &Gke) -> String {
    format!(
        "projects/{}/locations/{}/clusters/{}",
        cluster.credentials.project_id(),
        cluster.region(),
        cluster.cluster_name()
    )
}

fn build_service_account_credentials(sa: IoJsonCredentials) -> Result<GoogleCloudCredentials, String> {
    let json = serde_json::to_value(sa)
        .map_err(|err| format!("Failed to serialize GCP service account credentials: {err}"))?;
    ServiceAccountCredentialsBuilder::new(json)
        .build()
        .map_err(|err| format!("Failed to build GCP service account credentials: {err}"))
}

fn gcp_credentials(cluster: &Gke) -> Result<GoogleCloudCredentials, String> {
    match &cluster.credentials {
        GcpCredentials::ServiceAccount(sa) => {
            build_service_account_credentials(IoJsonCredentials::from(sa.as_ref().clone()))
        }
        GcpCredentials::AccessToken(at) => Ok(new_google_auth_credentials_from_access_token(at)),
    }
}

pub(super) fn firewalls_client(cluster: &Gke) -> Result<Firewalls, String> {
    block_on(async move {
        let credentials = gcp_credentials(cluster)?;
        Firewalls::builder()
            .with_credentials(credentials)
            .build()
            .await
            .map_err(|err| format!("Failed to create GCP Firewalls API client: {err}"))
    })
}

fn gke_client(cluster: &Gke, event_details: EventDetails) -> Result<ClusterManager, Box<EngineError>> {
    let client = match &cluster.credentials {
        GcpCredentials::ServiceAccount(credentials) => {
            let service_account_json = serde_json::to_value(IoJsonCredentials::from(credentials.as_ref().clone()))
                .map_err(|err| {
                    Box::new(EngineError::new_cannot_get_cluster_error(
                        event_details.clone(),
                        EngineCommandError::new_from_safe_message(format!(
                            "Failed to serialize GCP service account credentials: {err}"
                        )),
                    ))
                })?;

            block_on(async move {
                let credentials = ServiceAccountCredentialsBuilder::new(service_account_json)
                    .build()
                    .map_err(|err| format!("Failed to create GCP credentials for GKE API client: {err}"))?;

                ClusterManager::builder()
                    .with_credentials(credentials)
                    .build()
                    .await
                    .map_err(|err| format!("Failed to create GKE API client: {err}"))
            })
        }
        GcpCredentials::AccessToken(credentials) => block_on(async move {
            ClusterManager::builder()
                .with_credentials(new_google_auth_credentials_from_access_token(credentials))
                .build()
                .await
                .map_err(|err| format!("Failed to create GKE API client: {err}"))
        }),
    };

    client.map_err(|err_message| {
        Box::new(EngineError::new_cannot_get_cluster_error(
            event_details,
            EngineCommandError::new_from_safe_message(err_message),
        ))
    })
}

async fn is_master_authorized_networks_enabled(
    gke_client: &ClusterManager,
    cluster_resource_name: &str,
) -> Result<bool, google_cloud_container_v1::Error> {
    let request = GetClusterRequest::new().set_name(cluster_resource_name);
    let cluster = gke_client.get_cluster().with_request(request).send().await?;

    #[allow(deprecated)]
    let is_enabled = cluster
        .master_authorized_networks_config
        .map(|config| config.enabled)
        .unwrap_or(false);

    Ok(is_enabled)
}

async fn disable_master_authorized_networks(
    gke_client: &ClusterManager,
    cluster_resource_name: &str,
) -> Result<google_cloud_container_v1::model::Operation, google_cloud_container_v1::Error> {
    #[allow(deprecated)]
    let update = ClusterUpdate::new()
        .set_desired_master_authorized_networks_config(MasterAuthorizedNetworksConfig::new().set_enabled(false));

    let request = UpdateClusterRequest::new()
        .set_name(cluster_resource_name)
        .set_update(update);

    gke_client.update_cluster().with_request(request).send().await
}

async fn wait_for_gke_operation(
    gke_client: &ClusterManager,
    operation_name: &str,
    logger: &impl InfraLogger,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    const OPERATION_TIMEOUT: Duration = Duration::from_secs(60 * 10);
    const OPERATION_POLL_INTERVAL: Duration = Duration::from_secs(5);

    if operation_name.is_empty() {
        return Err(Box::new(EngineError::new_cannot_get_cluster_error(
            event_details,
            EngineCommandError::new_from_safe_message(
                "GKE MAN disable operation returned an empty operation name.".to_string(),
            ),
        )));
    }

    let deadline = Instant::now() + OPERATION_TIMEOUT;
    loop {
        let request = GetOperationRequest::new().set_name(operation_name);
        let operation = gke_client
            .get_operation()
            .with_request(request)
            .send()
            .await
            .map_err(|err| {
                Box::new(EngineError::new_cannot_get_cluster_error(
                    event_details.clone(),
                    EngineCommandError::new_from_safe_message(format!(
                        "Failed to retrieve GKE operation status ({operation_name}): {err}"
                    )),
                ))
            })?;

        if operation.status == GkeOperationStatus::Done {
            if let Some(error) = operation.error
                && error.code != 0
            {
                return Err(Box::new(EngineError::new_cannot_get_cluster_error(
                    event_details,
                    EngineCommandError::new_from_safe_message(format!(
                        "GKE MAN disable operation ({operation_name}) failed with code {}: {}",
                        error.code, error.message
                    )),
                )));
            }
            return Ok(());
        }

        logger.info(format!(
            "Waiting for GKE MAN disable operation {operation_name} to complete (status: {:?}).",
            operation.status
        ));

        if Instant::now() >= deadline {
            return Err(Box::new(EngineError::new_cannot_get_cluster_error(
                event_details,
                EngineCommandError::new_from_safe_message(format!(
                    "Timed out while waiting for GKE MAN disable operation to complete ({operation_name})."
                )),
            )));
        }

        tokio::time::sleep(OPERATION_POLL_INTERVAL).await;
    }
}

fn is_cluster_not_found_error(error: &google_cloud_container_v1::Error) -> bool {
    if error.http_status_code() == Some(404) {
        return true;
    }

    let message = error.to_string().to_lowercase();
    message.contains("not_found") || message.contains("not found")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GkeQoveryTerraformOutput {
    #[serde(deserialize_with = "from_terraform_value")]
    pub gke_cluster_public_hostname: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub loki_logging_service_account_email: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub kubeconfig: String,

    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    thanos_service_account_email: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub cluster_name: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub cluster_self_link: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub cluster_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub network: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub keda_operator_service_account_email: Option<String>,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub keda_metrics_server_service_account_email: Option<String>,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub external_secrets_operator_service_account_email: Option<String>,
    #[serde(deserialize_with = "from_terraform_optional_version_number")]
    #[serde(default)]
    pub qovery_deployed_with_engine_version: Option<VersionsNumber>,
}
