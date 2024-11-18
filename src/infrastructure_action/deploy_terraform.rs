use crate::cmd::terraform::{
    terraform_apply, terraform_apply_with_tf_workers_resources, terraform_destroy, terraform_init_validate,
    terraform_output, terraform_plan, terraform_state_list,
};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::infrastructure_action::InfraLogger;
use crate::template::generate_and_copy_all_files_into_dir;
use crate::utilities::envs_to_slice;
use itertools::Itertools;
use serde::de::DeserializeOwned;
use std::path::PathBuf;
use tera::Context as TeraContext;

pub struct TerraformInfraResources {
    tera_context: TeraContext,
    terraform_common_folder: PathBuf,
    destination_folder: PathBuf,
    event_details: EventDetails,
    envs: Vec<(String, String)>,
    is_dry_run: bool,
}

impl TerraformInfraResources {
    pub fn new(
        tera_context: TeraContext,
        terraform_common_folder: PathBuf,
        destination_folder: PathBuf,
        event_details: EventDetails,
        envs: Vec<(String, String)>,
        is_dry_run: bool,
    ) -> TerraformInfraResources {
        TerraformInfraResources {
            tera_context,
            terraform_common_folder,
            destination_folder,
            event_details,
            envs,
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

        Ok(())
    }

    fn terraform_init(&self, envs: &[(&str, &str)]) -> Result<(), Box<EngineError>> {
        terraform_init_validate(
            self.destination_folder.to_string_lossy().as_ref(),
            envs,
            &TerraformValidators::Default,
        )
        .map_err(|e| Box::new(EngineError::new_terraform_error(self.event_details.clone(), e)))?;
        Ok(())
    }

    pub fn create<T: DeserializeOwned>(&self, logger: &impl InfraLogger) -> Result<T, Box<EngineError>> {
        let envs = envs_to_slice(self.envs.as_slice());
        self.prepare_terraform_files()?;
        self.terraform_init(&envs)?;

        logger.info("🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️");
        logger.info("🏗️ Creating terraform resources with the following plan");
        terraform_plan(self.destination_folder.to_string_lossy().as_ref(), &envs, false)
            .map_err(|e| Box::new(EngineError::new_terraform_error(self.event_details.clone(), e)))?
            .raw_std_output
            .into_iter()
            .for_each(|line| logger.info(line));

        // Apply will be skipped/do nothing if dry run is enabled
        terraform_apply(
            self.destination_folder.to_string_lossy().as_ref(),
            self.is_dry_run,
            &envs,
            &TerraformValidators::Default,
        )
        .map_err(|e| Box::new(EngineError::new_terraform_error(self.event_details.clone(), e)))?;
        logger.info("🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️");

        terraform_output::<T>(self.destination_folder.to_string_lossy().as_ref(), &envs)
            .map_err(|e| Box::new(EngineError::new_terraform_error(self.event_details.clone(), e)))
    }

    pub fn delete(&self, logger: &impl InfraLogger) -> Result<(), Box<EngineError>> {
        let envs = envs_to_slice(self.envs.as_slice());
        self.prepare_terraform_files()?;
        self.terraform_init(&envs)?;

        logger.info("🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️");
        logger.info("🏗️ Deleting terraform resources with the following plan");
        terraform_plan(self.destination_folder.to_string_lossy().as_ref(), &envs, true)
            .map_err(|e| Box::new(EngineError::new_terraform_error(self.event_details.clone(), e)))?
            .raw_std_output
            .into_iter()
            .for_each(|line| logger.info(line));

        if self.is_dry_run {
            return Ok(());
        }

        terraform_destroy(
            self.destination_folder.to_string_lossy().as_ref(),
            &envs,
            &TerraformValidators::None,
        )
        .map_err(|e| Box::new(EngineError::new_terraform_error(self.event_details.clone(), e)))?;
        logger.info("🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️ 🏗️");

        Ok(())
    }

    pub fn pause(&self, resources_filters: &[&str]) -> Result<(), Box<EngineError>> {
        let envs = envs_to_slice(self.envs.as_slice());
        self.prepare_terraform_files()?;
        self.terraform_init(&envs)?;

        // pause: only select terraform workers elements to pause to avoid applying on the whole config
        // this to avoid failures because of helm deployments on removing workers nodes
        let tf_workers_resources = terraform_state_list(
            self.destination_folder.to_string_lossy().as_ref(),
            &envs,
            &TerraformValidators::Default,
        )
        .map_err(|e| Box::new(EngineError::new_terraform_error(self.event_details.clone(), e)))?
        .raw_std_output
        .into_iter()
        .filter(|resources| resources_filters.iter().any(|filter| resources.starts_with(filter)))
        .collect_vec();

        // TODO: Extract the plan out of this function. so we can log it
        terraform_apply_with_tf_workers_resources(
            self.destination_folder.to_string_lossy().as_ref(),
            tf_workers_resources,
            &envs,
            &TerraformValidators::Default,
            self.is_dry_run,
        )
        .map_err(|e| Box::new(EngineError::new_terraform_error(self.event_details.clone(), e)))?;

        Ok(())
    }
}
