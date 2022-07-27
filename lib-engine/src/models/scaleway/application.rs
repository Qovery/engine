use crate::cloud_provider::kubernetes::validate_k8s_required_cpu_and_burstable;
use crate::cloud_provider::models::StorageDataTemplate;
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::io_models::progress_listener::ListenersHelper;
use crate::models::application::Application;
use crate::models::scaleway::ScwStorageType;
use crate::models::types::{ToTeraContext, SCW};
use tera::Context as TeraContext;

impl ToTeraContext for Application<SCW> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = self.default_tera_context(kubernetes, environment);

        // container registry credentials
        context.insert("registry_secret_name", &format!("registry-token-{}", &self.id));
        context.insert(
            "container_registry_docker_json_config",
            self.build
                .image
                .clone()
                .registry_docker_json_config
                .unwrap_or_default()
                .as_str(),
        );

        let cpu_limits = match validate_k8s_required_cpu_and_burstable(
            &ListenersHelper::new(&self.listeners),
            self.context.execution_id(),
            &self.id,
            self.total_cpus(),
            self.cpu_burst(),
            event_details.clone(),
            self.logger(),
        ) {
            Ok(l) => l,
            Err(e) => {
                return Err(EngineError::new_k8s_validate_required_cpu_and_burstable_error(
                    event_details,
                    self.total_cpus(),
                    self.cpu_burst(),
                    e,
                ));
            }
        };
        context.insert("cpu_burst", &cpu_limits.cpu_limit);

        let storage = self
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

        let is_storage = !storage.is_empty();
        context.insert("storage", &storage);
        context.insert("is_storage", &is_storage);

        Ok(context)
    }
}
