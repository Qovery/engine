use std::io::Error;
use std::str::FromStr;

use rusoto_core::Region;
use tera::Context;

use crate::build_platform::Image;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::service::{
    Create, DatabaseOptions, DatabaseType, Delete, Service, ServiceError, ServiceType,
};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};

pub struct PostgreSQL {
    id: String,
    name: String,
    version: String,
    options: DatabaseOptions,
}

impl PostgreSQL {
    pub fn new(id: &str, name: &str, version: &str, options: DatabaseOptions) -> Self {
        PostgreSQL {
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
        crate::fs::workspace_directory(format!("charts/databases/postgresql-{}", self.id()))
    }

    fn kubernetes_config_path(
        &self,
        owner_id: &str,
        kubernetes_cluster_id: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
    ) -> Result<String, Error> {
        let kubernetes_config_bucket_name = format!("kubeconfigs-{}", owner_id); // FIXME
        let kubernetes_config_object_key = format!("{}-{}", region, kubernetes_cluster_id); // FIXME

        let workspace_directory = self.workspace_directory();
        let kubernetes_config_file_path =
            format!("{}/kubernetes_config", workspace_directory.as_str());

        let _region = Region::from_str(region).unwrap();

        let _ = crate::s3::get_kubernetes_config_file(
            access_key_id,
            secret_access_key,
            &_region,
            kubernetes_config_bucket_name.as_str(),
            kubernetes_config_object_key.as_str(),
            kubernetes_config_file_path.as_str(),
        )?;

        Ok(kubernetes_config_file_path)
    }
}

impl Service for PostgreSQL {
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

    fn is_valid(&self) -> Result<(), ServiceError> {
        let binaries = ["helm", "terraform"];

        for binary in binaries.iter() {
            if !crate::cmd::does_command_exists(binary) {
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
                let _ = crate::fs::generate_and_copy_all_files_into_dir(
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

                let kubernetes_config_file_path = self.kubernetes_config_path(
                    environment.owner_id.as_str(),
                    kubernetes.id(),
                    aws.access_key_id.as_str(),
                    aws.secret_access_key.as_str(),
                    kubernetes.region(),
                )?;

                let _ = crate::fs::generate_and_copy_all_files_into_dir(
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

                // do exec helm upgrade
                let _ = crate::cmd::helm_exec_upgrade(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    helm_release_name.as_str(),
                    workspace_dir.as_str(),
                    helm_envs.clone(),
                )?;

                // list helm history
                let helm_history_rows = crate::cmd::helm_exec_history(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    helm_release_name.as_str(),
                    helm_envs,
                )?;

                // take the last deployment from helm history
                let helm_history_row = match helm_history_rows.first() {
                    Some(helm_history_row) => helm_history_row,
                    None => {
                        // should never happened
                        return Err(ServiceError::Unexpected(
                            "helm history is empty - what's wrong?".to_string(),
                        ));
                    }
                };

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
