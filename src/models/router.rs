use crate::cloud_provider::models::{CustomDomain, CustomDomainDataTemplate, Route, RouteDataTemplate};
use crate::cloud_provider::service::{default_tera_context, Action, Service, ServiceType};
use crate::cloud_provider::utilities::sanitize_name;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventMessage, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::progress_listener::{Listener, Listeners};
use crate::logger::Logger;
use crate::models::types::CloudProvider;
use crate::models::types::ToTeraContext;
use crate::utilities::to_short_id;
use std::borrow::Borrow;
use std::marker::PhantomData;
use tera::Context as TeraContext;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum RouterError {
    #[error("Router invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct RouterAdvancedSettings {
    pub custom_domain_check_enabled: bool,
}

pub struct Router<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(crate) context: Context,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) action: Action,
    pub(crate) name: String,
    pub(crate) default_domain: String,
    pub(crate) custom_domains: Vec<CustomDomain>,
    pub(crate) sticky_sessions_enabled: bool,
    pub(crate) routes: Vec<Route>,
    pub(crate) listeners: Listeners,
    pub(crate) logger: Box<dyn Logger>,
    pub(crate) _extra_settings: T::RouterExtraSettings,
    pub(crate) advanced_settings: RouterAdvancedSettings,
}

impl<T: CloudProvider> Router<T> {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        action: Action,
        default_domain: &str,
        custom_domains: Vec<CustomDomain>,
        routes: Vec<Route>,
        sticky_sessions_enabled: bool,
        extra_settings: T::RouterExtraSettings,
        advanced_settings: RouterAdvancedSettings,
        listeners: Listeners,
        logger: Box<dyn Logger>,
    ) -> Result<Self, RouterError> {
        Ok(Self {
            _marker: PhantomData,
            context,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            action,
            default_domain: default_domain.to_string(),
            custom_domains,
            sticky_sessions_enabled,
            routes,
            listeners,
            logger,
            _extra_settings: extra_settings,
            advanced_settings,
        })
    }

    fn selector(&self) -> Option<String> {
        Some(format!("routerId={}", self.id))
    }

    pub(crate) fn default_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError>
    where
        Self: Service,
    {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);

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

        // FIXME: For now we only support one public port per application
        // it should be possible add more if we prefix port_name in front of the fqdn
        // (i.e: p4242.zxxxx-zzzzz.cluster.mydomain.com)
        let route_data_templates = self
            .routes
            .iter()
            .filter_map(|r| {
                if let Some(application) = &environment
                    .applications
                    .iter()
                    .find(|app| app.long_id() == &r.service_long_id)
                {
                    if let Some(public_port) = application.public_port() {
                        return Some(RouteDataTemplate {
                            path: r.path.clone(),
                            application_name: application.sanitized_name(),
                            application_port: public_port,
                        });
                    }
                }

                if let Some(container) = &environment
                    .containers
                    .iter()
                    .find(|container| container.long_id() == &r.service_long_id)
                {
                    if let Some(public_port) = container.public_port() {
                        return Some(RouteDataTemplate {
                            path: r.path.clone(),
                            application_name: container.kube_service_name(),
                            application_port: public_port,
                        });
                    }
                }

                None
            })
            .collect::<Vec<_>>();

        // autoscaler
        context.insert("nginx_enable_horizontal_autoscaler", "false");
        context.insert("nginx_minimum_replicas", "1");
        context.insert("nginx_maximum_replicas", "10");
        // resources
        context.insert("nginx_requests_cpu", "200m");
        context.insert("nginx_requests_memory", "128Mi");
        context.insert("nginx_limit_cpu", "200m");
        context.insert("nginx_limit_memory", "128Mi");

        let kubernetes_config_file_path = kubernetes.get_kubeconfig_file_path()?;

        // Default domain
        match crate::cmd::kubectl::kubectl_exec_get_external_ingress_hostname(
            kubernetes_config_file_path,
            "nginx-ingress",
            "nginx-ingress-ingress-nginx-controller",
            kubernetes.cloud_provider().credentials_environment_variables(),
        ) {
            Ok(external_ingress_hostname_default) => match external_ingress_hostname_default {
                Some(hostname) => context.insert("external_ingress_hostname_default", hostname.as_str()),
                None => {
                    // TODO(benjaminch): Handle better this one via a proper error eventually
                    self.logger().log(EngineEvent::Warning(
                        event_details,
                        EventMessage::new_from_safe(
                            "Error while trying to get Load Balancer hostname from Kubernetes cluster".to_string(),
                        ),
                    ));
                }
            },
            _ => {
                // FIXME really?
                // TODO(benjaminch): Handle better this one via a proper error eventually
                self.logger().log(EngineEvent::Warning(
                    event_details,
                    EventMessage::new_from_safe("Can't fetch external ingress hostname.".to_string()),
                ));
            }
        }

        let router_default_domain_hash = crate::crypto::to_sha1_truncate_16(self.default_domain.as_str());

        // TODO(benjaminch): remove this one once subdomain migration has been done, CF ENG-1302
        // If domain contains cluster id in it, it means cluster has already declared wildcard and there is no need to declare app domain since it can use the cluster wildcard.
        let router_should_declare_domain_to_external_dns = !self
            .default_domain
            .contains(format!(".{}.", self.context().cluster_id()).as_str());

        let tls_domain = kubernetes.dns_provider().domain().wildcarded();
        context.insert("router_tls_domain", tls_domain.to_string().as_str());
        context.insert("router_default_domain", self.default_domain.as_str());
        context.insert("router_default_domain_hash", router_default_domain_hash.as_str());
        context.insert(
            "router_should_declare_domain_to_external_dns",
            &router_should_declare_domain_to_external_dns,
        );
        context.insert("custom_domains", &custom_domain_data_templates);
        context.insert("routes", &route_data_templates);
        context.insert("spec_acme_email", "tls@qovery.com"); // TODO CHANGE ME
        context.insert("metadata_annotations_cert_manager_cluster_issuer", "letsencrypt-qovery");

        let lets_encrypt_url = match self.context.is_test_cluster() {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("spec_acme_server", lets_encrypt_url);

        // Nginx
        context.insert("sticky_sessions_enabled", &self.sticky_sessions_enabled);

        // ingress advanced settings
        // 1 app == 1 ingress, we filter only on the app to retrieve advanced settings
        if let Some(route) = self.routes.first() {
            if let Some(advanced_settings) = environment
                .applications
                .iter()
                .find(|app| app.long_id() == &route.service_long_id)
                .map(|app| app.advanced_settings())
            {
                context.insert("advanced_settings", &advanced_settings);
            }

            if let Some(advanced_settings) = environment
                .containers
                .iter()
                .find(|app| app.long_id() == &route.service_long_id)
                .map(|app| app.advanced_settings())
            {
                context.insert("advanced_settings", &advanced_settings);
            }
        };

        Ok(context)
    }

    pub fn helm_release_name(&self) -> String {
        crate::string::cut(format!("router-{}", self.id), 50)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!(
            "{}/{}/charts/q-ingress-tls",
            self.context.lib_root_dir(),
            T::lib_directory_name()
        )
    }
}

impl<T: CloudProvider> Service for Router<T> {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Router
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn sanitized_name(&self) -> String {
        sanitize_name("router", self.id())
    }

    fn version(&self) -> String {
        "1.0".to_string()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn selector(&self) -> Option<String> {
        self.selector()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    fn to_transmitter(&self) -> Transmitter {
        Transmitter::Router(self.id.to_string(), self.name.to_string())
    }

    fn as_service(&self) -> &dyn Service {
        self
    }
}

pub trait RouterService: Service + DeploymentAction + ToTeraContext {
    /// all domains (auto-generated by Qovery and user custom domains) associated to the router
    fn has_custom_domains(&self) -> bool;
}

impl<T: CloudProvider> RouterService for Router<T>
where
    Router<T>: Service + ToTeraContext,
{
    fn has_custom_domains(&self) -> bool {
        !self.custom_domains.is_empty()
    }
}
