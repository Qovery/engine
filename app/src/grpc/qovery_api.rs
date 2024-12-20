use super::GrpcEngineClient;
use crate::grpc::engine::{ClusterCredentialsUpdate, GitTokenRequest, ServiceVersionRequest};
use crate::tokio_utils::block_on;
use anyhow::Context;
use chrono::DateTime;
use qovery_engine::engine_task::qovery_api::{EngineServiceType, QoveryApi};
use qovery_engine::infrastructure::models::cloud_provider::service::ServiceType;
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

    fn update_cluster_credentials(&self, kubeconfig: String) -> anyhow::Result<()> {
        let call = || async {
            info!("Updating cluster credentials");
            self.client
                .clone()
                .update_cluster_credentials(ClusterCredentialsUpdate {
                    jwt_token: self.jwt_token.clone(),
                    kubeconfig: kubeconfig.clone(),
                })
                .await?;

            Ok(())
        };

        with_max_retry(call, 5)
    }
}
