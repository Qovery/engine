use crate::environment::action::DeploymentAction;
use crate::environment::models::annotations_group::AnnotationsGroupTeraContext;
use crate::environment::models::labels_group::LabelsGroupTeraContext;
use crate::environment::models::port::Port;
use crate::environment::models::types::CloudProvider;
use crate::environment::models::types::ToTeraContext;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::Build;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType, default_tera_context};
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::application::Protocol;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{
    CustomDomain, CustomDomainDataTemplate, EnvironmentVariable, HostDataTemplate, HostPathType, Route,
};
use crate::utilities::to_short_id;
use std::collections::HashMap;
use std::iter;
use std::marker::PhantomData;
use std::path::PathBuf;
use tera::Context as TeraContext;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum RouterError {
    #[error("Router invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Error decoding base64 basic Auth environment variable `{env_var_name}`: `{env_var_value}`")]
    BasicAuthEnvVarBase64DecodeError {
        env_var_name: String,
        env_var_value: String,
    },
    #[error("Basic Auth environment variable `{env_var_name}` not found but defined in the advanced settings")]
    BasicAuthEnvVarNotFound { env_var_name: String },
}

#[derive(Default)]
pub struct RouterAdvancedSettings {
    pub whitelist_source_range: Option<String>,
    pub denylist_source_range: Option<String>,
    pub basic_auth: Option<String>,
}

impl RouterAdvancedSettings {
    pub fn new(
        whitelist_source_range: Option<String>,
        denylist_source_range: Option<String>,
        basic_auth: Option<String>,
    ) -> Self {
        let definitive_whitelist =
            whitelist_source_range.filter(|whitelist| whitelist != &Self::whitelist_source_range_default_value());
        Self {
            whitelist_source_range: definitive_whitelist,
            denylist_source_range,
            basic_auth,
        }
    }

    pub fn whitelist_source_range_default_value() -> String {
        "0.0.0.0/0".to_string()
    }
}

pub struct Router<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(crate) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) action: Action,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) default_domain: String,
    pub(crate) custom_domains: Vec<CustomDomain>,
    pub(crate) routes: Vec<Route>,
    pub(crate) _extra_settings: T::RouterExtraSettings,
    pub(crate) advanced_settings: RouterAdvancedSettings,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) lib_root_directory: String,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
}

impl<T: CloudProvider> Router<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: &str,
        kube_name: String,
        action: Action,
        default_domain: &str,
        custom_domains: Vec<CustomDomain>,
        routes: Vec<Route>,
        extra_settings: T::RouterExtraSettings,
        advanced_settings: RouterAdvancedSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
    ) -> Result<Self, RouterError> {
        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("routers/{long_id}"),
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
            kube_name,
            action,
            default_domain: default_domain.to_string(),
            custom_domains,
            routes,
            _extra_settings: extra_settings,
            advanced_settings,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
            annotations_group: AnnotationsGroupTeraContext::new(annotations_groups),
            labels_group: LabelsGroupTeraContext::new(labels_groups),
        })
    }

    fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub fn workspace_directory(&self) -> &str {
        self.workspace_directory.to_str().unwrap_or("")
    }

    pub(crate) fn default_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>>
    where
        Self: Service,
    {
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));

        // We can only have 1 router per application/container.
        // Core never mix multiple services inside one router
        let service_id = self
            .routes
            .first()
            .ok_or_else(|| EngineError::new_router_failed_to_deploy(event_details.clone()))?
            .service_long_id;

        // Check if the service is an application
        let (service_name, ports) =
            if let Some(application) = &environment.applications.iter().find(|app| app.long_id() == &service_id) {
                // advanced settings
                context.insert("advanced_settings", &application.advanced_settings());
                context.insert("associated_service_long_id", &service_id);
                context.insert("associated_service_type", "application");

                if let Some(network_ingress_nginx_controller_server_snippet) = &application
                    .advanced_settings()
                    .network_ingress_nginx_controller_server_snippet
                {
                    // this advanced setting is injected independently of other advanced settings because we inject the proper model object instead of the io model
                    context.insert(
                        "nginx_ingress_controller_server_snippet",
                        &network_ingress_nginx_controller_server_snippet
                            .to_model()
                            .get_snippet_value(),
                    );
                }

                if let Some(network_ingress_nginx_controller_configuration_snippet) = &application
                    .advanced_settings()
                    .network_ingress_nginx_controller_configuration_snippet
                {
                    // this advanced setting is injected independently of other advanced settings because we inject the proper model object instead of the io model
                    context.insert(
                        "nginx_ingress_controller_configuration_snippet",
                        &network_ingress_nginx_controller_configuration_snippet
                            .to_model()
                            .get_snippet_value(),
                    );
                }

                (application.kube_name(), application.public_ports())
            } else if let Some(container) = &environment
                .containers
                .iter()
                .find(|container| container.long_id() == &service_id)
            {
                // advanced settings
                context.insert("advanced_settings", &container.advanced_settings());
                context.insert("associated_service_long_id", &service_id);
                context.insert("associated_service_type", "container");

                if let Some(network_ingress_nginx_controller_server_snippet) = &container
                    .advanced_settings()
                    .network_ingress_nginx_controller_server_snippet
                {
                    // this advanced setting is inject independently of other advanced settings because we inject the proper model object instead of the io model
                    context.insert(
                        "nginx_ingress_controller_server_snippet",
                        &network_ingress_nginx_controller_server_snippet
                            .to_model()
                            .get_snippet_value(),
                    );
                }

                if let Some(network_ingress_nginx_controller_configuration_snippet) = &container
                    .advanced_settings()
                    .network_ingress_nginx_controller_configuration_snippet
                {
                    // this advanced setting is injected independently of other advanced settings because we inject the proper model object instead of the io model
                    context.insert(
                        "nginx_ingress_controller_configuration_snippet",
                        &network_ingress_nginx_controller_configuration_snippet
                            .to_model()
                            .get_snippet_value(),
                    );
                }

                (container.kube_name(), container.public_ports())
            } else {
                let helm_chart = environment
                    .helm_charts
                    .iter()
                    .find(|helm_chart| helm_chart.long_id() == &service_id)
                    .ok_or_else(|| EngineError::new_router_failed_to_deploy(event_details.clone()))?;

                // advanced settings
                context.insert("advanced_settings", &helm_chart.advanced_settings());
                context.insert("associated_service_long_id", &service_id);
                context.insert("associated_service_type", "helm");

                if let Some(network_ingress_nginx_controller_server_snippet) = &helm_chart
                    .advanced_settings()
                    .network_ingress_nginx_controller_server_snippet
                {
                    // this advanced setting is injected independently of other advanced settings because we inject the proper model object instead of the io model
                    context.insert(
                        "nginx_ingress_controller_server_snippet",
                        &network_ingress_nginx_controller_server_snippet
                            .to_model()
                            .get_snippet_value(),
                    );
                }

                if let Some(network_ingress_nginx_controller_configuration_snippet) = &helm_chart
                    .advanced_settings()
                    .network_ingress_nginx_controller_configuration_snippet
                {
                    // this advanced setting is injected independently of other advanced settings because we inject the proper model object instead of the io model
                    context.insert(
                        "nginx_ingress_controller_configuration_snippet",
                        &network_ingress_nginx_controller_configuration_snippet
                            .to_model()
                            .get_snippet_value(),
                    );
                }

                (helm_chart.kube_name(), helm_chart.public_ports())
            };

        // inject basic auth data
        context.insert("basic_auth_htaccess", &self.advanced_settings.basic_auth);

        // Get the alternative names we need to generate for the certificate
        // For custom domain, we need to generate a subdomain for each port. p80.mydomain.com, p443.mydomain.com
        let cluster_domain = target.dns_provider.domain().to_string();
        context.insert(
            "certificate_alternative_names",
            &generate_certificate_alternative_names(&self.custom_domains, &cluster_domain, &ports),
        );

        let http_ports: Vec<&Port> = ports
            .iter()
            .filter(|port| port.protocol.protocol() == Protocol::HTTP)
            .cloned()
            .collect();
        let grpc_ports: Vec<&Port> = ports
            .iter()
            .filter(|port| port.protocol.protocol() == Protocol::GRPC)
            .cloned()
            .collect();
        let cluster_domain = target.dns_provider.domain().to_string();
        let http_hosts_per_namespace = to_host_data_template(
            service_name,
            &http_ports,
            &self.default_domain,
            &self.custom_domains,
            &cluster_domain,
            environment.namespace(),
        );
        let grpc_hosts_per_namespace = to_host_data_template(
            service_name,
            &grpc_ports,
            &self.default_domain,
            &self.custom_domains,
            &cluster_domain,
            environment.namespace(),
        );

        let http_hosts_has_regex_path = http_hosts_per_namespace
            .values()
            .flatten()
            .any(|host| host.path != Port::DEFAULT_PUBLIC_PATH);
        let grpc_hosts_has_regex_path = grpc_hosts_per_namespace
            .values()
            .flatten()
            .any(|host| host.path != Port::DEFAULT_PUBLIC_PATH);
        context.insert("has_wildcard_domain", &self.custom_domains.iter().any(|d| d.is_wildcard()));
        context.insert("http_hosts_has_regex_path", &http_hosts_has_regex_path);
        context.insert("http_hosts_per_namespace", &http_hosts_per_namespace);
        context.insert("grpc_hosts_has_regex_path", &grpc_hosts_has_regex_path);
        context.insert("grpc_hosts_per_namespace", &grpc_hosts_per_namespace);

        context.insert("annotations_group", &self.annotations_group);
        context.insert("labels_group", &self.labels_group);

        let lets_encrypt_url = match target.is_test_cluster {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("spec_acme_server", lets_encrypt_url);

        context.insert("k8s_deploy_api_gateway", &kubernetes.advanced_settings().k8s_deploy_api_gateway);
        context.insert("k8s_use_api_gateway", &kubernetes.advanced_settings().k8s_use_api_gateway);
        context.insert("k8s_remove_nginx", &kubernetes.advanced_settings().k8s_remove_nginx);

        Ok(context)
    }

    pub fn helm_release_name(&self) -> String {
        crate::string::cut(format!("router-{}", self.id), 50)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-ingress-tls", self.lib_root_directory,)
    }
}

fn to_host_data_template(
    service_name: &str,
    ports: &[&Port],
    default_domain: &str,
    custom_domains: &[CustomDomain],
    cluster_domain: &str,
    environment_namespace: &str,
) -> HashMap<String, Vec<HostDataTemplate>> {
    if ports.is_empty() {
        return HashMap::new();
    }
    let to_port_path = |port: &Port| -> String { port.public_path().expect("port should be public here").to_string() };
    let to_path_rewrite = |port: &Port| -> Option<String> { port.public_path_rewrite().map(|p| p.to_string()) };
    let to_path_type = |port: &Port| -> HostPathType {
        HostPathType::from_path(port.public_path().unwrap_or_default(), HostPathType::PathPrefix)
    };
    let to_path_weight = |_port: &Port| -> u32 { 1 };
    let ports_by_namespace = get_ports_by_namespace(ports);

    let mut hosts_per_namespace: HashMap<String, Vec<HostDataTemplate>> =
        HashMap::with_capacity(ports_by_namespace.keys().len());
    for (namespace, ports) in &ports_by_namespace {
        let mut hosts: Vec<HostDataTemplate> = Vec::with_capacity((custom_domains.len() + 1) * (ports.len() + 1));

        // Special case for wildcard domains, where we want to create only 2 routes
        // 1 for the wildcard domain and 1 for the default domain (*.mydomain.com and mydomain.com)
        // It imposes that there is only 1 public port, as else we cant route to the correct service
        let (wildcards_domains, custom_domains): (Vec<&CustomDomain>, Vec<&CustomDomain>) =
            custom_domains.iter().partition(|cd| cd.is_wildcard());
        for wildcard_domain in &wildcards_domains {
            for port in ports {
                hosts.push(HostDataTemplate {
                    domain_name: format!("{}.{}", port.name, wildcard_domain.domain_without_wildcard()),
                    service_name: get_service_name(port, service_name),
                    service_port: port.port,
                    path: to_port_path(port),
                    path_rewrite: to_path_rewrite(port),
                    path_type: to_path_type(port),
                    weight: to_path_weight(port),
                });
            }

            let Some(port) = ports.iter().find(|p| p.is_default) else {
                continue;
            };

            hosts.push(HostDataTemplate {
                domain_name: wildcard_domain.domain.clone(),
                service_name: get_service_name(port, service_name),
                service_port: port.port,
                path: to_port_path(port),
                path_rewrite: to_path_rewrite(port),
                path_type: to_path_type(port),
                weight: to_path_weight(port),
            });
            hosts.push(HostDataTemplate {
                domain_name: wildcard_domain.domain_without_wildcard().to_string(),
                service_name: get_service_name(port, service_name),
                service_port: port.port,
                path: to_port_path(port),
                path_rewrite: to_path_rewrite(port),
                path_type: to_path_type(port),
                weight: to_path_weight(port),
            });
        }

        // Normal case
        // We create 1 route per port and per custom domain
        for port in ports {
            hosts.push(HostDataTemplate {
                domain_name: format!("{}-{}", port.name, default_domain),
                service_name: get_service_name(port, service_name),
                service_port: port.port,
                path: to_port_path(port),
                path_rewrite: to_path_rewrite(port),
                path_type: to_path_type(port),
                weight: to_path_weight(port),
            });

            if port.is_default {
                hosts.push(HostDataTemplate {
                    domain_name: default_domain.to_string(),
                    service_name: get_service_name(port, service_name),
                    service_port: port.port,
                    path: to_port_path(port),
                    path_rewrite: to_path_rewrite(port),
                    path_type: to_path_type(port),
                    weight: to_path_weight(port),
                });
            }

            for custom_domain in &custom_domains {
                // We allow users to use the cluster domain as a custom domain. So in this case it must be separated by a -
                // and not create a new subdomain
                let separator = if custom_domain.domain.ends_with(cluster_domain) {
                    '-'
                } else {
                    '.'
                };
                hosts.push(HostDataTemplate {
                    domain_name: format!("{}{}{}", port.name, separator, custom_domain.domain),
                    service_name: get_service_name(port, service_name),
                    service_port: port.port,
                    path: to_port_path(port),
                    path_rewrite: to_path_rewrite(port),
                    path_type: to_path_type(port),
                    weight: to_path_weight(port),
                });

                if port.is_default {
                    hosts.push(HostDataTemplate {
                        domain_name: custom_domain.domain.clone(),
                        service_name: get_service_name(port, service_name),
                        service_port: port.port,
                        path: to_port_path(port),
                        path_rewrite: to_path_rewrite(port),
                        path_type: to_path_type(port),
                        weight: to_path_weight(port),
                    });
                }
            }
        }

        hosts_per_namespace.insert(
            namespace
                .as_ref()
                .cloned()
                .unwrap_or_else(|| environment_namespace.to_string()),
            hosts,
        );
    }
    hosts_per_namespace
}

fn get_ports_by_namespace(ports: &[&Port]) -> HashMap<Option<String>, Vec<Port>> {
    let mut ports_by_namespace: HashMap<Option<String>, Vec<Port>> = HashMap::new();
    for &port in ports {
        let entry = ports_by_namespace.entry(port.namespace.clone()).or_default();
        entry.push(port.clone());
    }
    ports_by_namespace
}

fn get_service_name(port: &Port, default_service_name: &str) -> String {
    port.service_name
        .as_ref()
        .cloned()
        .unwrap_or_else(|| default_service_name.to_string())
}

// Generating certificates correctly is tricky
// Ideally we would like to always generate a certificate for the root domain (I.e: example.com) and all the subdomains (I.e: *.example.com)
// But we have limited access to the dns of the clients. So we can't always generate a wildcard certificate for the domain.
// LetsEncrypt has a limit that wildcard certificates can only be generated with DNS01 challenge, which requires us to have access to the dns of the client.
// So we must use HTTP01 challenge, by specifing each ASN domain we want to generate a certificate for. (and hopping client have correctly set their DNS records)
// Wildcard domains if for now a special case, were clients give us access to their cloudflare DNS, so we can generate wildcard certificates for them.
fn generate_certificate_alternative_names(
    custom_domains: &[CustomDomain],
    cluster_domain: &str,
    ports: &[&Port],
) -> Vec<CustomDomainDataTemplate> {
    if ports.is_empty() || custom_domains.is_empty() {
        return vec![];
    }

    custom_domains
        .iter()
        // we filter out domain that belongs to our cluster, we dont need to create certificate for them
        // we keep wildcard domains, as we will need to create certificate for them
        .filter(|domain| {
            (domain.is_wildcard() || !domain.domain.ends_with(&cluster_domain)) && domain.generate_certificate
        })
        .flat_map(|cd| {
            // We always want the root domain to be in the certificate (I.e: example.com, or if *.example.com -> example.com)
            let default_domain = CustomDomainDataTemplate {
                domain: cd.domain_without_wildcard().to_string(),
            };

            // If it is a wildcard domain, we want to generate the wildcard certificate (*.example.com)
            // if there is a single public port, we can use only the default domain and don't generate subdomains for each port. (to avoid migration for clients)
            iter::once(default_domain).chain(if cd.is_wildcard() {
                vec![CustomDomainDataTemplate {
                    domain: cd.domain.to_string(),
                }]
            } else if ports.len() == 1 {
                vec![]
            } else {
                ports
                    .iter()
                    .map(|port| CustomDomainDataTemplate {
                        domain: format!("{}.{}", port.name, cd.domain),
                    })
                    .collect()
            })
        })
        .collect::<Vec<_>>()
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

    fn version(&self) -> String {
        "".to_string()
    }

    fn kube_name(&self) -> &str {
        &self.kube_name
    }

    fn kube_label_selector(&self) -> String {
        self.kube_label_selector()
    }

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        &self.action
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

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        vec![]
    }
}

pub trait RouterService: Service + DeploymentAction + ToTeraContext + Send {
    /// all domains (auto-generated by Qovery and user custom domains) associated to the router
    fn has_custom_domains(&self) -> bool;

    fn as_deployment_action(&self) -> &dyn DeploymentAction;

    fn associated_service_id(&self) -> Option<Uuid>;
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

    fn associated_service_id(&self) -> Option<Uuid> {
        self.routes.first().map(|route| route.service_long_id)
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::models::port::{HttpPublicPortConfig, Port, PortProtocol};
    use crate::environment::models::router::{generate_certificate_alternative_names, to_host_data_template};

    use crate::io_models::models::{CustomDomain, CustomDomainDataTemplate, HostDataTemplate, HostPathType};

    #[test]
    pub fn test_certificate_alternative_names() {
        let custom_domains = vec![
            CustomDomain {
                domain: "toto.com".to_string(),
                target_domain: "".to_string(),
                generate_certificate: true,
                use_cdn: true,
            },
            CustomDomain {
                domain: "cluster.com".to_string(),
                target_domain: "".to_string(),
                generate_certificate: true,
                use_cdn: true,
            },
            CustomDomain {
                domain: "titi.com".to_string(),
                target_domain: "".to_string(),
                generate_certificate: false,
                use_cdn: true,
            },
        ];

        let ports: Vec<&Port> = vec![];

        let certificate_names = generate_certificate_alternative_names(&custom_domains, "cluster.com", &ports);
        assert_eq!(certificate_names.len(), 0);

        let port = Port {
            long_id: Default::default(),
            name: "http".to_string(),
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
            port: 80,
            is_default: false,
            service_name: None,
            namespace: None,
        };
        let ports = vec![&port];

        // We don't generate subdomains when there is a singe public port
        let certificate_names = generate_certificate_alternative_names(&custom_domains, "cluster.com", &ports);
        assert_eq!(certificate_names.len(), 1);
        assert_eq!(certificate_names[0].domain, "toto.com");

        let port2 = Port {
            long_id: Default::default(),
            name: "grpc".to_string(),
            port: 8080,
            is_default: false,
            service_name: None,
            namespace: None,
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
        };
        let ports = vec![&port, &port2];

        let certificate_names = generate_certificate_alternative_names(&custom_domains, "cluster.com", &ports);
        assert_eq!(certificate_names.len(), 3);
        assert!(certificate_names.contains(&CustomDomainDataTemplate {
            domain: "toto.com".to_string()
        }));
        assert!(certificate_names.contains(&CustomDomainDataTemplate {
            domain: "http.toto.com".to_string()
        }));
        assert!(certificate_names.contains(&CustomDomainDataTemplate {
            domain: "grpc.toto.com".to_string()
        }));

        // We generate wildcard certificate when there is a wildcard domain
        let custom_domains = vec![CustomDomain {
            domain: "*.toto.cluster.com".to_string(),
            target_domain: "".to_string(),
            generate_certificate: true,
            use_cdn: true,
        }];
        let port2 = Port {
            long_id: Default::default(),
            name: "grpc".to_string(),
            protocol: PortProtocol::GRPC {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
            port: 8080,
            is_default: false,
            service_name: None,
            namespace: None,
        };
        let ports = vec![&port, &port2];

        let certificate_names = generate_certificate_alternative_names(&custom_domains, "cluster.com", &ports);
        assert_eq!(certificate_names.len(), 2);
        assert!(certificate_names.contains(&CustomDomainDataTemplate {
            domain: "toto.cluster.com".to_string()
        }));
        assert!(certificate_names.contains(&CustomDomainDataTemplate {
            domain: "*.toto.cluster.com".to_string()
        }));
    }

    #[test]
    pub fn test_ingress_host_template_with_wildcard() {
        let port_http = Port {
            long_id: Default::default(),
            name: "http".to_string(),
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
            port: 80,
            is_default: true,
            service_name: None,
            namespace: None,
        };
        let port_grpc = Port {
            long_id: Default::default(),
            name: "grpc".to_string(),
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
            port: 8080,
            is_default: false,
            service_name: None,
            namespace: None,
        };
        let custom_domains = vec![CustomDomain {
            domain: "*.toto.mydomain.com".to_string(),
            target_domain: "".to_string(),
            generate_certificate: true,
            use_cdn: true,
        }];

        let namespace = "env_namespace";
        let ret = to_host_data_template("srv", &[&port_http], "cluster.com", &custom_domains, "cluster.com", namespace);
        assert_eq!(ret.len(), 1);
        let host_data = ret.get(namespace).unwrap();
        assert_eq!(host_data.len(), 5);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "*.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));

        // If the port is not the default one, there should not be default and wildcard route/host
        let namespace = "env_namespace2";
        let ret = to_host_data_template("srv", &[&port_grpc], "cluster.com", &custom_domains, "cluster.com", namespace);
        assert_eq!(ret.len(), 1);
        let host_data = ret.get(namespace).unwrap();
        assert_eq!(host_data.len(), 2);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "grpc-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "grpc.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));

        // we mix both wildcard and non wildcard domains
        let custom_domains = vec![
            CustomDomain {
                domain: "super.mydomain.com".to_string(),
                target_domain: "".to_string(),
                generate_certificate: true,
                use_cdn: true,
            },
            CustomDomain {
                domain: "*.toto.mydomain.com".to_string(),
                target_domain: "".to_string(),
                generate_certificate: true,
                use_cdn: true,
            },
        ];

        let namespace = "env_namespace3";
        let ret = to_host_data_template(
            "srv",
            &[&port_http, &port_grpc],
            "cluster.com",
            &custom_domains,
            "cluster.com",
            namespace,
        );
        assert_eq!(ret.len(), 1);
        let host_data = ret.get(namespace).unwrap();
        assert_eq!(host_data.len(), 10);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "grpc-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "grpc.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "*.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "super.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http.super.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "grpc.super.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
    }

    #[test]
    pub fn test_ingress_host_template_with_custom_domain_managed_by_cluster() {
        let port_http = Port {
            long_id: Default::default(),
            name: "http".to_string(),
            port: 80,
            is_default: true,
            service_name: None,
            namespace: None,
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
        };
        let custom_domains = vec![CustomDomain {
            domain: "toto.cluster.com".to_string(),
            target_domain: "".to_string(),
            generate_certificate: true,
            use_cdn: true,
        }];

        let namespace = "namespace1";
        let ret = to_host_data_template("srv", &[&port_http], "cluster.com", &custom_domains, "cluster.com", namespace);
        assert_eq!(ret.len(), 1);
        let host_data = ret.get(namespace).unwrap();
        assert_eq!(host_data.len(), 4);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-toto.cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));

        let namespace = "namespace2";
        let ret = to_host_data_template("srv", &[&port_http], "cluster.com", &custom_domains, "fake.com", namespace);
        assert_eq!(ret.len(), 1);
        let host_data = ret.get(namespace).unwrap();
        assert_eq!(host_data.len(), 4);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http.toto.cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
    }

    #[test]
    pub fn test_ingress_host_template_with_path_rewrite() {
        let port_http = Port {
            long_id: Default::default(),
            name: "http-1".to_string(),
            port: 80,
            is_default: false,
            service_name: None,
            namespace: None,
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
        };
        let port_http_with_path_rewrite = Port {
            long_id: Default::default(),
            name: "http-2".to_string(),
            port: 8080,
            is_default: false,
            service_name: None,
            namespace: None,
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/toto".to_string(),
                    path_rewrite: Some("/titi".to_string()),
                }),
            },
        };

        let namespace = "env_namespace";
        let ret = to_host_data_template(
            "srv",
            &[&port_http, &port_http_with_path_rewrite],
            "cluster.com",
            &[],
            "cluster.com",
            namespace,
        );
        assert_eq!(ret.len(), 1);
        let host_data = ret.get(namespace).unwrap();
        assert_eq!(host_data.len(), 2);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-2-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
            path: "/toto".to_string(),
            path_rewrite: Some("/titi".to_string()),
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
    }

    #[test]
    pub fn test_ingress_host_template_with_service_name_defined_in_port() {
        let port_http = Port {
            long_id: Default::default(),
            name: "http-1".to_string(),
            port: 80,
            is_default: false,
            service_name: None,
            namespace: None,
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
        };
        let port_http_with_service_name = Port {
            long_id: Default::default(),
            name: "http-2".to_string(),
            port: 8080,
            is_default: false,
            service_name: Some("service1".to_string()),
            namespace: None,
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
        };
        let custom_domains = vec![CustomDomain {
            domain: "*.toto.mydomain.com".to_string(),
            target_domain: "".to_string(),
            generate_certificate: true,
            use_cdn: true,
        }];

        let namespace = "env_namespace";
        let ret = to_host_data_template(
            "srv",
            &[&port_http, &port_http_with_service_name],
            "cluster.com",
            &custom_domains,
            "cluster.com",
            namespace,
        );
        assert_eq!(ret.len(), 1);
        let host_data = ret.get(namespace).unwrap();
        assert_eq!(host_data.len(), 4);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-1.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-1-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-2.toto.mydomain.com".to_string(),
            service_name: "service1".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-2-cluster.com".to_string(),
            service_name: "service1".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
    }

    #[test]
    pub fn test_ingress_host_template_with_service_name_and_namespace_defined_in_port() {
        let port_http = Port {
            long_id: Default::default(),
            name: "http-1".to_string(),
            port: 80,
            is_default: false,
            service_name: None,
            namespace: None,
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
        };
        let port_http_with_service_name = Port {
            long_id: Default::default(),
            name: "http-2".to_string(),
            port: 8080,
            is_default: false,
            service_name: Some("service1".to_string()),
            namespace: Some("namespace1".to_string()),
            protocol: PortProtocol::HTTP {
                public: Some(HttpPublicPortConfig {
                    path: "/".to_string(),
                    path_rewrite: None,
                }),
            },
        };
        let custom_domains = vec![CustomDomain {
            domain: "*.toto.mydomain.com".to_string(),
            target_domain: "".to_string(),
            generate_certificate: true,
            use_cdn: true,
        }];

        let namespace = "env_namespace";
        let ret = to_host_data_template(
            "srv",
            &[&port_http, &port_http_with_service_name],
            "cluster.com",
            &custom_domains,
            "cluster.com",
            namespace,
        );
        assert_eq!(ret.len(), 2);
        let host_data = ret.get(namespace).unwrap();
        assert_eq!(host_data.len(), 2);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-1.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-1-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        let host_data = ret.get("namespace1").unwrap();
        assert_eq!(host_data.len(), 2);
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-2.toto.mydomain.com".to_string(),
            service_name: "service1".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
        assert!(host_data.contains(&HostDataTemplate {
            domain_name: "http-2-cluster.com".to_string(),
            service_name: "service1".to_string(),
            service_port: 8080,
            path: "/".to_string(),
            path_rewrite: None,
            path_type: HostPathType::PathPrefix,
            weight: 1,
        }));
    }
}
