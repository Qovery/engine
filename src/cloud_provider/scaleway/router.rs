use tera::Context as TeraContext;

use crate::cloud_provider::models::{CustomDomain, Route};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, send_progress_on_long_task, Action, Create, Delete, Helm, Pause,
    Service, ServiceType, StatelessService,
};
use crate::cloud_provider::utilities::sanitize_name;
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::error::{EngineError, EngineErrorScope};
use crate::models::{Context, Listen, Listener, Listeners};

pub struct Router {
    context: Context,
    id: String,
    action: Action,
    name: String,
    default_domain: String,
    custom_domains: Vec<CustomDomain>,
    routes: Vec<Route>,
    listeners: Listeners,
}

impl Router {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        action: Action,
        default_domain: &str,
        custom_domains: Vec<CustomDomain>,
        routes: Vec<Route>,
        listeners: Listeners,
    ) -> Router {
        Router {
            context,
            id: id.to_string(),
            name: name.to_string(),
            action,
            default_domain: default_domain.to_string(),
            custom_domains,
            routes,
            listeners,
        }
    }
}

impl Service for Router {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Router
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn sanitized_name(&self) -> String {
        sanitize_name("router", self.name())
    }

    fn version(&self) -> &str {
        "1.0"
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
        "1".to_string()
    }

    fn cpu_burst(&self) -> String {
        unimplemented!()
    }

    fn total_ram_in_mib(&self) -> u32 {
        1
    }

    fn total_instances(&self) -> u16 {
        1
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };
        let context = default_tera_context(self, kubernetes, environment);

        Ok(context)
    }

    fn selector(&self) -> String {
        "app=nginx-ingress".to_string()
    }

    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Router(self.id().to_string(), self.name().to_string())
    }
}

impl crate::cloud_provider::service::Router for Router {
    fn domains(&self) -> Vec<&str> {
        let mut _domains = vec![self.default_domain.as_str()];

        for domain in &self.custom_domains {
            _domains.push(domain.domain.as_str());
        }

        _domains
    }

    fn has_custom_domains(&self) -> bool {
        !self.custom_domains.is_empty()
    }
}

impl Helm for Router {
    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("router-{}", self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/nginx-ingress", self.context().lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        format!("{}/scaleway/chart_values/nginx-ingress", self.context.lib_root_dir())
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        String::new()
    }
}

impl Listen for Router {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

impl StatelessService for Router {}

impl Create for Router {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("Scaleway.router.on_create() called for {}", self.name());
        let (kubernetes, _environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let _workspace_dir = self.workspace_directory();
        let _helm_release_name = self.helm_release_name();

        let _kubernetes_config_file_path = kubernetes.config_file_path()?;

        // respect order - getting the context here and not before is mandatory
        // the nginx-ingress must be available to get the external dns target if necessary
        let _context = self.tera_context(target)?;

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        use crate::cloud_provider::service::Router;

        // check non custom domains
        self.check_domains()?;

        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("SCW.router.on_create_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Create,
            Box::new(|| delete_stateless_service(target, self, true)),
        )
    }
}

impl Pause for Router {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("SCW.router.on_pause() called for {}", self.name());

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
        warn!("SCW.router.on_pause_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Pause,
            Box::new(|| delete_stateless_service(target, self, true)),
        )
    }
}

impl Delete for Router {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("SCW.router.on_delete() called for {}", self.name());

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
        warn!("SCW.router.on_delete_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Delete,
            Box::new(|| delete_stateless_service(target, self, true)),
        )
    }
}
