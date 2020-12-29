use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::digitalocean::common::get_uuid_of_cluster_from_name;
use crate::cloud_provider::digitalocean::DO;
use crate::cloud_provider::models::{
    EnvironmentVariable, EnvironmentVariableDataTemplate, Storage, StorageDataTemplate,
};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, deploy_stateless_service_error,
    deploy_user_stateless_service, Action, Create, Delete, Helm, Pause, Service, ServiceType,
    StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::container_registry::docr::subscribe_kube_cluster_to_container_registry;
use crate::error::{EngineError, EngineErrorScope};
use crate::models::Context;

pub struct Application {
    context: Context,
    id: String,
    action: Action,
    name: String,
    private_port: Option<u16>,
    total_cpus: String,
    cpu_burst: String,
    total_ram_in_mib: u32,
    total_instances: u16,
    start_timeout_in_seconds: u32,
    image: Image,
    storage: Vec<Storage<StorageType>>,
    environment_variables: Vec<EnvironmentVariable>,
}

impl Application {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        private_port: Option<u16>,
        total_cpus: String,
        cpu_burst: String,
        total_ram_in_mib: u32,
        total_instances: u16,
        start_timeout_in_seconds: u32,
        image: Image,
        storage: Vec<Storage<StorageType>>,
        environment_variables: Vec<EnvironmentVariable>,
    ) -> Self {
        Application {
            context,
            id: id.to_string(),
            action,
            name: name.to_string(),
            private_port,
            total_cpus,
            cpu_burst,
            total_ram_in_mib,
            total_instances,
            start_timeout_in_seconds,
            image,
            storage,
            environment_variables,
        }
    }
}

impl crate::cloud_provider::service::Application for Application {
    fn image(&self) -> &Image {
        &self.image
    }

    fn set_image(&mut self, image: Image) {
        self.image = image;
    }
}

impl Helm for Application {
    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.name, self.id), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!(
            "{}/digitalocean/charts/q-application",
            self.context.lib_root_dir()
        )
    }

    fn helm_chart_values_dir(&self) -> String {
        String::new()
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        String::new()
    }
}

impl StatelessService for Application {}

impl Service for Application {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Application
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.image.commit_id.as_str()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn private_port(&self) -> Option<u16> {
        self.private_port
    }

    fn start_timeout(&self) -> Timeout<u32> {
        Timeout::Value(self.start_timeout_in_seconds)
    }

    fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    fn total_instances(&self) -> u16 {
        self.total_instances
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let mut context = default_tera_context(self, kubernetes, environment);
        let commit_id = self.image.commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);

        match &self.image.registry_url {
            Some(registry_url) => context.insert("image_name_with_tag", registry_url.as_str()),
            None => {
                let image_name_with_tag = self.image.name_with_tag();
                warn!("there is no registry url, use image name with tag with the default container registry: {}", image_name_with_tag.as_str());
                context.insert("image_name_with_tag", image_name_with_tag.as_str());
            }
        }

        let environment_variables = self
            .environment_variables
            .iter()
            .map(|ev| EnvironmentVariableDataTemplate {
                key: ev.key.clone(),
                value: ev.value.clone(),
            })
            .collect::<Vec<_>>();

        context.insert("environment_variables", &environment_variables);

        match self.image.registry_name.as_ref() {
            Some(registry_name) => {
                context.insert("is_registry_name", &true);
                context.insert("registry_name", registry_name);
            }
            None => {
                context.insert("is_registry_name", &false);
            }
        };

        let storage = self
            .storage
            .iter()
            .map(|s| StorageDataTemplate {
                id: s.id.clone(),
                name: s.name.clone(),
                storage_type: match s.storage_type {
                    StorageType::Standard => "do-block-storage",
                }
                .to_string(),
                size_in_gib: s.size_in_gib,
                mount_point: s.mount_point.clone(),
                snapshot_retention_in_days: s.snapshot_retention_in_days,
            })
            .collect::<Vec<_>>();

        let is_storage = storage.len() > 0;

        context.insert("storage", &storage);
        context.insert("is_storage", &is_storage);

        context.insert("clone", &false);
        context.insert("start_timeout_in_seconds", &self.start_timeout_in_seconds);

        Ok(context)
    }

    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Application(self.id().to_string(), self.name().to_string())
    }
}

impl Create for Application {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("DO.application.on_create() called for {}", self.name);

        let (kubernetes, _) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let digitalocean = kubernetes
            .cloud_provider()
            .as_any()
            .downcast_ref::<DO>()
            .unwrap();

        // retrieve the cluster uuid, useful to link DO registry to k8s cluster
        let cluster_uuid_res =
            get_uuid_of_cluster_from_name(digitalocean.token.as_str(), kubernetes.name());

        match cluster_uuid_res {
            // ensure DO registry is linked to k8s cluster
            Ok(uuid) => match subscribe_kube_cluster_to_container_registry(
                digitalocean.token.as_str(),
                uuid.as_str(),
            ) {
                Ok(_) => info!("Container registry is well linked with the Cluster"),
                Err(e) => error!("Unable to link cluster to registry {:?}", e.message),
            },
            Err(e) => error!("Unable to get cluster uuid {:?}", e.message),
        };

        deploy_user_stateless_service(target, self)
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "DO.application.on_create_error() called for {}",
            self.name()
        );
        deploy_stateless_service_error(target, self)
    }
}

impl Pause for Application {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("DO.application.on_pause() called for {}", self.name());
        delete_stateless_service(target, self, false)
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("DO.application.on_pause_error() called for {}", self.name());
        delete_stateless_service(target, self, true)
    }
}

impl Delete for Application {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("DO.application.on_delete() called for {}", self.name());
        delete_stateless_service(target, self, false)
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "DO.application.on_delete_error() called for {}",
            self.name()
        );
        delete_stateless_service(target, self, true)
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum StorageType {
    Standard,
}
