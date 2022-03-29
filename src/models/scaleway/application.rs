use crate::cloud_provider::kubernetes::validate_k8s_required_cpu_and_burstable;
use crate::cloud_provider::models::{EnvironmentVariableDataTemplate, StorageDataTemplate};
use crate::cloud_provider::service::default_tera_context;
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::io_models::ListenersHelper;
use crate::models::application::Application;
use crate::models::scaleway::ScwStorageType;
use crate::models::types::{ToTeraContext, SCW};
use tera::Context as TeraContext;

impl ToTeraContext for Application<SCW> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);
        let commit_id = self.build.image.commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);
        context.insert("image_name_with_tag", &self.build.image.full_image_name_with_tag());

        let environment_variables = self
            .environment_variables
            .iter()
            .map(|ev| EnvironmentVariableDataTemplate {
                key: ev.key.clone(),
                value: ev.value.clone(),
            })
            .collect::<Vec<_>>();

        context.insert("environment_variables", &environment_variables);
        context.insert("ports", &self.ports);
        context.insert("is_registry_secret", &true);
        context.insert("registry_secret_name", &format!("registry-token-{}", &self.id));

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
        context.insert("clone", &false);
        context.insert("start_timeout_in_seconds", &self.start_timeout_in_seconds);

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert("resource_expiration_in_seconds", &self.context.resource_expiration_in_seconds())
        }

        // container registry credentials
        context.insert(
            "container_registry_docker_json_config",
            self.build
                .image
                .clone()
                .registry_docker_json_config
                .unwrap_or_default()
                .as_str(),
        );

        Ok(context)
    }
}
