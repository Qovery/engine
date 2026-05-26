pub mod deploy_helm;
pub mod deploy_terraform;

use crate::blueprint::models::error::BlueprintError;
use crate::cmd::terraform::{TerraformApplyOptions, terraform_apply_with_options, terraform_init_validate};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EventDetails, EventMessage};
use crate::logger::Logger;
use crate::template::generate_and_copy_all_files_into_dir;
use std::path::Path;
use std::time::Duration;

/// Common logic for blueprint actions: render Tera template → terraform init → terraform apply.
/// Used by both deploy_terraform and deploy_helm.
///
/// Renders into an isolated tempdir rather than the cloned catalog directory. Catalogs declare
/// their own providers (e.g. `terraform { required_providers { aws = ... } }`) which
/// collide with the engine's `terraform { required_providers { qovery } }` block if rendered alongside.
pub(crate) fn render_and_apply(
    template_dir: &Path,
    tera_context: &tera::Context,
    qovery_api_token: &str,
    timeout_sec: u64,
    is_dry_run: bool,
    service_type: &str,
    event_details: &EventDetails,
    logger: &dyn Logger,
) -> Result<(), Box<EngineError>> {
    let engine_workspace = tempfile::TempDir::new().map_err(|e| {
        Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::WorkspaceError(format!("Failed to create engine terraform workspace: {}", e)),
        ))
    })?;
    let working_dir = engine_workspace.path();

    // 1. Generate terraform files from template
    generate_and_copy_all_files_into_dir(template_dir, working_dir, tera_context).map_err(|e| {
        Box::new(EngineError::new_blueprint_error(
            event_details.clone(),
            BlueprintError::TerraformGenerationError(format!("Failed to generate terraform files: {}", e)),
        ))
    })?;

    logger.log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new(format!("Generated terraform files for {}", service_type), None),
    ));

    let envs: Vec<(&str, &str)> = vec![("QOVERY_API_TOKEN", qovery_api_token)];
    let dir = working_dir.to_string_lossy();

    // 2. Terraform init + validate
    logger.log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new("Running terraform init + validate".to_string(), None),
    ));
    terraform_init_validate(&dir, &envs, &TerraformValidators::Default)
        .map_err(|e| Box::new(EngineError::new_terraform_error(event_details.clone(), e)))?;

    // 3. Terraform apply (skip if dry-run)
    if is_dry_run {
        logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new("Dry run mode — skipping terraform apply".to_string(), None),
        ));
    } else {
        logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new(format!("Creating {} via Qovery Terraform provider", service_type), None),
        ));
        terraform_apply_with_options(
            &dir,
            is_dry_run,
            &envs,
            &TerraformValidators::Default,
            TerraformApplyOptions {
                max_retries: 0,
                command_timeout: Some(Duration::from_secs(timeout_sec)),
            },
        )
        .map_err(|e| Box::new(EngineError::new_terraform_error(event_details.clone(), e)))?;
    }

    Ok(())
}
