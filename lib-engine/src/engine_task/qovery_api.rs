use crate::infrastructure::action::cluster_outputs_helper::ClusterOutputsRequest;
use crate::infrastructure::models::cloud_provider::service::ServiceType;
use crate::io_models::application::GitCredentials;
use anyhow::anyhow;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Eq, PartialEq, Hash)]
pub enum EngineServiceType {
    ShellAgent,
    ClusterAgent,
    Engine,
}

pub trait QoveryApi: Send + Sync {
    fn service_version(&self, service_type: EngineServiceType) -> anyhow::Result<String>;
    fn git_token(&self, service_type: ServiceType, service_id: &Uuid) -> anyhow::Result<GitCredentials>;

    fn update_cluster_outputs(&self, cluster_state_request: &ClusterOutputsRequest) -> anyhow::Result<()>;
}

pub struct FakeQoveryApi {}

impl QoveryApi for FakeQoveryApi {
    fn service_version(&self, _service_type: EngineServiceType) -> anyhow::Result<String> {
        Err(anyhow!("not implemented"))
    }

    fn git_token(&self, _service_type: ServiceType, _service_id: &Uuid) -> anyhow::Result<GitCredentials> {
        Err(anyhow!("not implemented"))
    }

    fn update_cluster_outputs(&self, _cluster_outputs_request: &ClusterOutputsRequest) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct StaticQoveryApi {
    pub versions: HashMap<EngineServiceType, String>,
}

impl QoveryApi for StaticQoveryApi {
    fn service_version(&self, service_type: EngineServiceType) -> anyhow::Result<String> {
        Ok(self
            .versions
            .get(&service_type)
            .ok_or_else(|| anyhow!("Missing service version for {service_type:?}"))?
            .clone())
    }

    fn git_token(&self, _service_type: ServiceType, _service_id: &Uuid) -> anyhow::Result<GitCredentials> {
        Err(anyhow!("not implemented"))
    }

    fn update_cluster_outputs(&self, _cluster_outputs_request: &ClusterOutputsRequest) -> anyhow::Result<()> {
        Ok(())
    }
}
