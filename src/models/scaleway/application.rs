use crate::cloud_provider::models::StorageDataTemplate;
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::application::Application;
use crate::models::scaleway::ScwStorageType;
use crate::models::types::{ToTeraContext, SCW};
use tera::Context as TeraContext;

impl ToTeraContext for Application<SCW> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        let mut context = self.default_tera_context(target);
        let storages = self
            .storage
            .iter()
            .map(|s| StorageDataTemplate {
                id: s.id.clone(),
                long_id: self.long_id,
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

        Ok(TeraContext::from_serialize(context).unwrap_or_default())
    }
}
