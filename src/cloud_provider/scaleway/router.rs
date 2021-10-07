use tera::Context as TeraContext;

use crate::cloud_provider::models::{CustomDomain, CustomDomainDataTemplate, Route, RouteDataTemplate};
use crate::cloud_provider::service::{
    default_tera_context, delete_router, delete_stateless_service, send_progress_on_long_task, Action, Create, Delete,
    Helm, Pause, Router as RRouter, Service, ServiceType, StatelessService,
};
use crate::cloud_provider::utilities::{check_cname_for, sanitize_name};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope};
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

    fn version(&self) -> String {
        "1.0".to_string()
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

        let mut context = default_tera_context(self, kubernetes, environment);

        let applications = environment
            .stateless_services
            .iter()
            .filter(|x| x.service_type() == ServiceType::Application)
            .collect::<Vec<_>>();

        let custom_domain_data_templates = self
            .custom_domains
            .iter()
            .map(|cd| {
                let domain_hash = crate::crypto::to_sha1_truncate_16(cd.domain.as_str());
                CustomDomainDataTemplate {
                    domain: cd.domain.clone(),
                    domain_hash,
                    target_domain: cd.target_domain.clone(),
                }
            })
            .collect::<Vec<_>>();

        let route_data_templates = self
            .routes
            .iter()
            .map(|r| {
                match applications
                    .iter()
                    .find(|app| app.name() == r.application_name.as_str())
                {
                    Some(application) => application.private_port().map(|private_port| RouteDataTemplate {
                        path: r.path.clone(),
                        application_name: application.sanitized_name(),
                        application_port: private_port,
                    }),
                    _ => None,
                }
            })
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();

        let router_default_domain_hash = crate::crypto::to_sha1_truncate_16(self.default_domain.as_str());

        let tls_domain = format!("*.{}", kubernetes.dns_provider().domain());
        context.insert("router_tls_domain", tls_domain.as_str());
        context.insert("router_default_domain", self.default_domain.as_str());
        context.insert("router_default_domain_hash", router_default_domain_hash.as_str());
        context.insert("custom_domains", &custom_domain_data_templates);
        context.insert("routes", &route_data_templates);
        context.insert("spec_acme_email", "tls@qovery.com"); // TODO CHANGE ME
        context.insert("metadata_annotations_cert_manager_cluster_issuer", "letsencrypt-qovery");

        let lets_encrypt_url = match self.context.is_test_cluster() {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("spec_acme_server", lets_encrypt_url);

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
        format!("{}/common/charts/ingress-nginx", self.context().lib_root_dir())
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
        info!("SCW.router.on_create() called for {}", self.name());
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let workspace_dir = self.workspace_directory();
        let helm_release_name = self.helm_release_name();

        let kubernetes_config_file_path = kubernetes.config_file_path()?;

        // respect order - getting the context here and not before is mandatory
        // the nginx-ingress must be available to get the external dns target if necessary
        let context = self.tera_context(target)?;

        let from_dir = format!("{}/scaleway/charts/q-ingress-tls", self.context.lib_root_dir());
        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(from_dir.as_str(), workspace_dir.as_str(), &context),
        )?;

        // do exec helm upgrade and return the last deployment status
        let helm_history_row = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::cmd::helm::helm_exec_with_upgrade_history(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name.as_str(),
                workspace_dir.as_str(),
                Timeout::Default,
                kubernetes.cloud_provider().credentials_environment_variables(),
            ),
        )?;

        if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
            return Err(self.engine_error(EngineErrorCause::Internal, "Router has failed to be deployed".into()));
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        // check non custom domains
        self.check_domains()?;

        // Wait/Check that custom domain is a CNAME targeting qovery
        for domain_to_check in self.custom_domains.iter() {
            match check_cname_for(
                self.progress_scope(),
                self.listeners(),
                &domain_to_check.domain,
                self.context.execution_id(),
            ) {
                Ok(cname) if cname.trim_end_matches('.') == domain_to_check.target_domain.trim_end_matches('.') => {
                    continue
                }
                Ok(err) | Err(err) => {
                    warn!(
                        "Invalid CNAME for {}. Might not be an issue if user is using a CDN: {}",
                        domain_to_check.domain, err
                    );
                }
            }
        }

        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("SCW.router.on_create_error() called for {}", self.name());

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            delete_stateless_service(target, self, true)
        })
    }
}

impl Pause for Router {
    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("SCW.router.on_pause() called for {}", self.name());
        Ok(())
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("SCW.router.on_pause_error() called for {}", self.name());
        Ok(())
    }
}

impl Delete for Router {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("SCW.router.on_delete() called for {}", self.name());
        delete_router(target, self, false)
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("SCW.router.on_delete_error() called for {}", self.name());
        delete_router(target, self, true)
    }
}
