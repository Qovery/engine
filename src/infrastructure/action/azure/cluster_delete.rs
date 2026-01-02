use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, EventMessage, InfrastructureStep};
use crate::infrastructure::action::azure::AksQoveryTerraformOutput;
use crate::infrastructure::action::cluster_outputs_helper::update_cluster_outputs;
use crate::infrastructure::action::delete_kube_apps::{delete_all_pdbs, delete_kube_apps};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::azure::aks::AKS;
use crate::infrastructure::models::object_storage::BucketDeleteStrategy;
use crate::infrastructure::models::object_storage::azure_object_storage::StorageAccount;
use crate::utilities::envs_to_string;
use std::collections::HashSet;

pub(super) fn delete_aks_cluster(
    cluster: &AKS,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Delete));

    logger.info("Preparing to delete cluster.");
    let temp_dir = cluster.temp_dir();

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    let message = format!(
        "Ensuring everything is up to date before deleting cluster {}/{}",
        cluster.name(),
        cluster.short_id()
    );
    logger.info(message);
    logger.info("Running Terraform apply before running a delete.");
    let tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    let tf_resources = TerraformInfraResources::new(
        tera_context.clone(),
        cluster.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        cluster.context().is_dry_run_deploy(),
    );
    let qovery_terraform_output: AksQoveryTerraformOutput = tf_resources.create(&logger)?;
    update_cluster_outputs(cluster, &qovery_terraform_output)?;

    // delete all PDBs first, because those will prevent node deletion
    if let Err(_errors) = delete_all_pdbs(infra_ctx, event_details.clone(), &logger) {
        logger.warn("Cannot delete all PDBs, this is not blocking cluster deletion.");
    }

    delete_kube_apps(cluster, infra_ctx, event_details.clone(), &logger, HashSet::with_capacity(0))?;

    // Delete cluster CR before terraform destroy because resource group should be deleted
    delete_container_registry(infra_ctx, event_details.clone())?;

    logger.info(format!("Deleting Kubernetes cluster {}/{}", cluster.name(), cluster.short_id()));
    tf_resources.delete(&[], &logger)?;

    delete_object_storage(
        cluster,
        &StorageAccount {
            access_key: qovery_terraform_output
                .main_storage_account_primary_access_key
                .to_string(),
            account_name: qovery_terraform_output.main_storage_account_name.to_string(),
        },
        &logger,
    )?;

    logger.info("Kubernetes cluster deleted successfully.");
    Ok(())
}

fn delete_object_storage(
    cluster: &AKS,
    storage_account: &StorageAccount,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    if let Err(e) = cluster.blob_storage.delete_bucket(
        storage_account,
        &cluster.logs_bucket_name(),
        BucketDeleteStrategy::HardDelete,
    )
    // Because cluster logs buckets can be sometimes very beefy, we delete them in a non-blocking way via a GCP job.
    //.delete_bucket_non_blocking(storage_account, &cluster.logs_bucket_name())
    {
        logger.warn(EventMessage::new(
            format!("Cannot delete cluster logs blob container `{}`", &cluster.logs_bucket_name()),
            Some(e.to_string()),
        ));
    }

    Ok(())
}

fn delete_container_registry(
    infra_ctx: &InfrastructureContext,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    let azure_cr = infra_ctx
        .container_registry()
        .as_azure_container_registry()
        .map_err(|e| EngineError::new_container_registry_error(event_details.clone(), e))?;

    azure_cr
        .delete_repository_in_resource_group(
            infra_ctx.kubernetes().cluster_name().as_str(), // Create the registry in the same resource group as the cluster
            infra_ctx.kubernetes().cluster_name().as_str(),
        )
        .map_err(|e| EngineError::new_container_registry_error(event_details.clone(), e))?;

    Ok(())
}
