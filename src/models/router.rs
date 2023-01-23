use crate::build_platform::Build;
use crate::cloud_provider::models::{CustomDomain, CustomDomainDataTemplate, HostDataTemplate, Route};
use crate::cloud_provider::service::{default_tera_context, Action, Service, ServiceType};
use crate::cloud_provider::utilities::sanitize_name;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::models::types::CloudProvider;
use crate::models::types::ToTeraContext;
use crate::utilities::to_short_id;
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
    pub whitelist_source_range: String,
}

pub struct Router<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails>,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) action: Action,
    pub(crate) name: String,
    pub(crate) default_domain: String,
    pub(crate) custom_domains: Vec<CustomDomain>,
    pub(crate) routes: Vec<Route>,
    pub(crate) _extra_settings: T::RouterExtraSettings,
    pub(crate) advanced_settings: RouterAdvancedSettings,
    pub(super) workspace_directory: String,
    pub(super) lib_root_directory: String,
}

impl<T: CloudProvider> Router<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: &str,
        action: Action,
        default_domain: &str,
        custom_domains: Vec<CustomDomain>,
        routes: Vec<Route>,
        extra_settings: T::RouterExtraSettings,
        advanced_settings: RouterAdvancedSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, RouterError> {
        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("databases/{}", long_id),
        )
        .map_err(|_| RouterError::InvalidConfig("Can't create workspace directory".to_string()))?;

        let event_details = mk_event_details(Transmitter::Router(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            action,
            default_domain: default_domain.to_string(),
            custom_domains,
            routes,
            _extra_settings: extra_settings,
            advanced_settings,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
    }

    fn selector(&self) -> Option<String> {
        Some(format!("routerId={}", self.id))
    }

    pub fn workspace_directory(&self) -> &str {
        &self.workspace_directory
    }

    pub(crate) fn default_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>>
    where
        Self: Service,
    {
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));

        let cluster_domain = kubernetes.dns_provider().domain().to_string();
        let client_custom_domains = self
            .custom_domains
            .iter()
            // we filter out domain that belongs to our cluster, we dont need to create certificate for them
            .filter(|domain| !domain.domain.ends_with(&cluster_domain))
            .map(|cd| CustomDomainDataTemplate {
                domain: cd.domain.clone(),
            })
            .collect::<Vec<_>>();

        // We can only have 1 router per application/container.
        // Core never mix multiple services inside one router
        let service_id = self
            .routes
            .first()
            .ok_or_else(|| EngineError::new_router_failed_to_deploy(event_details.clone()))?
            .service_long_id;

        // Check if the service is an application
        let (service_name, ports, sticky_session_enabled) =
            if let Some(application) = &environment.applications.iter().find(|app| app.long_id() == &service_id) {
                (
                    application.sanitized_name(),
                    application.public_ports(),
                    application.advanced_settings().network_ingress_sticky_session_enable,
                )
            } else {
                let container = environment
                    .containers
                    .iter()
                    .find(|container| container.long_id() == &service_id)
                    .ok_or_else(|| EngineError::new_router_failed_to_deploy(event_details))?;

                (
                    container.kube_service_name(),
                    container.public_ports(),
                    container.advanced_settings().network_ingress_sticky_session_enable,
                )
            };

        // (custom_domain + default_domain) * (ports + default_port)
        let mut hosts: Vec<HostDataTemplate> = Vec::with_capacity((self.custom_domains.len() + 1) * (ports.len() + 1));
        for port in ports {
            hosts.push(HostDataTemplate {
                domain_name: format!("p{}-{}", port.port, self.default_domain),
                service_name: service_name.clone(),
                service_port: port.port,
            });

            if port.is_default {
                hosts.push(HostDataTemplate {
                    domain_name: self.default_domain.clone(),
                    service_name: service_name.clone(),
                    service_port: port.port,
                });
            }

            for custom_domain in &self.custom_domains {
                hosts.push(HostDataTemplate {
                    domain_name: format!("p{}.{}", port.port, custom_domain.domain),
                    service_name: service_name.clone(),
                    service_port: port.port,
                });

                if port.is_default {
                    hosts.push(HostDataTemplate {
                        domain_name: custom_domain.domain.clone(),
                        service_name: service_name.clone(),
                        service_port: port.port,
                    });
                }
            }
        }

        // whitelist source ranges
        if self.advanced_settings.whitelist_source_range.contains("0.0.0.0") {
            // if whitelist source range contains 0.0.0.0, then we don't need to add the whitelist source range
            context.insert("whitelist_source_range_enabled", &false);
        } else {
            context.insert("whitelist_source_range_enabled", &true);
        }

        // autoscaler
        context.insert("nginx_enable_horizontal_autoscaler", "false");
        context.insert("nginx_minimum_replicas", "1");
        context.insert("nginx_maximum_replicas", "10");
        // resources
        context.insert("nginx_requests_cpu", "200m");
        context.insert("nginx_requests_memory", "128Mi");
        context.insert("nginx_limit_cpu", "200m");
        context.insert("nginx_limit_memory", "128Mi");

        // TODO(benjaminch): remove this one once subdomain migration has been done, CF ENG-1302
        // If domain contains cluster id in it, it means cluster has already declared wildcard and there is no need to declare app domain since it can use the cluster wildcard.
        let router_should_declare_domain_to_external_dns = !self
            .default_domain
            .contains(format!(".{}.", target.kubernetes.id()).as_str());

        let tls_domain = kubernetes.dns_provider().domain().wildcarded();
        context.insert("router_tls_domain", tls_domain.to_string().as_str());
        context.insert("router_default_domain", self.default_domain.as_str());
        context.insert(
            "router_should_declare_domain_to_external_dns",
            &router_should_declare_domain_to_external_dns,
        );
        context.insert("client_custom_domains", &client_custom_domains);
        context.insert("hosts", &hosts);
        context.insert("spec_acme_email", "tls@qovery.com"); // TODO CHANGE ME
        context.insert("metadata_annotations_cert_manager_cluster_issuer", "letsencrypt-qovery");

        let lets_encrypt_url = match target.is_test_cluster {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("spec_acme_server", lets_encrypt_url);

        // Nginx
        context.insert("sticky_sessions_enabled", &sticky_session_enabled);

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
        format!("{}/common/charts/q-ingress-tls", self.lib_root_directory,)
    }
}

impl<T: CloudProvider> Service for Router<T> {
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

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn selector(&self) -> Option<String> {
        self.selector()
    }

    fn as_service(&self) -> &dyn Service {
        self
    }

    fn as_service_mut(&mut self) -> &mut dyn Service {
        self
    }

    fn build(&self) -> Option<&Build> {
        None
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        None
    }
}

pub trait RouterService: Service + DeploymentAction + ToTeraContext {
    /// all domains (auto-generated by Qovery and user custom domains) associated to the router
    fn has_custom_domains(&self) -> bool;

    fn as_deployment_action(&self) -> &dyn DeploymentAction;
}

impl<T: CloudProvider> RouterService for Router<T>
where
    Router<T>: Service + ToTeraContext,
{
    fn has_custom_domains(&self) -> bool {
        !self.custom_domains.is_empty()
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }
}
