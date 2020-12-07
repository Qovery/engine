use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::digitalocean::common::get_uuid_of_cluster_from_name;
use crate::cloud_provider::digitalocean::{common, DO};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Action, Application as CApplication, Create, Delete, Pause, Service, ServiceType,
    StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::structs::{HelmHistoryRow, LabelsContent};
use crate::constants::DIGITAL_OCEAN_TOKEN;
use crate::container_registry::docr::{
    get_current_registry_name, subscribe_kube_cluster_to_container_registry,
};
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope, SimpleError,
};
use crate::models::Context;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize)]
struct EnvironmentVariableDataTemplate {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
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
            environment_variables,
        }
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.name, self.id), 50)
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("applications/{}", self.name),
        )
    }

    fn context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> TeraContext {
        let mut context = self.default_tera_context(kubernetes, environment);
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

        // retreive the registry name
        let digitalocean = kubernetes
            .cloud_provider()
            .as_any()
            .downcast_ref::<DO>()
            .unwrap();

        let current_registry_name = get_current_registry_name(&digitalocean.token);
        match current_registry_name {
            Ok(registry_name) => context.insert("registry_name", &registry_name),
            _ => error!("Unable to fetch the registry name !"),
        }
        let is_storage = false;
        context.insert("is_storage", &is_storage);

        context.insert("clone", &false);
        context.insert("start_timeout_in_seconds", &self.start_timeout_in_seconds);

        context
    }
}

impl Create for Application {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!(
            "DigitalOcean.application.on_create() called for {}",
            self.name
        );

        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let digitalocean = kubernetes
            .cloud_provider()
            .as_any()
            .downcast_ref::<DO>()
            .unwrap();

        let context = self.context(kubernetes, environment);
        let workspace_dir = self.workspace_directory();

        // retrieve the cluster uuid, useful to link DO registry to k8s cluster
        let kube_name = kubernetes.name();
        let cluster_uuid_res =
            get_uuid_of_cluster_from_name(digitalocean.token.as_str(), kube_name);
        match cluster_uuid_res {
            // ensure DO registry is linked to k8s cluster
            Ok(uuid) => match subscribe_kube_cluster_to_container_registry(
                digitalocean.token.as_str(),
                uuid.as_str(),
            ) {
                Ok(_) => info!("Container registry is well linked with the Cluster "),
                Err(e) => error!("Unable to link cluster to registry {:?}", e.message),
            },
            Err(e) => error!("Unable to get cluster uuid {:?}", e.message),
        };

        let from_dir = format!(
            "{}/digitalocean/charts/q-application",
            self.context.lib_root_dir()
        );

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                from_dir.as_str(),
                workspace_dir.as_str(),
                &context,
            ),
        )?;

        let kubeconfig_path = common::kubernetes_config_path(
            workspace_dir.as_str(),
            kubernetes.id(),
            digitalocean.region.as_str(),
            digitalocean.spaces_secret_key.as_str(),
            digitalocean.spaces_access_id.as_str(),
        );

        // define labels to add to namespace
        let namespace_labels = match self.context.resource_expiration_in_seconds() {
            Some(v) => Some(vec![
                (LabelsContent {
                    name: "ttl".to_string(),
                    value: format! {"{}", self.context.resource_expiration_in_seconds().unwrap()},
                }),
            ]),
            None => None,
        };

        match kubeconfig_path {
            Ok(path) => {
                let helm_release_name = self.helm_release_name();
                let digitalocean_envs = vec![(DIGITAL_OCEAN_TOKEN, digitalocean.token.as_str())];

                // create a namespace with labels if do not exists
                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::cmd::kubectl::kubectl_exec_create_namespace(
                        path.as_str(),
                        environment.namespace(),
                        namespace_labels,
                        digitalocean_envs.clone(),
                    ),
                )?;

                match crate::cmd::helm::helm_exec_with_upgrade_history(
                    path.as_str(),
                    environment.namespace(),
                    helm_release_name.as_str(),
                    workspace_dir.as_str(),
                    Timeout::Value(self.start_timeout_in_seconds),
                    digitalocean_envs.clone(),
                ) {
                    Ok(upgrade) => {
                        let selector = format!("app={}", self.name());
                        crate::cmd::kubectl::kubectl_exec_is_pod_ready_with_retry(
                            path.as_str(),
                            environment.namespace(),
                            selector.as_str(),
                            digitalocean_envs.clone(),
                        );
                    }
                    Err(e) => error!("Helm upgrade {:?}", e.message),
                }
            }
            Err(e) => error!("Retreiving the kubeconfig {:?}", e.message),
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}

impl Delete for Application {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}

impl Pause for Application {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
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

    fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    fn total_instances(&self) -> u16 {
        self.total_instances
    }
}
