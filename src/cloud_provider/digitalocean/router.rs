use retry::delay::Fibonacci;
use retry::OperationResult;
use tera::Context as TeraContext;

use crate::cloud_provider::models::{
    CustomDomain, CustomDomainDataTemplate, Route, RouteDataTemplate,
};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, send_progress_on_long_task, Action, Create,
    Delete, Helm, Pause, Service, ServiceType, StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope,
};
use crate::models::{Context, Listen, Listener, Listeners};

pub struct Router {
    context: Context,
    id: String,
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
        default_domain: &str,
        custom_domains: Vec<CustomDomain>,
        routes: Vec<Route>,
        listeners: Listeners,
    ) -> Self {
        Router {
            context,
            id: id.to_string(),
            name: name.to_string(),
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

    fn version(&self) -> &str {
        "1.0"
    }

    fn action(&self) -> &Action {
        &Action::Create
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
                //context.insert("target_hostname", cd.domain.clone());
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
                    Some(application) => match application.private_port() {
                        Some(private_port) => Some(RouteDataTemplate {
                            path: r.path.clone(),
                            application_name: application.name().to_string(),
                            application_port: private_port,
                        }),
                        _ => None,
                    },
                    _ => None,
                }
            })
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();

        let kubernetes_config_file_path = kubernetes.config_file_path();

        match kubernetes_config_file_path {
            Ok(kubernetes_config_file_path_string) => {
                // Default domain
                let external_ingress_hostname_default =
                    crate::cmd::kubectl::do_kubectl_exec_get_external_ingress_ip(
                        kubernetes_config_file_path_string.as_str(),
                        "nginx-ingress",
                        "app=nginx-ingress,component=controller",
                        kubernetes
                            .cloud_provider()
                            .credentials_environment_variables(),
                    );

                match external_ingress_hostname_default {
                    Ok(external_ingress_hostname_default) => {
                        match external_ingress_hostname_default {
                            Some(hostname) => context
                                .insert("external_ingress_hostname_default", hostname.as_str()),
                            None => {
                                warn!("unable to get external_ingress_hostname_default - what's wrong? This must never happened");
                            }
                        }
                    }
                    _ => {
                        error!("can't fetch external ingress ip");
                    }
                }

                // Check if there is a custom domain first
                if !self.custom_domains.is_empty() {
                    let external_ingress_hostname_custom =
                        crate::cmd::kubectl::do_kubectl_exec_get_external_ingress_ip(
                            kubernetes_config_file_path_string.as_str(),
                            environment.namespace(),
                            "app=nginx-ingress,component=controller",
                            kubernetes
                                .cloud_provider()
                                .credentials_environment_variables(),
                        );

                    match external_ingress_hostname_custom {
                        Ok(external_ingress_hostname_custom) => {
                            match external_ingress_hostname_custom {
                                Some(hostname) => {
                                    context.insert(
                                        "external_ingress_hostname_custom",
                                        hostname.as_str(),
                                    );
                                }
                                None => {
                                    warn!("unable to get external_ingress_hostname_custom - what's wrong? This must never happened");
                                }
                            }
                        }
                        _ => {
                            error!("can't fetch external_ingress_hostname_custom - what's wrong? This must never happened");
                        }
                    }
                    // FIXME app_id to appId
                    context.insert("app_id", kubernetes.id());
                }
            }
            Err(_) => error!(
                "can't fetch kubernetes config file - what's wrong? This must never happened"
            ),
        }

        let router_default_domain_hash =
            crate::crypto::to_sha1_truncate_16(self.default_domain.as_str());

        context.insert("router_default_domain", self.default_domain.as_str());
        context.insert(
            "router_default_domain_hash",
            router_default_domain_hash.as_str(),
        );
        context.insert("custom_domains", &custom_domain_data_templates);
        context.insert("routes", &route_data_templates);
        context.insert("spec_acme_email", "tls@qovery.com"); // TODO CHANGE ME
        context.insert(
            "metadata_annotations_cert_manager_cluster_issuer",
            "letsencrypt-qovery",
        );

        let lets_encrypt_url = match self.context.metadata() {
            Some(meta) => match meta.test {
                Some(true) => "https://acme-staging-v02.api.letsencrypt.org/directory",
                _ => "https://acme-v02.api.letsencrypt.org/directory",
            },
            _ => "https://acme-v02.api.letsencrypt.org/directory",
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
}

impl Helm for Router {
    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("router-{}", self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!(
            "{}/common/charts/nginx-ingress",
            self.context().lib_root_dir()
        )
    }

    fn helm_chart_values_dir(&self) -> String {
        format!(
            "{}/digitalocean/chart_values/nginx-ingress",
            self.context.lib_root_dir()
        )
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
        info!("DigitalOcean.router.on_create() called for {}", self.name());
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let workspace_dir = self.workspace_directory();
        let helm_release_name = self.helm_release_name();

        let kubernetes_config_file_path = kubernetes.config_file_path()?;

        // respect order - getting the context here and not before is mandatory
        // the nginx-ingress must be available to get the external dns target if necessary
        let mut context = self.tera_context(target)?;

        if !self.custom_domains.is_empty() {
            // custom domains? create an NGINX ingress
            info!("setup NGINX ingress for custom domains");

            let into_dir = crate::fs::workspace_directory(
                self.context.workspace_root_dir(),
                self.context.execution_id(),
                "routers/nginx-ingress",
            );

            let _ = cast_simple_error_to_engine_error(
                self.engine_error_scope(),
                self.context.execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    self.helm_chart_values_dir(),
                    into_dir.as_str(),
                    &context,
                ),
            )?;

            let _ = cast_simple_error_to_engine_error(
                self.engine_error_scope(),
                self.context.execution_id(),
                crate::template::copy_non_template_files(self.helm_chart_dir(), into_dir.as_str()),
            )?;

            // do exec helm upgrade and return the last deployment status
            let helm_history_row = cast_simple_error_to_engine_error(
                self.engine_error_scope(),
                self.context.execution_id(),
                crate::cmd::helm::helm_exec_with_upgrade_history_with_override(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    format!("custom-{}", helm_release_name).as_str(),
                    into_dir.as_str(),
                    format!("{}/nginx-ingress.yaml", into_dir.as_str()).as_str(),
                    kubernetes
                        .cloud_provider()
                        .credentials_environment_variables(),
                ),
            )?;

            // check deployment status
            if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    "Router has failed to be deployed".into(),
                ));
            }

            // waiting for the nlb, it should be deploy to get fqdn
            let external_ingress_hostname_custom_result = retry::retry(
                Fibonacci::from_millis(3000).take(10),
                || {
                    let external_ingress_ip_custom =
                        crate::cmd::kubectl::do_kubectl_exec_get_external_ingress_ip(
                            kubernetes_config_file_path.as_str(),
                            environment.namespace(),
                            format!(
                                "{},component=controller,release=custom-{}",
                                self.selector(),
                                helm_release_name
                            )
                            .as_str(),
                            kubernetes
                                .cloud_provider()
                                .credentials_environment_variables(),
                        );

                    match external_ingress_ip_custom {
                        Ok(external_ingress_ip_custom) => {
                            OperationResult::Ok(external_ingress_ip_custom)
                        }
                        Err(err) => {
                            error!(
                                "Waiting Digital Ocean LoadBalancer endpoint to be available to be able to configure TLS"
                            );
                            OperationResult::Retry(err)
                        }
                    }
                },
            );

            match external_ingress_hostname_custom_result {
                Ok(do_lb_ip) => {
                    //put it in the context
                    context.insert("do_lb_ingress_ip", &do_lb_ip);
                }
                Err(_) => error!("Error getting the NLB endpoint to be able to configure TLS"),
            }
        }

        let from_dir = format!(
            "{}/digitalocean/charts/q-ingress-tls",
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
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables(),
            ),
        )?;

        if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
            return Err(self.engine_error(
                EngineErrorCause::Internal,
                "Router has failed to be deployed".into(),
            ));
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        return Ok(());
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("DO.router.on_create_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Create,
            Box::new(|| delete_stateless_service(target, self, true)),
        )
    }
}

impl Pause for Router {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("DO.router.on_pause() called for {}", self.name());

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
        warn!("DO.router.on_pause_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Pause,
            Box::new(|| delete_stateless_service(target, self, true)),
        )
    }
}

impl Delete for Router {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("DO.router.on_delete() called for {}", self.name());

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
        warn!("DO.router.on_delete_error() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Delete,
            Box::new(|| delete_stateless_service(target, self, true)),
        )
    }
}
