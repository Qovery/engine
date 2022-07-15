use crate::cloud_provider::helm::ChartInfo;
use crate::cloud_provider::models::{CustomDomain, CustomDomainDataTemplate, Route, RouteDataTemplate};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, Action, Helm, Service, ServiceType,
};
use crate::cloud_provider::utilities::{check_cname_for, check_domain_for, print_action, sanitize_name};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm;
use crate::cmd::helm::to_engine_error;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::router::reporter::RouterDeploymentReporter;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage, ToTransmitter, Transmitter};
use crate::io_models::{ApplicationAdvancedSettings, Context, Listen, Listener, Listeners, ListenersHelper};
use crate::logger::Logger;
use crate::models::types::CloudProvider;
use crate::models::types::ToTeraContext;
use crate::utilities::to_short_id;
use function_name::named;
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

        let route_data_templates = self
            .routes
            .iter()
            .filter_map(|r| {
                match &environment
                    .applications
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

        let tls_domain = kubernetes.dns_provider().domain().wildcarded();
        context.insert("router_tls_domain", tls_domain.to_string().as_str());
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

        // Nginx
        context.insert("sticky_sessions_enabled", &self.sticky_sessions_enabled);

        // ingress advanced settings
        // 1 app == 1 ingress, we filter only on the app to retrieve advanced settings
        let _ = self
            .routes
            .iter()
            .map(|r| {
                if let Some(application) = &environment
                    .applications
                    .iter()
                    .find(|app| app.name() == r.application_name.as_str())
                {
                    if application.application_advanced_settings().is_some() {
                        let advanced_settings = application
                            .application_advanced_settings()
                            .expect("expected application advanced settings");
                        context.insert(
                            "ingress_proxy_body_size_mb",
                            &advanced_settings.network_ingress_proxy_body_size_mb,
                        );
                        context.insert("ingress_cors_enable", &advanced_settings.network_ingress_cors_enable);
                        context.insert(
                            "ingress_cors_allow_origin",
                            &advanced_settings.network_ingress_cors_allow_origin,
                        );
                        context.insert(
                            "ingress_cors_allow_methods",
                            &advanced_settings.network_ingress_cors_allow_methods,
                        );
                        context.insert(
                            "ingress_cors_allow_headers",
                            &advanced_settings.network_ingress_cors_allow_headers,
                        );
                    }
                }
            })
            .collect::<Vec<()>>();

        Ok(context)
    }
}

impl<T: CloudProvider> ToTransmitter for Router<T> {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::Router(self.id.to_string(), self.name.to_string())
    }
}

impl<T: CloudProvider> Listen for Router<T> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

impl<T: CloudProvider> Helm for Router<T> {
    fn helm_selector(&self) -> Option<String> {
        self.selector()
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("router-{}", self.id), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/ingress-nginx", self.context.lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        format!(
            "{}/{}/chart_values/nginx-ingress",
            self.context.lib_root_dir(),
            T::lib_directory_name()
        )
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        String::new()
    }
}

impl<T: CloudProvider> Service for Router<T>
where
    Router<T>: ToTeraContext,
{
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

    fn application_advanced_settings(&self) -> Option<ApplicationAdvancedSettings> {
        None
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

    fn total_cpus(&self) -> String {
        "1".to_string()
    }

    fn cpu_burst(&self) -> String {
        "1".to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        1
    }

    fn min_instances(&self) -> u32 {
        1
    }

    fn max_instances(&self) -> u32 {
        1
    }

    fn publicly_accessible(&self) -> bool {
        false
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        self.to_tera_context(target)
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn selector(&self) -> Option<String> {
        self.selector()
    }

    fn as_service(&self) -> &dyn Service {
        self
    }
}

pub trait RouterService: Service + DeploymentAction + Listen + Helm {
    /// all domains (auto-generated by Qovery and user custom domains) associated to the router
    fn has_custom_domains(&self) -> bool;
    fn check_domains(
        &self,
        domains_to_check: Vec<&str>,
        event_details: EventDetails,
        logger: &dyn Logger,
    ) -> Result<(), EngineError> {
        check_domain_for(
            ListenersHelper::new(self.listeners()),
            domains_to_check,
            self.id(),
            self.context().execution_id(),
            event_details,
            logger,
        )?;
        Ok(())
    }
}

impl<T: CloudProvider> DeploymentAction for Router<T>
where
    Router<T>: Service,
{
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(RouterDeploymentReporter::new(self, target, Action::Create), || {
            let kubernetes = target.kubernetes;
            let environment = target.environment;
            let workspace_dir = self.workspace_directory();
            let helm_release_name = self.helm_release_name();

            let kubernetes_config_file_path = kubernetes.get_kubeconfig_file_path()?;

            // respect order - getting the context here and not before is mandatory
            // the nginx-ingress must be available to get the external dns target if necessary
            let context = self.tera_context(target)?;

            let from_dir = format!(
                "{}/{}/charts/q-ingress-tls",
                self.context.lib_root_dir(),
                T::lib_directory_name()
            );
            if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
                from_dir.as_str(),
                workspace_dir.as_str(),
                context,
            ) {
                return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    event_details.clone(),
                    from_dir,
                    workspace_dir,
                    e,
                ));
            }

            // do exec helm upgrade and return the last deployment status
            let helm = helm::Helm::new(
                &kubernetes_config_file_path,
                &kubernetes.cloud_provider().credentials_environment_variables(),
            )
            .map_err(|e| to_engine_error(&event_details, e))?;

            let chart = ChartInfo::new_from_custom_namespace(
                helm_release_name,
                workspace_dir,
                environment.namespace().to_string(),
                600_i64,
                vec![],
                vec![],
                vec![],
                false,
                self.selector(),
            );

            helm.upgrade(&chart, &[])
                .map_err(|e| EngineError::new_helm_error(event_details.clone(), e))
        })
    }

    #[named]
    fn on_create_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        // check non custom domains
        self.check_domains(vec![self.default_domain.as_str()], event_details.clone(), self.logger())?;

        let custom_domains_to_check = if self.advanced_settings.custom_domain_check_enabled {
            self.custom_domains.iter().collect::<Vec<_>>()
        } else {
            self.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new("Custom domain check is disabled.".to_string(), None),
            ));

            vec![]
        };

        // Wait/Check that custom domain is a CNAME targeting qovery
        for domain_to_check in custom_domains_to_check {
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
                    // TODO(benjaminch): Handle better this one via a proper error eventually
                    self.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(
                            format!(
                                "Invalid CNAME for {}. It might not be an issue if user is using a CDN.",
                                domain_to_check.domain,
                            ),
                            Some(err.to_string()),
                        ),
                    ));
                }
            }
        }

        Ok(())
    }

    #[named]
    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        Ok(())
    }

    #[named]
    fn on_pause_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        Ok(())
    }

    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(RouterDeploymentReporter::new(self, target, Action::Delete), || {
            delete_stateless_service(target, self, event_details.clone())
        })
    }

    #[named]
    fn on_delete_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        Ok(())
    }
}

impl<T: CloudProvider> RouterService for Router<T>
where
    Router<T>: Service,
{
    fn has_custom_domains(&self) -> bool {
        !self.custom_domains.is_empty()
    }
}
