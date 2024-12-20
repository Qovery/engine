use crate::cmd;
use crate::cmd::kubectl::kubectl_exec_delete_secret;
use crate::cmd::terraform_validators::TerraformValidators;
use crate::environment::action::DeploymentAction;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::infrastructure::models::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::template::generate_and_copy_all_files_into_dir;
use serde_json::Value;
use std::path::PathBuf;
use tera::Context as TeraContext;

pub struct TerraformDeployment {
    tera_context: TeraContext,
    terraform_common_folder: PathBuf,
    terraform_resource_folder: PathBuf,
    destination_folder: PathBuf,
    event_details: EventDetails,
    is_dry_run: bool,
}

impl TerraformDeployment {
    pub fn new(
        tera_context: TeraContext,
        terraform_common_folder: PathBuf,
        terraform_resource_folder: PathBuf,
        destination_folder: PathBuf,
        event_details: EventDetails,
        is_dry_run: bool,
    ) -> TerraformDeployment {
        TerraformDeployment {
            tera_context,
            terraform_common_folder,
            terraform_resource_folder,
            destination_folder,
            event_details,
            is_dry_run,
        }
    }

    fn prepare_terraform_files(&self) -> Result<(), Box<EngineError>> {
        // Copy the root folder
        generate_and_copy_all_files_into_dir(
            &self.terraform_common_folder,
            &self.destination_folder,
            &self.tera_context,
        )
        .map_err(|e| {
            EngineError::new_cannot_copy_files_from_one_directory_to_another(
                self.event_details.clone(),
                self.terraform_common_folder.to_string_lossy().to_string(),
                self.destination_folder.to_string_lossy().to_string(),
                e,
            )
        })?;

        // If we have some special value override, replace it also
        generate_and_copy_all_files_into_dir(
            &self.terraform_resource_folder,
            &self.destination_folder,
            &self.tera_context,
        )
        .map_err(|e| {
            EngineError::new_cannot_copy_files_from_one_directory_to_another(
                self.event_details.clone(),
                self.terraform_resource_folder.to_string_lossy().to_string(),
                self.destination_folder.to_string_lossy().to_string(),
                e,
            )
        })?;

        Ok(())
    }

    pub fn delete_tfstate_secret(
        kubernetes: &dyn Kubernetes,
        cloud_provider: &dyn CloudProvider,
        namespace: &str,
        secret_name: &str,
    ) -> Result<(), Box<EngineError>> {
        // create the namespace to insert the tfstate in secrets
        let _ = kubectl_exec_delete_secret(
            kubernetes.kubeconfig_local_file_path(),
            namespace,
            secret_name,
            cloud_provider.credentials_environment_variables(),
        );

        Ok(())
    }
}

impl DeploymentAction for TerraformDeployment {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        self.prepare_terraform_files()?;
        let ret = cmd::terraform::terraform_init_validate_plan_apply(
            &self.destination_folder.to_string_lossy(),
            self.is_dry_run,
            target.cloud_provider.credentials_environment_variables().as_slice(),
            &TerraformValidators::Default,
        );

        if let Err(err) = ret {
            Err(Box::new(EngineError::new_terraform_error(self.event_details.clone(), err)))
        } else {
            Ok(())
        }
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        self.prepare_terraform_files()?;
        match cmd::terraform::terraform_init_validate_destroy(
            &self.destination_folder.to_string_lossy(),
            target.cloud_provider.credentials_environment_variables().as_slice(),
            &TerraformValidators::None,
        ) {
            Ok(_) => {
                if let Err(err) = TerraformDeployment::delete_tfstate_secret(
                    target.kubernetes,
                    target.cloud_provider,
                    target.environment.namespace(),
                    self.tera_context.get("tfstate_name").and_then(Value::as_str).unwrap(),
                ) {
                    warn!("Cannot delete tfstate {} for {:?}", err, self.tera_context);
                }
                Ok(())
            }
            Err(e) => Err(Box::new(EngineError::new_terraform_error(self.event_details.clone(), e))),
        }
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let command_error =
            CommandError::new_from_safe_message("Cannot restart Terraform managed resource".to_string());
        return Err(Box::new(EngineError::new_cannot_restart_service(
            EventDetails::clone_changing_stage(
                self.event_details.clone(),
                Stage::Environment(EnvironmentStep::Restart),
            ),
            target.environment.namespace(),
            "",
            command_error,
        )));
    }
}
