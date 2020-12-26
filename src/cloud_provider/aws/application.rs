use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{
    do_stateless_service_cleanup, get_stateless_resource_information, Kubernetes,
};
use crate::cloud_provider::models::{
    EnvironmentVariable, EnvironmentVariableDataTemplate, Storage, StorageDataTemplate,
};
use crate::cloud_provider::service::{
    deploy_application, Action, Application as CApplication, Create, Delete, Helm, Pause, Service,
    ServiceType, StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::error::{cast_simple_error_to_engine_error, EngineError};
use crate::models::Context;

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

    fn context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> TeraContext {
        let mut context = self.default_tera_context(kubernetes, environment);
        let commit_id = self.image().commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);

        match &self.image().registry_url {
            Some(registry_url) => context.insert("image_name_with_tag", registry_url.as_str()),
            None => {
                let image_name_with_tag = self.image().name_with_tag();
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

        let storage = self
            .storage
            .iter()
            .map(|s| StorageDataTemplate {
                id: s.id.clone(),
                name: s.name.clone(),
                storage_type: match s.storage_type {
                    StorageType::SC1 => "sc1",
                    StorageType::ST1 => "st1",
                    StorageType::GP2 => "gp2",
                    StorageType::IO1 => "io1",
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

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }

        context
    }

    fn delete(&self, target: &DeploymentTarget, is_error: bool) -> Result<(), EngineError> {
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let helm_release_name = self.helm_release_name();
        let selector = format!("app={}", self.name());

        if is_error {
            let _ = get_stateless_resource_information(kubernetes, environment, selector.as_str())?;
        }

        // clean the resource
        let _ = do_stateless_service_cleanup(kubernetes, environment, helm_release_name.as_str())?;

        Ok(())
    }
}

impl crate::cloud_provider::service::Application for Application {
    fn image(&self) -> &Image {
        &self.image
    }

    fn set_image(&mut self, image: Image) {
        self.image = image;
    }

    fn start_timeout_in_seconds(&self) -> u32 {
        self.start_timeout_in_seconds
    }
}

impl Helm for Application {
    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.name(), self.id()), 50)
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

impl Create for Application {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.application.on_create() called for {}", self.name());
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let context = self.context(kubernetes, environment);
        let charts_dir = format!("{}/aws/charts/q-application", self.context.lib_root_dir());

        deploy_application(kubernetes, environment, self, charts_dir.as_str(), &context)
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "AWS.application.on_create_error() called for {}",
            self.name()
        );

        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let kubernetes_config_file_path = kubernetes.config_file_path()?;

        let helm_release_name = self.helm_release_name();

        let history_rows = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::cmd::helm::helm_exec_history(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name.as_str(),
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables(),
            ),
        )?;

        if history_rows.len() == 1 {
            cast_simple_error_to_engine_error(
                self.engine_error_scope(),
                self.context.execution_id(),
                crate::cmd::helm::helm_exec_uninstall(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    helm_release_name.as_str(),
                    kubernetes
                        .cloud_provider()
                        .credentials_environment_variables(),
                ),
            )?;
        }

        Ok(())
    }
}

impl Pause for Application {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.application.on_pause() called for {}", self.name());
        self.delete(target, false)
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "AWS.application.on_pause_error() called for {}",
            self.name()
        );
        self.delete(target, true)
    }
}

impl Delete for Application {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.application.on_delete() called for {}", self.name());
        self.delete(target, false)
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "AWS.application.on_delete_error() called for {}",
            self.name()
        );
        self.delete(target, true)
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum StorageType {
    SC1,
    ST1,
    GP2,
    IO1,
}
