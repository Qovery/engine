use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::service::{
    Create, Delete, Service, ServiceError, ServiceType, StatefulService, StatelessService,
};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Application {
    pub execution_id: String,
    pub id: String,
    pub name: String,
    pub image: Image,
}

impl Application {
    pub fn new(execution_id: &str, id: &str, name: &str, image: Image) -> Self {
        Application {
            execution_id: execution_id.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            image,
        }
    }

    fn helm_release_name(&self) -> String {
        format!("application-{}-{}", self.name(), self.id())
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(
            self.execution_id(),
            format!("applications/{}-{}", self.name(), self.id()),
        )
    }
}

impl crate::cloud_provider::service::Application for Application {
    fn image(&self) -> &Image {
        &self.image
    }
}

impl StatelessService for Application {}

impl Service for Application {
    fn execution_id(&self) -> &str {
        self.execution_id.as_str()
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Application
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.image.commit_id.as_str()
    }

    fn private_port(&self) -> u16 {
        8080 // TODO it's customizable by the user
    }
}

impl Create for Application {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.application.on_create() called for {}", self.name());
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let aws = kubernetes
            .cloud_provider()
            .as_any()
            .downcast_ref::<AWS>()
            .unwrap();

        let context = Context::new();
        // TODO add context variables

        let workspace_dir = self.workspace_directory();

        let _ = crate::template::generate_and_copy_all_files_into_dir(
            "lib/aws/charts/q-application",
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
        warn!(
            "AWS.application.on_create_error() called for {}",
            self.name()
        );

        // FIXME
        Ok(())
    }
}

impl Delete for Application {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.application.on_delete() called for {}", self.name());

        // FIXME
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "AWS.application.on_delete_error() called for {}",
            self.name()
        );

        // FIXME
        Ok(())
    }
}
