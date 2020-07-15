use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::service::{Create, Delete, Service, ServiceError, ServiceType};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use rusoto_core::Region;
use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::str::FromStr;

pub struct Router {
    id: String,
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: Region,
    custom_domains: Vec<CustomDomain>,
    routes: Vec<Route>,
}

impl Router {
    pub fn new(
        id: &str,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
        custom_domains: Vec<CustomDomain>,
        routes: Vec<Route>,
    ) -> Self {
        Router {
            id: id.to_string(),
            name: name.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: Region::from_str(region).unwrap(),
            custom_domains,
            routes,
        }
    }

    fn helm_release_name(&self) -> String {
        format!("router-{}", self.id())
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(format!("charts/routers/{}", self.id()))
    }

    fn kubernetes_config_path(&self) -> Result<String, Error> {
        let kubernetes_config_bucket_name = ""; // FIXME
        let kubernetes_config_object_key = ""; // FIXME

        let workspace_directory = self.workspace_directory();
        let kubernetes_config_file_path =
            format!("{}/kubernetes_config", workspace_directory.as_str());

        let _ = crate::s3::get_kubernetes_config_file(
            self.access_key_id.as_str(),
            self.secret_access_key.as_str(),
            &self.region,
            kubernetes_config_bucket_name,
            kubernetes_config_object_key,
            kubernetes_config_file_path.as_str(),
        )?;

        Ok(kubernetes_config_file_path)
    }
}

impl<'a> Service for Router {
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
        // FIXME
        Ok(())
    }
}

impl Create for Router {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("EKS.router.on_create() called for {}", self.name());
        let environment = match target {
            DeploymentTarget::ManagedServices(_, env) => *env,
            DeploymentTarget::SelfHosted(_, env) => *env,
        };

        let context = Context::new();
        // TODO add context variables

        if !self.custom_domains.is_empty() {
            // custom domains? create an NGINX ingress
            info!("setup NGINX ingress for custom domains");
            let into_dir = crate::fs::workspace_directory("charts/routers/nginx-ingress");

            let _ = crate::fs::copy_non_template_files(
                "lib/common/charts/nginx-ingress",
                into_dir.as_str(),
            )?;

            let _ = crate::fs::generate_and_copy_j2_files_into_dir(
                "lib/common/charts/nginx-ingress",
                into_dir.as_str(),
                &context,
            )?;

            // TODO check the rendered files?
        }

        let workspace_dir = self.workspace_directory();

        let _ = crate::fs::copy_non_template_files(
            "lib/aws/charts/q-ingress-tls",
            workspace_dir.as_str(),
        )?;

        let _ = crate::fs::generate_and_copy_j2_files_into_dir(
            "lib/aws/charts/q-ingress-tls",
            workspace_dir.as_str(),
            &context,
        )?;

        // render
        // TODO check the rendered files?
        let helm_release_name = self.helm_release_name();
        let helm_envs = vec![
            (AWS_ACCESS_KEY_ID, self.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, self.secret_access_key.as_str()),
        ];

        let kubernetes_config_file_path = self.kubernetes_config_path()?;

        let _ = crate::cmd::helm_exec_with_named_args(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            workspace_dir.as_str(),
            helm_envs,
        )?;

        // TODO render helm common config and apply
        // TODO check deployment error with helm
        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!("EKS.router.on_create_error() called for {}", self.name());
        unimplemented!()
    }
}

impl Delete for Router {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("EKS.router.on_delete() called for {}", self.name());
        unimplemented!()
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!("EKS.router.on_delete_error() called for {}", self.name());
        unimplemented!()
    }
}

pub struct CustomDomain {}

pub struct Route {}
