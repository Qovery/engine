use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Action, Create, Delete, Pause, Router as RRouter, Service, ServiceError, ServiceType,
    StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::models::{
    Context, Listeners, ListenersHelper, Metadata, ProgressInfo, ProgressLevel, ProgressScope,
};
use dns_lookup::lookup_host;
use retry::delay::{Fibonacci, Fixed};
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

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
    ) -> Self {
        Router {
            context,
            id: id.to_string(),
            name: name.to_string(),
            default_domain: default_domain.to_string(),
            custom_domains,
            routes,
            listeners: vec![],
        }
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("router-{}", self.id()), 50)
    }

    fn aws_credentials_envs<'x>(&self, aws: &'x AWS) -> [(&'x str, &'x str); 2] {
        [
            (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
        ]
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("routers/{}", self.name()),
        )
    }

    fn tera_context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> TeraContext {
        let mut context = self.default_tera_context(kubernetes, environment);

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

        let workspace_dir = self.workspace_directory();
        let aws = kubernetes
            .cloud_provider()
            .as_any()
            .downcast_ref::<AWS>()
            .unwrap();

        let kubernetes_config_file_path = common::kubernetes_config_path(
            workspace_dir.as_str(),
            environment.organization_id.as_str(),
            kubernetes.id(),
            aws.access_key_id.as_str(),
            aws.secret_access_key.as_str(),
            kubernetes.region(),
        );

        match kubernetes_config_file_path {
            Ok(kubernetes_config_file_path_string) => {
                // Default domain
                let external_ingress_hostname_default =
                    crate::cmd::kubectl::kubectl_exec_get_external_ingress_hostname(
                        kubernetes_config_file_path_string.as_str(),
                        "nginx-ingress",
                        "app=nginx-ingress,component=controller",
                        self.aws_credentials_envs(aws).to_vec(),
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
                        // FIXME really?
                        warn!("can't fetch kubernetes config file - what's wrong? This must never happened");
                    }
                }

                // Check if there is a custom domain first
                if !self.custom_domains.is_empty() {
                    let external_ingress_hostname_custom =
                        crate::cmd::kubectl::kubectl_exec_get_external_ingress_hostname(
                            kubernetes_config_file_path_string.as_str(),
                            environment.namespace(),
                            "app=nginx-ingress,component=controller",
                            self.aws_credentials_envs(aws).to_vec(),
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
                            // FIXME really?
                            warn!("can't fetch kubernetes config file - what's wrong? This must never happened");
                        }
                    }
                    context.insert("app_id", kubernetes.id());
                }
            }
            Err(_) => error!(
                "can't fetch kubernetes config file - what's wrong? This must never happened"
            ), // FIXME should I return an Err?
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

        context
    }

    fn delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let workspace_dir = self.workspace_directory();
        let helm_release_name = self.helm_release_name();

        let _ = common::do_stateless_service_cleanup(
            kubernetes,
            environment,
            workspace_dir.as_str(),
            helm_release_name.as_str(),
        )?;

        Ok(())
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

    fn total_cpus(&self) -> String {
        "1".to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        1
    }

    fn total_instances(&self) -> u16 {
        1
    }
}

impl crate::cloud_provider::service::Router for Router {
    fn check_domains(&self) -> Result<(), ServiceError> {
        let check_result = retry::retry(Fibonacci::from_millis(3000).take(10), || {
            // TODO send information back to the core
            info!("check custom domain {}", self.default_domain.as_str());
            match lookup_host(self.default_domain.as_str()) {
                Ok(_) => OperationResult::Ok(()),
                Err(err) => {
                    debug!("{:?}", err);
                    OperationResult::Retry(())
                }
            }
        });

        // TODO - check custom domains? if yes, why wasting time waiting for user setting up the custom domain?

        match check_result {
            Ok(_) => {}
            Err(_) => return Err(ServiceError::CheckFailed),
        }

        Ok(())
    }
}

impl StatelessService for Router {}

impl Create for Router {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.router.on_create() called for {}", self.name());
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let aws = kubernetes
            .cloud_provider()
            .as_any()
            .downcast_ref::<AWS>()
            .unwrap();

        let workspace_dir = self.workspace_directory();
        let helm_release_name = self.helm_release_name();

        let kubernetes_config_file_path = common::kubernetes_config_path(
            workspace_dir.as_str(),
            environment.organization_id.as_str(),
            kubernetes.id(),
            aws.access_key_id.as_str(),
            aws.secret_access_key.as_str(),
            kubernetes.region(),
        )?;

        // respect order - getting the context here and not before is mandatory
        // the nginx-ingress must be available to get the external dns target if necessary
        let mut context = self.tera_context(kubernetes, environment);

        if !self.custom_domains.is_empty() {
            // custom domains? create an NGINX ingress
            info!("setup NGINX ingress for custom domains");

            let into_dir = crate::fs::workspace_directory(
                self.context.workspace_root_dir(),
                self.context.execution_id(),
                "routers/nginx-ingress",
            );

            let from_dir = format!("{}/common/chart_values", self.context.lib_root_dir());
            let _ = crate::template::generate_and_copy_all_files_into_dir(
                from_dir.as_str(),
                into_dir.as_str(),
                &context,
            )?;

            let _ = crate::template::copy_non_template_files(
                format!(
                    "{}/common/charts/nginx-ingress",
                    self.context().lib_root_dir()
                ),
                into_dir.as_str(),
            )?;
            // do exec helm upgrade and return the last deployment status
            let helm_history_row = crate::cmd::helm::helm_exec_with_upgrade_history_with_override(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                format!("custom-{}", helm_release_name).as_str(),
                into_dir.as_str(),
                format!("{}/nginx-ingress.yaml", into_dir.as_str()).as_str(),
                self.aws_credentials_envs(aws).to_vec(),
            )?;

            // check deployment status
            if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
                return Err(ServiceError::OnCreateFailed);
            }
            // waiting for the nlb, it should be deploy to get fqdn
            let external_ingress_hostname_custom_result =
                retry::retry(Fibonacci::from_millis(3000).take(10), || {
                    let external_ingress_hostname_custom =
                        crate::cmd::kubectl::kubectl_exec_get_external_ingress_hostname(
                            kubernetes_config_file_path.as_str(),
                            environment.namespace(),
                            format!(
                                "app=nginx-ingress,component=controller,release=custom-{}",
                                helm_release_name
                            )
                            .as_str(),
                            self.aws_credentials_envs(aws).to_vec(),
                        );
                    match external_ingress_hostname_custom {
                        Ok(external_ingress_hostname_custom) => {
                            OperationResult::Ok(external_ingress_hostname_custom)
                        }
                        Err(err) => {
                            error!(
                                "Waiting NLB endpoint to be available to be able to configure TLS"
                            );
                            OperationResult::Retry(err)
                        }
                    }
                });
            match external_ingress_hostname_custom_result {
                Ok(elb) => {
                    //put it in the context
                    context.insert("nlb_ingress_hostname", &elb);
                }
                Err(e) => error!("Error getting the NLB endpoint to be able to configure TLS"),
            }
        }
        let from_dir = format!("{}/aws/charts/q-ingress-tls", self.context.lib_root_dir());
        let _ = crate::template::generate_and_copy_all_files_into_dir(
            from_dir.as_str(),
            workspace_dir.as_str(),
            &context,
        )?;

        // do exec helm upgrade and return the last deployment status
        let helm_history_row = crate::cmd::helm::helm_exec_with_upgrade_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            workspace_dir.as_str(),
            self.aws_credentials_envs(aws).to_vec(),
        )?;

        if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
            return Err(ServiceError::OnCreateFailed);
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), ServiceError> {
        let check_result = retry::retry(Fixed::from_millis(3000).take(60), || {
            let rs_ips = lookup_host(self.default_domain.as_str());
            match rs_ips {
                Ok(ips) => {
                    info!("Records from DNS are successfully retrieved.");
                    OperationResult::Ok(ips)
                }
                Err(e) => {
                    warn!("Failed to retrieve record from DNS, retrying");
                    OperationResult::Retry(e)
                }
            }
        });
        match check_result {
            Ok(out) => Ok(()),
            Err(e) => {
                error!("While checking the DNS propagation");
                let listeners_helper = ListenersHelper::new(&self.listeners);
                listeners_helper.error(ProgressInfo::new(
                    ProgressScope::Router {
                        id: self.id.to_string(),
                    },
                    ProgressLevel::Error,
                    Some("DNS propagation goes wrong."),
                    self.context.execution_id(),
                ));
                Err(ServiceError::CheckFailed)
            }
        }
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!("AWS.router.on_create_error() called for {}", self.name());
        self.delete(target)
    }
}

impl Pause for Router {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.router.on_pause() called for {}", self.name());
        self.delete(target)
    }

    fn on_pause_check(&self) -> Result<(), ServiceError> {
        warn!("AWS.router.on_pause_error() called for {}", self.name());
        // TODO check resource has been cleaned?
        Ok(())
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        self.delete(target)
    }
}

impl Delete for Router {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.router.on_delete() called for {}", self.name());
        self.delete(target)
    }

    fn on_delete_check(&self) -> Result<(), ServiceError> {
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!("AWS.router.on_delete_error() called for {}", self.name());
        self.delete(target)
    }
}

pub struct CustomDomain {
    pub domain: String,
    pub target_domain: String,
}

#[derive(Serialize, Deserialize)]
struct CustomDomainDataTemplate {
    pub domain: String,
    pub domain_hash: String,
    pub target_domain: String,
}

pub struct Route {
    pub path: String,
    pub application_name: String,
}

#[derive(Serialize, Deserialize)]
struct RouteDataTemplate {
    pub path: String,
    pub application_name: String,
    pub application_port: u16,
}
