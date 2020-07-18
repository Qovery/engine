use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::str::FromStr;

use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::service::{
    Create, Delete, Service, ServiceError, ServiceType, StatelessService,
};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};

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

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(self.execution_id(), format!("charts/routers/{}", self.id()))
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

    fn is_valid(&self) -> Result<(), ServiceError> {
        Ok(())
    }
}

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

        let mut context = Context::new();
        // TODO set the template vars for the router
        // TODO lib/aws/charts/q-ingress-tls/**
        context.insert("domain", "");

        if !self.custom_domains.is_empty() {
            // custom domains? create an NGINX ingress
            info!("setup NGINX ingress for custom domains");

            let into_dir =
                crate::fs::workspace_directory(self.execution_id(), "charts/routers/nginx-ingress");

            let _ = crate::template::generate_and_copy_all_files_into_dir(
                "lib/common/charts/nginx-ingress",
                into_dir.as_str(),
                &context,
            )?;

            // TODO check the rendered files?
        }

        let workspace_dir = self.workspace_directory();

        let _ = crate::template::generate_and_copy_all_files_into_dir(
            "lib/aws/charts/q-ingress-tls",
            workspace_dir.as_str(),
            &context,
        )?;

        // render
        // TODO check the rendered files?
        let helm_release_name = self.helm_release_name();
        let helm_envs = vec![
            (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
        ];

        let kubernetes_config_file_path = common::kubernetes_config_path(
            workspace_dir.as_str(),
            environment.owner_id.as_str(),
            kubernetes.id(),
            aws.access_key_id.as_str(),
            aws.secret_access_key.as_str(),
            kubernetes.region(),
        )?;

        // do exec helm upgrade and return the last deployment status
        let helm_history_row = crate::cmd::helm_exec_with_upgrade_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            workspace_dir.as_str(),
            helm_envs,
        )?;

        // check deployment status
        if !helm_history_row.is_successfully_deployed() {
            return Err(ServiceError::DeploymentFailed);
        }

        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!("AWS.router.on_create_error() called for {}", self.name());
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

impl StatelessService for Router {}

pub struct CustomDomain {
    pub domain: String,
    pub target_domain: String,
}

pub struct Route {
    pub path: String,
    pub application_id: String,
}
