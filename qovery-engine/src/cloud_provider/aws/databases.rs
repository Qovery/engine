use std::io::Error;
use std::str::FromStr;

use rusoto_core::Region;
use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Backup, Create, DatabaseOptions, DatabaseType, Delete, Downgrade, Pause, Service, ServiceError,
    ServiceType, StatefulService, Upgrade,
};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};

pub struct PostgreSQL {
    execution_id: String,
    id: String,
    name: String,
    version: String,
    options: DatabaseOptions,
}

impl PostgreSQL {
    pub fn new(
        execution_id: &str,
        id: &str,
        name: &str,
        version: &str,
        options: DatabaseOptions,
    ) -> Self {
        PostgreSQL {
            execution_id: execution_id.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            options,
        }
    }

    fn helm_release_name(&self) -> String {
        format!("postgresql-{}", self.id())
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(
            self.execution_id(),
            format!("databases/postgresql-{}", self.id()),
        )
    }

    fn context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> Context {
        let mut context = self.default_context(kubernetes, environment);

        context.insert("database_login", self.options.login.as_str());
        context.insert("database_password", self.options.password.as_str());
        context.insert("database_port", &self.private_port());
        context.insert("database_disk_size_in_gib", &self.options.disk_size_in_gib);
        context.insert("database_instance_type", "db.t2.micro"); // TODO customizable
        context.insert("database_disk_type", "gp2"); // TODO customizable

        context
    }
}

impl StatefulService for PostgreSQL {}

impl Service for PostgreSQL {
    fn execution_id(&self) -> &str {
        self.execution_id.as_str()
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Database(DatabaseType::PostgreSQL(&self.options))
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.version.as_str()
    }

    fn private_port(&self) -> u16 {
        self.options.port
    }
}

impl Create for PostgreSQL {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.PostgreSQL.on_create() called for {}", self.name());

        let workspace_dir = self.workspace_directory();

        match target {
            DeploymentTarget::ManagedServices(kubernetes, environment) => {
                // use terraform
                info!("deploy PostgreSQL on AWS RDS for {}", self.name());

                let context = self.context(*kubernetes, *environment);

                let _ = crate::template::generate_and_copy_all_files_into_dir(
                    "lib/aws/services/postgresql",
                    workspace_dir.as_str(),
                    &context,
                )?;

                crate::cmd::terraform_exec_with_init_validate_plan_apply(
                    workspace_dir.as_str(),
                    false,
                )?;
            }
            DeploymentTarget::SelfHosted(kubernetes, environment) => {
                // use helm
                info!("deploy PostgreSQL on Kubernetes for {}", self.name());

                let context = self.context(*kubernetes, *environment);

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
                )?;

                let _ = crate::template::generate_and_copy_all_files_into_dir(
                    "lib/common/services/postgresql",
                    workspace_dir.as_str(),
                    &context,
                )?;

                // render templates
                let helm_release_name = self.helm_release_name();
                let aws_credentials_envs = vec![
                    (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
                    (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
                ];

                // do exec helm upgrade and return the last deployment status
                let helm_history_row = crate::cmd::helm_exec_with_upgrade_history(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    helm_release_name.as_str(),
                    workspace_dir.as_str(),
                    aws_credentials_envs.clone(),
                )?;

                // check deployment status
                if helm_history_row.is_none()
                    || !helm_history_row.unwrap().is_successfully_deployed()
                {
                    return Err(ServiceError::OnCreateFailed);
                }

                // check app status
                match crate::cmd::kubectl_exec_is_application_ready_with_retry(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    self.name(),
                    aws_credentials_envs,
                ) {
                    Ok(Some(true)) => {}
                    _ => return Err(ServiceError::OnCreateFailed),
                }
            }
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), ServiceError> {
        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "AWS.PostgreSQL.on_create_error() called for {}",
            self.name()
        );

        let workspace_dir = self.workspace_directory();

        match target {
            DeploymentTarget::ManagedServices(_, _) => {
                // TODO what to do with a PostgreSQL that is badly deployed on RDS?
            }
            DeploymentTarget::SelfHosted(kubernetes, environment) => {
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
                )?;

                let helm_release_name = self.helm_release_name();
                let helm_envs = vec![
                    (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
                    (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
                ];

                let history_rows = crate::cmd::helm_exec_history(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    helm_release_name.as_str(),
                    helm_envs.clone(),
                )?;

                // if there is no valid history - then delete the helm chart
                let first_valid_history_row =
                    history_rows.iter().find(|x| x.is_successfully_deployed());

                if first_valid_history_row.is_none() {
                    info!(
                        "there is no valid deployment for {} {} - let's delete it",
                        self.name(),
                        self.id()
                    );

                    crate::cmd::helm_exec_uninstall(
                        kubernetes_config_file_path.as_str(),
                        environment.namespace(),
                        helm_release_name.as_str(),
                        helm_envs,
                    )?;
                }
            }
        }

        Ok(())
    }
}

impl Pause for PostgreSQL {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }
}

impl Delete for PostgreSQL {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.PostgreSQL.on_delete() called for {}", self.name());

        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "AWS.PostgreSQL.on_create_error() called for {}",
            self.name()
        );

        Ok(())
    }
}

impl crate::cloud_provider::service::Clone for PostgreSQL {
    fn on_clone(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_clone_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }
}

impl Upgrade for PostgreSQL {
    fn on_upgrade(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_upgrade_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }
}

impl Downgrade for PostgreSQL {
    fn on_downgrade(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_downgrade_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }
}

impl Backup for PostgreSQL {
    fn on_backup(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_backup_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_restore(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn on_restore_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        unimplemented!()
    }
}
