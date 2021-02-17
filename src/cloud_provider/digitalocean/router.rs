use retry::delay::Fixed;
use retry::OperationResult;
use tera::Context as TeraContext;

use crate::cloud_provider::digitalocean::common::do_get_load_balancer_ip;
use crate::cloud_provider::digitalocean::DO;
use crate::cloud_provider::models::{CustomDomain, CustomDomainDataTemplate, Route, RouteDataTemplate};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, send_progress_on_long_task, Action, Create, Delete, Helm, Pause,
    Service, ServiceType, StatelessService,
};
use crate::cloud_provider::utilities::{check_cname_for, sanitize_name};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope, SimpleError, SimpleErrorKind,
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
        //TODO Quick fix, see to avoid doing sanitize app in the router
        let routes = routes
            .into_iter()
            .map(|mut r| {
                r.application_name = sanitize_name("app", r.application_name.as_str());
                r
            })
            .collect();

        Router {
            context,
            id: id.to_string(),
            name: sanitize_name("router", name),
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

        let digitalocean = kubernetes.cloud_provider().as_any().downcast_ref::<DO>().unwrap();

        let mut context = default_tera_context(self, kubernetes, environment);

        let applications = environment
            .stateless_services
            .iter()
            .filter(|x| x.service_type() == ServiceType::Application)
            .collect::<Vec<_>>();

        // it's a loop, but we can manage only one custom domain at a time. DO do not support more because of LB limitations
        // we'll have to change it in the future, not urgent
        let custom_domain_data_templates = self
            .custom_domains
            .iter()
            .map(|cd| {
                let domain_hash = crate::crypto::to_sha1_truncate_16(cd.domain.as_str());

                // https://github.com/digitalocean/digitalocean-cloud-controller-manager/issues/291
                // Can only manage 1 host at a time on an DO load balancer
                context.insert("custom_domain_name", cd.domain.as_str());

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
                let external_ingress_hostname_default = crate::cmd::kubectl::do_kubectl_exec_get_external_ingress_ip(
                    kubernetes_config_file_path_string.as_str(),
                    "nginx-ingress",
                    "app=nginx-ingress,component=controller",
                    kubernetes.cloud_provider().credentials_environment_variables(),
                );

                match external_ingress_hostname_default {
                    Ok(external_ingress_hostname_default) => match external_ingress_hostname_default {
                        Some(hostname) => context.insert("external_ingress_hostname_default", hostname.as_str()),
                        None => {
                            return Err(self.engine_error(
                                EngineErrorCause::Internal,
                                "Error while trying to get Load Balancer IP from Kubernetes cluster".into(),
                            ));
                        }
                    },
                    _ => {
                        error!("can't fetch external ingress ip");
                    }
                }

                // Custom domain
                if !self.custom_domains.is_empty() {
                    let external_ingress_ip_selector =
                        format!("app=nginx-ingress,component=controller,app_id={}", self.id());

                    let deployed_ingress = match crate::cmd::kubectl::do_kubectl_exec_get_external_ingress_ip(
                        kubernetes_config_file_path_string.as_str(),
                        environment.namespace(),
                        external_ingress_ip_selector.as_str(),
                        kubernetes.cloud_provider().credentials_environment_variables(),
                    ) {
                        Ok(x) => x.is_some(),
                        _ => false,
                    };

                    if deployed_ingress {
                        let do_load_balancer_ip = retry::retry(Fixed::from_millis(5000).take(40), || {
                            // we first need to retrieve the id from the nginx ingress service
                            let lb_id = crate::cmd::kubectl::do_kubectl_exec_get_loadbalancer_id(
                                kubernetes_config_file_path_string.as_str(),
                                environment.namespace(),
                                external_ingress_ip_selector.as_str(),
                                kubernetes.cloud_provider().credentials_environment_variables(),
                            );

                            // then we can get the DO Load Balancer IP address which will be used in the custom ingress for the app
                            match lb_id {
                                Ok(id) => match id {
                                    Some(id) => match do_get_load_balancer_ip(&digitalocean.token, id.as_str()) {
                                        Ok(ip) => {
                                            info!("Got the IP {}", &ip);
                                            OperationResult::Ok(ip)
                                        }
                                        Err(e) => {
                                            error!("Error while trying to get Load Balancer IP from Digital Ocean API, mandatory for requirements");
                                            OperationResult::Retry(SimpleError::new(
                                                SimpleErrorKind::Other,
                                                Some(format!("Error while trying to get Load Balancer IP from Digital Ocean API, mandatory for requirements. {:?}", e)),
                                            ))
                                        }
                                    },
                                    None => {
                                        error!("No Load Balancer id from Digital Ocean API was found, mandatory for custom ingress");
                                        OperationResult::Retry(SimpleError::new(
                                            SimpleErrorKind::Other,
                                            Some("No Load Balancer id from Digital Ocean API was found, mandatory for custom ingress"),
                                        ))
                                    }
                                },
                                Err(_) => {
                                    info!("Can't get Load Balancer id from Digital Ocean API, load balancer may be not ready yet or not yet deployed");
                                    OperationResult::Retry(SimpleError::new(
                                        SimpleErrorKind::Other,
                                        Some("Can't get Load Balancer id from Digital Ocean API, load balancer may be not ready yet or not yet deployed"),
                                    ))
                                }
                            }
                        });
                        match do_load_balancer_ip {
                            Err(_) => {
                                // "Error while trying to get Load Balancer IP from Digital Ocean API, mandatory for requirements. SimpleError { kind: Other, message: Some("Error While trying to deserialize json received from Digital Ocean Load Balancer API: missing field `id` at line 1 column 1926") }"
                                return Err(self.engine_error(
                                    EngineErrorCause::Internal,
                                    "Wasn't able to get load balancer info, stopping now as ingress rendering will fail".into(),
                                ));
                            }
                            Ok(ip) => context.insert("do_lb_ingress_ip", &ip.to_string()),
                        }
                    };

                    // FIXME app_id to appId
                    context.insert("app_id", kubernetes.id());
                }
            }
            Err(_) => error!("can't fetch kubernetes config file - what's wrong? This must never happened"),
        }

        let router_default_domain_hash = crate::crypto::to_sha1_truncate_16(self.default_domain.as_str());

        let tls_domain = format!("*.{}", kubernetes.dns_provider().domain());
        context.insert("router_tls_domain", tls_domain.as_str());
        context.insert("router_default_domain", self.default_domain.as_str());
        context.insert("router_default_domain_hash", router_default_domain_hash.as_str());
        context.insert("custom_domains", &custom_domain_data_templates);
        context.insert("routes", &route_data_templates);
        context.insert("spec_acme_email", "tls@qovery.com"); // TODO CHANGE ME
        context.insert("metadata_annotations_cert_manager_cluster_issuer", "letsencrypt-qovery");

        let lets_encrypt_url = match self.context.metadata() {
            Some(meta) => match meta.test {
                Some(true) => "https://acme-staging-v02.api.letsencrypt.org/directory",
                _ => "https://acme-v02.api.letsencrypt.org/directory",
            },
            _ => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("spec_acme_server", lets_encrypt_url);

        eprintln!("{}", context.clone().into_json());

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
        format!("{}/common/charts/nginx-ingress", self.context().lib_root_dir())
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

        // custom domain
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
                    kubernetes.cloud_provider().credentials_environment_variables(),
                ),
            )?;

            // check deployment status
            if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
                return Err(self.engine_error(EngineErrorCause::Internal, "Router has failed to be deployed".into()));
            }

            // waiting for the load balancer, it should be deploy to get fqdn
            let _ = retry::retry(Fixed::from_millis(3000).take(60), || {
                let external_ingress_ip_custom = crate::cmd::kubectl::do_kubectl_exec_get_external_ingress_ip(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    format!(
                        "{},component=controller,release=custom-{}",
                        self.selector(),
                        helm_release_name
                    )
                    .as_str(),
                    kubernetes.cloud_provider().credentials_environment_variables(),
                );

                match external_ingress_ip_custom {
                    Ok(external_ingress_ip_custom) => OperationResult::Ok(external_ingress_ip_custom),
                    Err(err) => {
                        error!(
                            "Waiting Digital Ocean LoadBalancer endpoint to be available to be able to configure TLS"
                        );
                        OperationResult::Retry(err)
                    }
                }
            });
        }

        // re-run context to get get lb ip address to use it then in the ingress
        context = self.tera_context(target)?;

        let from_dir = format!("{}/digitalocean/charts/q-ingress-tls", self.context.lib_root_dir());
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
        use crate::cloud_provider::service::Router;

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
                    continue;
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
