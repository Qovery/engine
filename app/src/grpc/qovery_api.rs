use super::GrpcEngineClient;
use crate::grpc::engine::{
    ClusterOutputsUpdateRequest, ExternalSecretsAuthentication as ProtoEsa, GitTokenRequest, KubernetesProviderKind,
    ServiceVersionRequest, TerraformResourcesRequest,
    external_secrets_authentication::Authentication as ProtoEsaAuthentication,
};
use crate::tokio_utils::block_on;
use anyhow::{Context, anyhow};
use chrono::DateTime;
use qovery_engine::engine_task::qovery_api::{
    EngineServiceType, QoveryApi, TerraformResourcesRequest as EngineResourcesRequest,
};
use qovery_engine::infrastructure::action::cluster_outputs_helper::{
    ClusterOutputsRequest, ExternalSecretsAuthentication,
};
use qovery_engine::infrastructure::models::cloud_provider::service::ServiceType;
use qovery_engine::infrastructure::models::kubernetes::Kind;
use qovery_engine::io_models::application::GitCredentials;
use std::future::Future;
use std::thread;
use std::time::Duration;
use uuid::Uuid;

pub struct GrpcCoreServiceApi {
    jwt_token: String,
    client: GrpcEngineClient,
}

impl GrpcCoreServiceApi {
    pub fn new(jwt_token: String, client: GrpcEngineClient) -> Self {
        GrpcCoreServiceApi { jwt_token, client }
    }
}

/// Remove the timestamp suffix from execution_id
/// Format: {uuid}-{version}-{timestamp} -> {uuid}-{version}
fn clean_execution_id(execution_id: &str) -> String {
    // Find the last dash and check if what follows is a numeric timestamp
    if let Some(last_dash_pos) = execution_id.rfind('-') {
        let potential_timestamp = &execution_id[last_dash_pos + 1..];
        // Unix timestamps are typically 10 digits (seconds) or 13+ (milliseconds)
        // Version numbers are usually 1-3 digits, so a long numeric suffix indicates a timestamp
        if potential_timestamp.chars().all(|c| c.is_ascii_digit()) && potential_timestamp.len() >= 10 {
            return execution_id[..last_dash_pos].to_string();
        }
    }
    // If not in expected format, return as-is
    execution_id.to_string()
}

fn with_max_retry<T, F, Fut>(f: F, max_retry: usize) -> anyhow::Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let mut retry = 0;
    let mut retry_delay = Duration::from_secs(0);
    loop {
        match block_on(f()) {
            Ok(v) => return Ok(v),
            Err(err) => {
                error!("retrying due to error: {:?}", err);
                if retry > max_retry {
                    return Err(err);
                }
                retry += 1;
                thread::sleep(retry_delay);
                retry_delay += Duration::from_secs(1);
            }
        }
    }
}

impl QoveryApi for GrpcCoreServiceApi {
    fn service_version(&self, service_type: EngineServiceType) -> anyhow::Result<String> {
        let call = || async {
            info!("Getting service version for {:?}", service_type);
            let response = self
                .client
                .clone()
                .get_service_version(ServiceVersionRequest {
                    jwt_token: self.jwt_token.clone(),
                    service_type: match service_type {
                        EngineServiceType::ShellAgent => super::engine::EngineServiceType::ShellAgent as i32,
                        EngineServiceType::ClusterAgent => super::engine::EngineServiceType::ClusterAgent as i32,
                        EngineServiceType::Engine => super::engine::EngineServiceType::Engine as i32,
                    },
                })
                .await?;

            let response = response.into_inner();
            info!("Service version for {:?} is {:?}", service_type, response);
            Ok(response.version)
        };

        with_max_retry(call, 5)
    }

    fn git_token(&self, service_type: ServiceType, service_id: &Uuid) -> anyhow::Result<GitCredentials> {
        let call = || async {
            info!("Fetching git token for service type: {:?}({})", service_type, service_id);
            let response = self
                .client
                .clone()
                .get_git_token(GitTokenRequest {
                    jwt_token: self.jwt_token.clone(),
                    service_type: match service_type {
                        ServiceType::Application => super::engine::ServiceType::Application as i32,
                        ServiceType::Database(_) => super::engine::ServiceType::UnknownSrv as i32,
                        ServiceType::Router => super::engine::ServiceType::UnknownSrv as i32,
                        ServiceType::Container => super::engine::ServiceType::UnknownSrv as i32,
                        ServiceType::Job => super::engine::ServiceType::Job as i32,
                        ServiceType::HelmChart => super::engine::ServiceType::Helm as i32,
                        ServiceType::Terraform => super::engine::ServiceType::Terraform as i32,
                    },
                    service_id: service_id.to_string(),
                })
                .await?
                .into_inner();

            info!(
                "Retrieved git token for service type: {:?}({}) with expiration date {:?}",
                service_type, service_id, response.expired_at
            );
            Ok(GitCredentials {
                login: response.login,
                access_token: response.access_token,
                expired_at: DateTime::parse_from_rfc3339(&response.expired_at)
                    .with_context(|| "invalid datetime for expired_at field")?
                    .with_timezone(&chrono::Utc),
            })
        };

        with_max_retry(call, 5)
    }

    fn update_cluster_outputs(&self, cluster_outputs_request: &ClusterOutputsRequest) -> anyhow::Result<()> {
        info!("update_cluster_outputs");
        let kubernetes_provider_kind = to_kubernetes_provider_kind(cluster_outputs_request.kubernetes_kind)?;

        let call = || async {
            info!("Updating cluster outputs for cluster kind {:?}", kubernetes_provider_kind);
            self.client
                .clone()
                .update_cluster_outputs(ClusterOutputsUpdateRequest {
                    jwt_token: self.jwt_token.clone(),
                    kubeconfig: cluster_outputs_request.kubeconfig.clone(),
                    kubernetes_provider_kind: kubernetes_provider_kind.into(),
                    cluster_name: cluster_outputs_request.cluster_name.to_string(),
                    cluster_id: cluster_outputs_request.cluster_id.to_string(),
                    cluster_arn: cluster_outputs_request.cluster_arn.clone(),
                    cluster_self_link: cluster_outputs_request.cluster_self_link.clone(),
                    cluster_oidc_issuer: cluster_outputs_request.cluster_oidc_issuer.clone(),
                    vpc_id: cluster_outputs_request.cluster_vpc_id.clone(),
                    network: cluster_outputs_request.network.clone(),
                    private_network_id: cluster_outputs_request.private_network_id.clone(),
                    external_secrets_automatic_authentication: cluster_outputs_request
                        .external_secrets_automatic_authentication
                        .as_ref()
                        .map(|auth| ProtoEsa {
                            authentication: Some(match auth {
                                ExternalSecretsAuthentication::EksRoleArn(arn) => {
                                    ProtoEsaAuthentication::EksRoleArn(arn.clone())
                                }
                                ExternalSecretsAuthentication::GkeServiceAccount(sa) => {
                                    ProtoEsaAuthentication::GkeServiceAccount(sa.clone())
                                }
                            }),
                        }),
                })
                .await?;

            Ok(())
        };

        with_max_retry(call, 5)
    }

    fn send_terraform_resources(&self, request: &EngineResourcesRequest) -> anyhow::Result<()> {
        info!(
            "send_terraform_resources for terraform {} with execution_id {}",
            request.terraform_id, request.execution_id
        );

        let terraform_resources_request = TerraformResourcesRequest {
            jwt_token: self.jwt_token.clone(),
            terraform_id: request.terraform_id.to_string(),
            execution_id: clean_execution_id(&request.execution_id),
            resources_json: serde_json::to_string(&request.resources)
                .context("Failed to serialize terraform resources to JSON")?,
        };

        let call = || async {
            info!("Sending {} terraform resources to core", request.resources.len());
            self.client
                .clone()
                .send_terraform_resources(terraform_resources_request.clone())
                .await?;

            info!("Successfully sent terraform resources to core");
            Ok(())
        };

        with_max_retry(call, 5)
    }
}

fn to_kubernetes_provider_kind(kubernetes_kind: Kind) -> anyhow::Result<KubernetesProviderKind> {
    match kubernetes_kind {
        Kind::Eks => Ok(KubernetesProviderKind::Eks),
        Kind::ScwKapsule => Ok(KubernetesProviderKind::ScwKapsule),
        Kind::Gke => Ok(KubernetesProviderKind::Gke),
        Kind::Aks => Ok(KubernetesProviderKind::Aks),
        Kind::EksSelfManaged
        | Kind::GkeSelfManaged
        | Kind::AksSelfManaged
        | Kind::ScwSelfManaged
        | Kind::OnPremiseSelfManaged
        | Kind::EksAnywhere => Err(anyhow!(format!("kubernetes_kind is not supported: {}", kubernetes_kind))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_execution_id_removes_timestamp() {
        let input = "bb1d1f62-a078-4a0f-9766-d135f904903a-8-1768511080";
        let expected = "bb1d1f62-a078-4a0f-9766-d135f904903a-8";
        assert_eq!(clean_execution_id(input), expected);
    }

    #[test]
    fn test_clean_execution_id_without_timestamp() {
        let input = "bb1d1f62-a078-4a0f-9766-d135f904903a-8";
        assert_eq!(clean_execution_id(input), input);
    }

    #[test]
    fn test_clean_execution_id_with_non_numeric_suffix() {
        let input = "bb1d1f62-a078-4a0f-9766-d135f904903a-abc";
        assert_eq!(clean_execution_id(input), input);
    }
}
