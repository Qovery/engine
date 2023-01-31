use crate::cloud_provider::service::ServiceType;
use crate::io_models::application::GitCredentials;
use anyhow::anyhow;
use uuid::Uuid;

#[derive(Debug)]
pub enum EngineServiceType {
    ShellAgent,
    ClusterAgent,
    Engine,
}

pub trait QoveryApi: Send + Sync {
    fn service_version(&self, service_type: EngineServiceType) -> anyhow::Result<String>;
    fn git_token(&self, service_type: ServiceType, service_id: &Uuid) -> anyhow::Result<GitCredentials>;
}

pub struct FakeCoreServiceApi {}

impl QoveryApi for FakeCoreServiceApi {
    fn service_version(&self, _service_type: EngineServiceType) -> anyhow::Result<String> {
        Err(anyhow!("not implemented"))
    }

    fn git_token(&self, _service_type: ServiceType, _service_id: &Uuid) -> anyhow::Result<GitCredentials> {
        Err(anyhow!("not implemented"))
    }
}
