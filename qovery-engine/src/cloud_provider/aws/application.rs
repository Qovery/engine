use chrono::Duration;
use retry::delay::{jitter, Exponential};
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Application as CApplication, Create, Delete, Pause, Service, ServiceError, ServiceType,
    StatefulService, StatelessService,
};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd::CmdError;
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Application {
    execution_id: String,
    id: String,
    name: String,
    private_port: Option<u16>,
    image: Image,
    storage: Vec<Storage>,
    environment_variables: Vec<EnvironmentVariable>,
}

impl Application {
    pub fn new(
        execution_id: &str,
        id: &str,
        name: &str,
        private_port: Option<u16>,
        image: Image,
        storage: Vec<Storage>,
        environment_variables: Vec<EnvironmentVariable>,
    ) -> Self {
        Application {
            execution_id: execution_id.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            private_port,
            image,
            storage,
            environment_variables,
        }
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.name(), self.id()), 50)
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(self.execution_id(), format!("applications/{}", self.name()))
    }

    fn context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> Context {
        let mut context = self.default_context(kubernetes, environment);
        let commit_id = self.image().commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);

        match &self.image().registry_url {
            Some(registry_url) => context.insert("image_name_with_tag", registry_url.as_str()),
            None => {
                let image_name_with_tag = self.image().name_with_tag();
                warn!("there is no registry url, use image name with tag with the default container registry: {}", image_name_with_tag.as_str());
                context.insert("image_name_with_tag", image_name_with_tag.as_str());
            }
        }

        let environment_variables = self
            .environment_variables
            .iter()
            .map(|ev| EnvironmentVariableDataTemplate {
                key: ev.key.clone(),
                value: ev.value.clone(),
            })
            .collect::<Vec<_>>();

        context.insert("environment_variables", &environment_variables);

        let storage = self
            .storage
            .iter()
            .map(|s| StorageDataTemplate {
                id: s.id.clone(),
                name: s.name.clone(),
                storage_type: match s.storage_type {
                    StorageType::SC1 => "sc1",
                    StorageType::ST1 => "st1",
                    StorageType::GP2 => "gp2",
                    StorageType::IO1 => "io1",
                }
                .to_string(),
                size_in_gib: s.size_in_gib,
                mount_point: s.mount_point.clone(),
                snapshot_retention_in_days: s.snapshot_retention_in_days,
            })
            .collect::<Vec<_>>();

        let is_storage = storage.len() > 0;

        context.insert("storage", &storage);
        context.insert("is_storage", &is_storage);
        context.insert("clone", &false);

        context
    }
}

impl crate::cloud_provider::service::Application for Application {
    fn image(&self) -> &Image {
        &self.image
    }

    fn set_image(&mut self, image: Image) {
        self.image = image;
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

    fn private_port(&self) -> Option<u16> {
        self.private_port
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

        let context = self.context(kubernetes, environment);
        let workspace_dir = self.workspace_directory();

        let _ = crate::template::generate_and_copy_all_files_into_dir(
            "lib/aws/charts/q-application",
            workspace_dir.as_str(),
            &context,
        )?;

        // render
        // TODO check the rendered files?
        let helm_release_name = self.helm_release_name();
        let aws_credentials_envs = vec![
            (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
        ];

        let kubernetes_config_file_path = common::kubernetes_config_path(
            workspace_dir.as_str(),
            environment.organization_id.as_str(),
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
            aws_credentials_envs.clone(),
        )?;

        // check deployment status
        if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
            // TODO get pod output by using kubectl and return it into the OnCreateFailed
            return Err(ServiceError::OnCreateFailed);
        }

        // check app status
        let selector = format!("app={}", self.name());

        match crate::cmd::kubectl_exec_is_application_ready_with_retry(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector.as_str(),
            aws_credentials_envs,
        ) {
            Ok(Some(true)) => {}
            _ => return Err(ServiceError::OnCreateFailed),
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), ServiceError> {
        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "AWS.application.on_create_error() called for {}",
            self.name()
        );

        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let workspace_dir = self.workspace_directory();
        let helm_release_name = self.helm_release_name();
        let selector = format!("app={}", self.name());

        let _ = common::get_stateless_resource_information(
            kubernetes,
            environment,
            workspace_dir.as_str(),
            selector.as_str(),
        )?;

        // clean the resource
        let _ = common::do_stateless_service_cleanup(
            kubernetes,
            environment,
            workspace_dir.as_str(),
            helm_release_name.as_str(),
        )?;

        Ok(())
    }
}

impl Pause for Application {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
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

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize)]
struct EnvironmentVariableDataTemplate {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Storage {
    pub id: String,
    pub name: String,
    pub storage_type: StorageType,
    pub size_in_gib: u16,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum StorageType {
    SC1,
    ST1,
    GP2,
    IO1,
}

#[derive(Serialize, Deserialize)]
struct StorageDataTemplate {
    pub id: String,
    pub name: String,
    pub storage_type: String,
    pub size_in_gib: u16,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}
