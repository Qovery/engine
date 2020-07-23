use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::str::FromStr;

use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Create, Delete, Pause, Router as RRouter, Service, ServiceError, ServiceType, StatelessService,
};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd::CmdError;
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use dns_lookup::lookup_host;
use itertools::enumerate;
use retry::delay::Exponential;
use retry::OperationResult;
use serde::{Deserialize, Serialize};

pub struct Router {
    execution_id: String,
    id: String,
    name: String,
    default_domain: String,
    custom_domains: Vec<CustomDomain>,
    routes: Vec<Route>,
}

impl Router {
    pub fn new(
        execution_id: &str,
        id: &str,
        name: &str,
        default_domain: &str,
        custom_domains: Vec<CustomDomain>,
        routes: Vec<Route>,
    ) -> Self {
        Router {
            execution_id: execution_id.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            default_domain: default_domain.to_string(),
            custom_domains,
            routes,
        }
    }

    fn helm_release_name(&self) -> String {
        format!("router-{}", self.id())
    }

    fn helm_envs<'a>(&self, aws: &'a AWS) -> [(&'a str, &'a str); 2] {
        [
            (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
        ]
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(self.execution_id(), format!("charts/routers/{}", self.id()))
    }

    fn context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> Context {
        let mut context = self.default_context(kubernetes, environment);

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
                // FIXME unsafe to unwrap here?
                let application = applications
                    .iter()
                    .find(|app| app.id() == r.application_id.as_str())
                    .unwrap();

                RouteDataTemplate {
                    path: r.path.clone(),
                    application_name: application.name().to_string(),
                    application_port: application.private_port(),
                }
            })
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

        if kubernetes_config_file_path.is_ok() {
            // it should never occurred.. but in case of..
            match crate::cmd::kubectl_exec_get_external_ingress_hostname(
                kubernetes_config_file_path.unwrap().as_str(),
                environment.namespace(),
                "app=nginx-ingress,component=controller",
                self.helm_envs(aws).to_vec(),
            ) {
                Ok(external_ingress_hostname) => match external_ingress_hostname {
                    Some(hostname) => {
                        context.insert("external_ingress_hostname", hostname.as_str())
                    }
                    None => {
                        warn!("unable to get external_ingress_hostname - what's wrong? This should never occurred");
                    }
                },
                _ => {
                    warn!("can't fetch kubernetes config file - what's wrong? This should never occurred");
                }
            }
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
        context.insert("spec_acme_email", "tls@qovery.com");
        context.insert(
            "metadata_annotations_cert_manager_cluster_issuer",
            "letsencrypt-qovery",
        );
        context.insert(
            "spec_acme_server",
            "https://acme-v02.api.letsencrypt.org/directory",
        );

        context
    }
}

impl<'a> Service for Router {
    fn execution_id(&self) -> &str {
        self.execution_id.as_str()
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

    fn private_port(&self) -> u16 {
        0
    }
}

impl crate::cloud_provider::service::Router for Router {
    fn check_domains(&self) -> Result<(), ServiceError> {
        for custom_domain in &self.custom_domains {
            let check_result = retry::retry(Exponential::from_millis(1000).take(5), || {
                // TODO send information back to the core - does the custom domain is linked?
                info!("check custom domain {}", custom_domain.domain.as_str());
                match lookup_host(custom_domain.domain.as_str()) {
                    Ok(_) => OperationResult::Ok(()),
                    Err(err) => {
                        debug!("{:?}", err);
                        OperationResult::Retry(())
                    }
                }
            });

            match check_result {
                Ok(_) => {}
                Err(_) => return Err(ServiceError::CheckFailed),
            }
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

        if !self.custom_domains.is_empty() {
            // custom domains? create an NGINX ingress
            info!("setup NGINX ingress for custom domains");

            let into_dir =
                crate::fs::workspace_directory(self.execution_id(), "charts/routers/nginx-ingress");

            // copy nginx-ingress files, there is no templates so do not generate anything and
            // simply copy/paste files into our working dir
            let _ = crate::template::copy_non_template_files(
                "lib/common/charts/nginx-ingress",
                into_dir.as_str(),
            )?;

            // TODO exec helm to apply
            // do exec helm upgrade and return the last deployment status
            let helm_history_row = crate::cmd::helm_exec_with_upgrade_history(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name.as_str(), // FIXME change helm release name?
                into_dir.as_str(),
                self.helm_envs(aws).to_vec(),
            )?;

            // check deployment status
            if !helm_history_row.is_successfully_deployed() {
                return Err(ServiceError::OnCreateFailed);
            }
        }

        // respect order - getting the context here and not before is mandatory
        // the nginx-ingress must be available to get the external dns target if necessary
        let context = self.context(kubernetes, environment);

        let _ = crate::template::generate_and_copy_all_files_into_dir(
            "lib/aws/charts/q-ingress-tls",
            workspace_dir.as_str(),
            &context,
        )?;

        // do exec helm upgrade and return the last deployment status
        let helm_history_row = crate::cmd::helm_exec_with_upgrade_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            workspace_dir.as_str(),
            self.helm_envs(aws).to_vec(),
        )?;

        // check deployment status
        if !helm_history_row.is_successfully_deployed() {
            return Err(ServiceError::OnCreateFailed);
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), ServiceError> {
        self.check_domains()
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!("AWS.router.on_create_error() called for {}", self.name());
        unimplemented!()
    }
}

impl Pause for Router {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }
}

impl Delete for Router {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.router.on_delete() called for {}", self.name());
        unimplemented!()
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!("AWS.router.on_delete_error() called for {}", self.name());
        unimplemented!()
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
    pub application_id: String,
}

#[derive(Serialize, Deserialize)]
struct RouteDataTemplate {
    pub path: String,
    pub application_name: String,
    pub application_port: u16,
}
