use crate::build_platform::Build;
use crate::cloud_provider::models::{CustomDomain, CustomDomainDataTemplate, HostDataTemplate, Route};
use crate::cloud_provider::service::{default_tera_context, Action, Service, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, Transmitter};
use crate::io_models::application::{Port, Protocol};
use crate::io_models::context::Context;
use crate::models::types::CloudProvider;
use crate::models::types::ToTeraContext;
use crate::utilities::to_short_id;
use std::iter;
use std::marker::PhantomData;
use tera::Context as TeraContext;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum RouterError {
    #[error("Router invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Error decoding base64 secret: {0}")]
    Base64DecodeError(String),
    #[error("Basic Auth environment variable not found but defined in the advanced settings")]
    BasicAuthEnvVarNotFound,
}

pub struct RouterAdvancedSettings {
    pub custom_domain_check_enabled: bool,
    pub whitelist_source_range: Option<String>,
    pub denylist_source_range: Option<String>,
    pub basic_auth: Option<String>,
}

impl Default for RouterAdvancedSettings {
    fn default() -> Self {
        Self {
            custom_domain_check_enabled: true,
            whitelist_source_range: None,
            denylist_source_range: None,
            basic_auth: None,
        }
    }
}

impl RouterAdvancedSettings {
    pub fn new(
        custom_domain_check_enabled: bool,
        whitelist_source_range: Option<String>,
        denylist_source_range: Option<String>,
        basic_auth: Option<String>,
    ) -> Self {
        let definitive_whitelist =
            whitelist_source_range.filter(|whitelist| whitelist != &Self::whitelist_source_range_default_value());
        Self {
            custom_domain_check_enabled,
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
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
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
    pub(super) workspace_directory: String,
    pub(super) lib_root_directory: String,
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
        })
    }

    fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
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
                (application.kube_name(), application.public_ports())
            } else {
                let container = environment
                    .containers
                    .iter()
                    .find(|container| container.long_id() == &service_id)
                    .ok_or_else(|| EngineError::new_router_failed_to_deploy(event_details.clone()))?;

                // advanced settings
                context.insert("advanced_settings", &container.advanced_settings());
                context.insert("associated_service_long_id", &service_id);
                context.insert("associated_service_type", "container");

                (container.kube_name(), container.public_ports())
            };

        // Get the alternative names we need to generate for the certificate
        // For custom domain, we need to generate a subdomain for each port. p80.mydomain.com, p443.mydomain.com
        let cluster_domain = kubernetes.dns_provider().domain().to_string();
        context.insert(
            "certificate_alternative_names",
            &generate_certificate_alternative_names(&self.custom_domains, &cluster_domain, &ports),
        );

        let http_ports: Vec<&Port> = ports
            .iter()
            .filter(|port| port.protocol == Protocol::HTTP)
            .cloned()
            .collect();
        let grpc_ports: Vec<&Port> = ports
            .iter()
            .filter(|port| port.protocol == Protocol::GRPC)
            .cloned()
            .collect();
        let cluster_domain = target.kubernetes.dns_provider().domain().to_string();
        let http_hosts = to_host_data_template(
            service_name,
            &http_ports,
            &self.default_domain,
            &self.custom_domains,
            &cluster_domain,
        );
        let grpc_hosts = to_host_data_template(
            service_name,
            &grpc_ports,
            &self.default_domain,
            &self.custom_domains,
            &cluster_domain,
        );

        context.insert("has_wildcard_domain", &self.custom_domains.iter().any(|d| d.is_wildcard()));
        context.insert("http_hosts", &http_hosts);
        context.insert("grpc_hosts", &grpc_hosts);

        let lets_encrypt_url = match target.is_test_cluster {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("spec_acme_server", lets_encrypt_url);

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
) -> Vec<HostDataTemplate> {
    if ports.is_empty() {
        return vec![];
    }

    let mut hosts: Vec<HostDataTemplate> = Vec::with_capacity((custom_domains.len() + 1) * (ports.len() + 1));

    // Special case for wildcard domains, were we want to create only 2 routes
    // 1 for the wildcard domain and 1 for the default domain (*.mydomain.com and mydomain.com)
    // It impose that there is only 1 public port, as else we cant route to the correct service
    let (wildcards_domains, custom_domains): (Vec<&CustomDomain>, Vec<&CustomDomain>) =
        custom_domains.iter().partition(|cd| cd.is_wildcard());
    for wildcard_domain in &wildcards_domains {
        for port in ports {
            hosts.push(HostDataTemplate {
                domain_name: format!("{}.{}", port.name, wildcard_domain.domain_without_wildcard()),
                service_name: service_name.to_string(),
                service_port: port.port,
            });
        }

        let Some(port) = ports.iter().find(|p| p.is_default) else {
            continue;
        };

        hosts.push(HostDataTemplate {
            domain_name: wildcard_domain.domain.clone(),
            service_name: service_name.to_string(),
            service_port: port.port,
        });
        hosts.push(HostDataTemplate {
            domain_name: wildcard_domain.domain_without_wildcard().to_string(),
            service_name: service_name.to_string(),
            service_port: port.port,
        });
    }

    // Normal case
    // We create 1 route per port and per custom domain
    for port in ports {
        hosts.push(HostDataTemplate {
            domain_name: format!("{}-{}", port.name, default_domain),
            service_name: service_name.to_string(),
            service_port: port.port,
        });

        if port.is_default {
            hosts.push(HostDataTemplate {
                domain_name: default_domain.to_string(),
                service_name: service_name.to_string(),
                service_port: port.port,
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
                service_name: service_name.to_string(),
                service_port: port.port,
            });

            if port.is_default {
                hosts.push(HostDataTemplate {
                    domain_name: custom_domain.domain.clone(),
                    service_name: service_name.to_string(),
                    service_port: port.port,
                });
            }
        }
    }

    hosts
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
        .filter(|domain| domain.is_wildcard() || !domain.domain.ends_with(&cluster_domain))
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

    fn kube_name(&self) -> &str {
        &self.kube_name
    }

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn kube_label_selector(&self) -> String {
        self.kube_label_selector()
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

    fn version(&self) -> String {
        "".to_string()
    }
}

pub trait RouterService: Service + DeploymentAction + ToTeraContext + Send {
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

#[cfg(test)]
mod tests {
    use super::RouterAdvancedSettings;
    use crate::cloud_provider::models::{CustomDomain, CustomDomainDataTemplate, HostDataTemplate};
    use crate::io_models::application::{Port, Protocol};
    use crate::models::router::{generate_certificate_alternative_names, to_host_data_template};

    #[test]
    pub fn test_router_advanced_settings() {
        // this should be true by default
        let router_advanced_settings_defaults = RouterAdvancedSettings::default();
        assert!(router_advanced_settings_defaults.custom_domain_check_enabled);
    }

    #[test]
    pub fn test_certificate_alternative_names() {
        let custom_domains = vec![
            CustomDomain {
                domain: "toto.com".to_string(),
                target_domain: "".to_string(),
            },
            CustomDomain {
                domain: "cluster.com".to_string(),
                target_domain: "".to_string(),
            },
        ];

        let ports: Vec<&Port> = vec![];

        let certificate_names = generate_certificate_alternative_names(&custom_domains, "cluster.com", &ports);
        assert_eq!(certificate_names.len(), 0);

        let port = Port {
            long_id: Default::default(),
            name: "http".to_string(),
            publicly_accessible: true,
            port: 80,
            is_default: false,
            protocol: Protocol::HTTP,
        };
        let ports = vec![&port];

        // We don't generate subdomains when there is a singe public port
        let certificate_names = generate_certificate_alternative_names(&custom_domains, "cluster.com", &ports);
        assert_eq!(certificate_names.len(), 1);
        assert_eq!(certificate_names[0].domain, "toto.com");

        let port2 = Port {
            long_id: Default::default(),
            name: "grpc".to_string(),
            publicly_accessible: true,
            port: 8080,
            is_default: false,
            protocol: Protocol::GRPC,
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
        }];
        let port2 = Port {
            long_id: Default::default(),
            name: "grpc".to_string(),
            publicly_accessible: true,
            port: 8080,
            is_default: false,
            protocol: Protocol::GRPC,
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
            publicly_accessible: true,
            port: 80,
            is_default: true,
            protocol: Protocol::HTTP,
        };
        let port_grpc = Port {
            long_id: Default::default(),
            name: "grpc".to_string(),
            publicly_accessible: true,
            port: 8080,
            is_default: false,
            protocol: Protocol::GRPC,
        };
        let custom_domains = vec![CustomDomain {
            domain: "*.toto.mydomain.com".to_string(),
            target_domain: "".to_string(),
        }];

        let ret = to_host_data_template("srv", &[&port_http], "cluster.com", &custom_domains, "cluster.com");
        assert_eq!(ret.len(), 5);
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "http-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "*.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "http.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));

        // If the port is not the default one, there should not be  default and wildcard route/host
        let ret = to_host_data_template("srv", &[&port_grpc], "cluster.com", &custom_domains, "cluster.com");
        assert_eq!(ret.len(), 2);
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "grpc-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "grpc.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
        }));

        // we mix both wildcard and non wildcard domains
        let custom_domains = vec![
            CustomDomain {
                domain: "super.mydomain.com".to_string(),
                target_domain: "".to_string(),
            },
            CustomDomain {
                domain: "*.toto.mydomain.com".to_string(),
                target_domain: "".to_string(),
            },
        ];
        let ret =
            to_host_data_template("srv", &[&port_http, &port_grpc], "cluster.com", &custom_domains, "cluster.com");
        assert_eq!(ret.len(), 10);
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "grpc-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "grpc.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "http-cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "*.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "http.toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "toto.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "super.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "http.super.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "grpc.super.mydomain.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 8080,
        }));
    }

    #[test]
    pub fn test_ingress_host_template_with_custom_domain_managed_by_cluster() {
        let port_http = Port {
            long_id: Default::default(),
            name: "http".to_string(),
            publicly_accessible: true,
            port: 80,
            is_default: true,
            protocol: Protocol::HTTP,
        };
        let custom_domains = vec![CustomDomain {
            domain: "toto.cluster.com".to_string(),
            target_domain: "".to_string(),
        }];

        let ret = to_host_data_template("srv", &[&port_http], "cluster.com", &custom_domains, "cluster.com");
        assert_eq!(ret.len(), 4);
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "http-toto.cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));

        let ret = to_host_data_template("srv", &[&port_http], "cluster.com", &custom_domains, "fake.com");
        assert_eq!(ret.len(), 4);
        assert!(ret.contains(&HostDataTemplate {
            domain_name: "http.toto.cluster.com".to_string(),
            service_name: "srv".to_string(),
            service_port: 80,
        }));
    }
}
