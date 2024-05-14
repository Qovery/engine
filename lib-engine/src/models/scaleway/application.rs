use crate::cloud_provider::models::StorageDataTemplate;
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::application::Application;
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
                storage_type: s.storage_class.0.clone(),
                size_in_gib: s.size_in_gib,
                mount_point: s.mount_point.clone(),
                snapshot_retention_in_days: s.snapshot_retention_in_days,
            })
            .collect::<Vec<_>>();

        context.service.storages = storages;

        Ok(TeraContext::from_serialize(context).unwrap_or_default())
    }
}
