use std::io::Error;
use std::str::FromStr;

use rusoto_core::Region;
use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::service::{
    Backup, Create, DatabaseOptions, DatabaseType, Delete, Downgrade, Service, ServiceError,
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

    fn is_valid(&self) -> Result<(), ServiceError> {
        let binaries = ["helm", "terraform", "aws-iam-authenticator"];

        for binary in binaries.iter() {
            if !crate::cmd::does_binary_exist(binary) {
                let err = format!("{} binary not found", binary);
                return Err(ServiceError::Unexpected(err));
            }
        }

        // TODO check lib directories available

        Ok(())
    }
}

impl Create for PostgreSQL {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.PostgreSQL.on_create() called for {}", self.name());

        let context = Context::new();
        // TODO

        let workspace_dir = self.workspace_directory();

        match target {
            DeploymentTarget::ManagedServices(_, _) => {
                // use terraform
                info!("deploy PostgreSQL on AWS RDS for {}", self.name());
                let _ = crate::template::generate_and_copy_all_files_into_dir(
                    "lib/aws/services/postgresql",
                    workspace_dir.as_str(),
                    &context,
                )?;

                crate::cmd::terraform_exec_with_init_validate_plan_apply(
                    workspace_dir.as_str(),
                    false,
                )?;

                // TODO check terraform deployment?
            }
            DeploymentTarget::SelfHosted(kubernetes, environment) => {
                // use helm
                info!("deploy PostgreSQL on Kubernetes for {}", self.name());
                let aws = kubernetes
                    .cloud_provider()
                    .as_any()
                    .downcast_ref::<AWS>()
                    .unwrap();

                let kubernetes_config_file_path = common::kubernetes_config_path(
                    workspace_dir.as_str(),
                    environment.owner_id.as_str(),
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
                let helm_envs = vec![
                    (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
                    (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
                ];

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
            }
        }

        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "AWS.PostgreSQL.on_create_error() called for {}",
            self.name()
        );

        Ok(())
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
