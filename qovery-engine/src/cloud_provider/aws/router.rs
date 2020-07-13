use crate::build_platform::Image;
use crate::cloud_provider::service::{Create, Delete, Service, ServiceError, ServiceType};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use tera::Context;

pub struct Router {
    pub id: String,
    pub name: String,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
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
    fn on_create(&self, _: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("EKS.router.on_create() called for {}", self.name());
        if !self.custom_domains.is_empty() {
            // TODO custom domains? create an NGINX ingress
            let into_dir = crate::fs::workspace_directory("charts/nginx-ingress");

            let _ = crate::fs::copy_chart_directory(
                "lib/aws/charts/nginx-ingress",
                "lib/common/charts/nginx-ingress",
                into_dir.as_str(),
            )?;

            let context = Context::new();
            let _ = crate::fs::generate_and_copy_j2_files_into_dir(
                "lib/aws/charts/nginx-ingress",
                into_dir.as_str(),
                &context,
            )?;
        }

        // TODO render helm common config and apply
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
