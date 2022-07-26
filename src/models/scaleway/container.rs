use crate::cloud_provider::models::StorageDataTemplate;
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::container::{Container, RegistryTeraContext};
use crate::models::scaleway::ScwStorageType;
use crate::models::types::{ToTeraContext, SCW};
use tera::Context as TeraContext;

impl ToTeraContext for Container<SCW> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let mut context = self.default_tera_context(target.kubernetes, target.environment);

        // container registry credentials
        let registry = RegistryTeraContext {
            secret_name: format!("registry-token-{}", &self.long_id),
            // FIXME: Find a way to get the registry docker json config
            docker_json_config: "".to_string(),
        };
        //    self.build
        //        .image
        //        .clone()
        //        .registry_docker_json_config
        //        .unwrap_or_default()
        //        .as_str();

        let storages = self
            .storages
            .iter()
            .map(|s| StorageDataTemplate {
                id: s.id.clone(),
                name: s.name.clone(),
                storage_type: match s.storage_type {
                    // TODO(benjaminch): Switch to proper storage class
                    // Note: Seems volume storage type are not supported, only blocked storage for the time being
                    // https://github.com/scaleway/scaleway-csi/tree/master/examples/kubernetes#different-storageclass
                    ScwStorageType::BlockSsd => "scw-sbv-ssd-0", // "b_ssd",
                    ScwStorageType::LocalSsd => "l_ssd",
                }
                .to_string(),
                size_in_gib: s.size_in_gib,
                mount_point: s.mount_point.clone(),
                snapshot_retention_in_days: s.snapshot_retention_in_days,
            })
            .collect::<Vec<_>>();

        context.service.storages = storages;
        context.registry = Some(registry);

        Ok(TeraContext::from_serialize(context).unwrap_or_default())
    }
}
