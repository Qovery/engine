use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::models::{EnvironmentVariable, EnvironmentVariableDataTemplate};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, deploy_stateless_service_error, deploy_user_stateless_service,
    send_progress_on_long_task, Action, Application as AApplication, Create, Delete, Helm, Pause, Service, ServiceType,
    StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::error::{EngineError, EngineErrorScope};
use crate::models::{Context, Listen, Listener, Listeners};

pub struct ExternalService {
    context: Context,
    id: String,
    action: Action,
    name: String,
    total_cpus: String,
    total_ram_in_mib: u32,
    image: Image,
    environment_variables: Vec<EnvironmentVariable>,
    listeners: Listeners,
}

impl ExternalService {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        total_cpus: String,
        total_ram_in_mib: u32,
        image: Image,
        environment_variables: Vec<EnvironmentVariable>,
        listeners: Listeners,
    ) -> Self {
        ExternalService {
            context,
            id: id.to_string(),
            action,
            name: name.to_string(),
            total_cpus,
            total_ram_in_mib,
            image,
            environment_variables,
            listeners,
        }
    }
}

impl crate::cloud_provider::service::ExternalService for ExternalService {}

impl crate::cloud_provider::service::Application for ExternalService {
    fn image(&self) -> &Image {
        &self.image
    }

    fn set_image(&mut self, image: Image) {
        self.image = image;
    }
}

impl Helm for ExternalService {
    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("external-service-{}-{}", self.name(), self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/common/services/q-job", self.context.lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        String::new()
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        String::new()
    }
}

impl StatelessService for ExternalService {}

impl Service for ExternalService {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::ExternalService
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
        None
    }

    fn start_timeout(&self) -> Timeout<u32> {
        Timeout::Default
    }

    fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    fn total_instances(&self) -> u16 {
        1
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let mut context = default_tera_context(self, kubernetes, environment);
        let commit_id = self.image().commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);

        match &self.image().registry_url {
            Some(registry_url) => context.insert("image_name_with_tag", registry_url.as_str()),
            None => {
                let image_name_with_tag = self.image().name_with_tag();
                warn!(
                    "there is no registry url, use image name with tag with the default container registry: {}",
                    image_name_with_tag.as_str()
                );
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

        Ok(context)
    }

    fn selector(&self) -> String {
        format!("app={}", self.name())
    }

    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::ExternalService(self.id().to_string(), self.name().to_string())
    }
}

impl Create for ExternalService {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.external_service.on_create() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Create,
            Box::new(|| deploy_user_stateless_service(target, self)),
        )
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.external_service.on_create_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Create,
            Box::new(|| deploy_stateless_service_error(target, self)),
        )
    }
}

impl Pause for ExternalService {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.external_service.on_pause() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Pause,
            Box::new(|| delete_stateless_service(target, self, false)),
        )
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.external_service.on_pause_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Pause,
            Box::new(|| delete_stateless_service(target, self, true)),
        )
    }
}

impl Delete for ExternalService {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.external_service.on_delete() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Delete,
            Box::new(|| delete_stateless_service(target, self, false)),
        )
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.external_service.on_delete_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Delete,
            Box::new(|| delete_stateless_service(target, self, true)),
        )
    }
}

impl Listen for ExternalService {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
