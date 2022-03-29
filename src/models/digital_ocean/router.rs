use crate::cloud_provider::models::{CustomDomainDataTemplate, RouteDataTemplate};
use crate::cloud_provider::service::{default_tera_context, Service, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventMessage, Stage};
use crate::models::router::Router;
use crate::models::types::{ToTeraContext, DO};
use tera::Context as TeraContext;

impl ToTeraContext for Router<DO> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);
        context.insert("doks_cluster_id", kubernetes.id());

        let applications = environment
            .stateless_services()
            .into_iter()
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
            .filter_map(|r| {
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
        let external_ingress_hostname_default = crate::cmd::kubectl::kubectl_exec_get_external_ingress_hostname(
            kubernetes_config_file_path,
            "nginx-ingress",
            "nginx-ingress-ingress-nginx-controller",
            kubernetes.cloud_provider().credentials_environment_variables(),
        );

        match external_ingress_hostname_default {
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

        // Nginx
        context.insert("sticky_sessions_enabled", &self.sticky_sessions_enabled);

        Ok(context)
    }
}
