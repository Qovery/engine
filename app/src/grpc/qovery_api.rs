use super::GrpcEngineClient;
use crate::grpc::engine::{GitTokenRequest, ServiceVersionRequest};
use crate::tokio_utils::block_on;
use anyhow::Context;
use chrono::DateTime;
use qovery_engine::cloud_provider::service::ServiceType;
use qovery_engine::engine_task::core_service_api::{EngineServiceType, QoveryApi};
use qovery_engine::io_models::application::GitCredentials;
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

impl QoveryApi for GrpcCoreServiceApi {
    fn service_version(&self, service_type: EngineServiceType) -> anyhow::Result<String> {
        let call = async {
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

        block_on(call)
    }

    fn git_token(&self, service_type: ServiceType, service_id: &Uuid) -> anyhow::Result<GitCredentials> {
        let call = async {
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

        block_on(call)
    }
}
