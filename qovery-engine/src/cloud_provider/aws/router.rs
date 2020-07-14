use std::borrow::Borrow;

use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::aws::kubernetes::EKS;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::service::{Create, Delete, Service, ServiceError, ServiceType};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd::{helm_exec, helm_exec_with_named_args};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};

pub struct Router {
    pub id: String,
    pub name: String,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

impl Router {
    fn helm_release_name(&self) -> String {
        format!("router-{}", self.id())
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

impl<'a> Create<AWS, EKS<'a>> for Router {
    fn on_create(&self, target: &DeploymentTarget<AWS, EKS>) -> Result<(), ServiceError> {
        info!("EKS.router.on_create() called for {}", self.name());
        let environment = match target {
            DeploymentTarget::ManagedServices(c, env) => *env,
            DeploymentTarget::SelfHosted(k, env) => *env,
        };

        let context = Context::new();
        // TODO add context variables

        if !self.custom_domains.is_empty() {
            // custom domains? create an NGINX ingress
            info!("setup NGINX ingress for custom domains");
            let into_dir = crate::fs::workspace_directory("charts/router/nginx-ingress");

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

        let temp_dir = crate::fs::workspace_directory("charts/router/q-ingress-tls");

        let _ =
            crate::fs::copy_non_template_files("lib/aws/charts/q-ingress-tls", temp_dir.as_str())?;

        let _ = crate::fs::generate_and_copy_j2_files_into_dir(
            "lib/aws/charts/q-ingress-tls",
            temp_dir.as_str(),
            &context,
        )?;

        // render

        // TODO check the rendered files?
        let helm_release_name = self.helm_release_name();
        let helm_envs = vec![(AWS_ACCESS_KEY_ID, ""), (AWS_SECRET_ACCESS_KEY, "")];

        let _ = helm_exec_with_named_args(
            temp_dir.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            temp_dir.as_str(),
            helm_envs,
        )?;

        // TODO render helm common config and apply
        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget<AWS, EKS>) -> Result<(), ServiceError> {
        warn!("EKS.router.on_create_error() called for {}", self.name());
        unimplemented!()
    }
}

impl<'a> Delete<AWS, EKS<'a>> for Router {
    fn on_delete(&self, target: &DeploymentTarget<AWS, EKS>) -> Result<(), ServiceError> {
        info!("EKS.router.on_delete() called for {}", self.name());
        unimplemented!()
    }

    fn on_delete_error(&self, target: &DeploymentTarget<AWS, EKS>) -> Result<(), ServiceError> {
        warn!("EKS.router.on_delete_error() called for {}", self.name());
        unimplemented!()
    }
}

pub struct CustomDomain {}

pub struct Route {}
