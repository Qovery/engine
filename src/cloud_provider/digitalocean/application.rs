use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::digitalocean::{common, DO};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Action, Application as CApplication, Create, Delete, Pause, Service, ServiceType,
    StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::constants::DIGITAL_OCEAN_TOKEN;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope,
};
use crate::models::Context;
use tracing::{event, span, Level};
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
        // TODO use default_tera_context instead
        let mut context = TeraContext::new();
        let commit_id = self.image.commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);

        match &self.image.registry_url {
            Some(registry_url) => context.insert("image_name_with_tag", registry_url.as_str()),
            None => {
                let image_name_with_tag = self.image.name_with_tag();
                event!(Level::WARN,"there is no registry url, use image name with tag with the default container registry: {}", image_name_with_tag.as_str());
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

        //TODO: no storage for the moment
        context.insert("clone", &false);
        context.insert("start_timeout_in_seconds", &self.start_timeout_in_seconds);

        context
    }
}

impl Create for Application {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        event!(
            Level::INFO,
            "DigitalOcean.application.on_create() called for {}",
            self.name
        );
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
