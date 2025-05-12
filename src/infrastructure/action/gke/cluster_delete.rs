use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventMessage, InfrastructureStep};
use crate::infrastructure::action::delete_kube_apps::{delete_all_pdbs, delete_kube_apps};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::gke::GkeQoveryTerraformOutput;
use crate::infrastructure::action::kubeconfig_helper::update_kubeconfig_file;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::gcp::Gke;
use crate::infrastructure::models::object_storage::ObjectStorage;
use crate::utilities::envs_to_string;
use std::collections::HashSet;

pub(super) fn delete_gke_cluster(
    cluster: &Gke,
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
    let qovery_terraform_output: GkeQoveryTerraformOutput = tf_resources.create(&logger)?;
    update_kubeconfig_file(cluster, &qovery_terraform_output.kubeconfig)?;

    // Configure kubectl to be able to connect to cluster
    let _ = cluster.configure_gcloud_for_cluster(infra_ctx); // TODO(ENG-1802): properly handle this error

    // delete all PDBs first, because those will prevent node deletion
    if let Err(_errors) = delete_all_pdbs(infra_ctx, event_details.clone(), &logger) {
        logger.warn("Cannot delete all PDBs, this is not blocking cluster deletion.");
    }

    delete_kube_apps(cluster, infra_ctx, event_details.clone(), &logger, HashSet::with_capacity(0))?;

    logger.info(format!("Deleting Kubernetes cluster {}/{}", cluster.name(), cluster.short_id()));
    tf_resources.delete(&[], &logger)?;

    delete_object_storage(cluster, &logger)?;
    logger.info("Kubernetes cluster deleted successfully.");
    Ok(())
}

fn delete_object_storage(cluster: &Gke, logger: &impl InfraLogger) -> Result<(), Box<EngineError>> {
    // Because cluster logs buckets can be sometimes very beefy, we delete them in a non-blocking way via a GCP job.
    if let Err(e) = cluster
        .object_storage
        .delete_bucket_non_blocking(&cluster.logs_bucket_name())
    {
        logger.warn(EventMessage::new(
            format!("Cannot delete cluster logs object storage `{}`", &cluster.logs_bucket_name()),
            Some(e.to_string()),
        ));
    }

    if let Err(e) = cluster
        .object_storage
        .delete_bucket_non_blocking(&cluster.prometheus_bucket_name())
    {
        logger.warn(EventMessage::new(
            format!(
                "Cannot delete cluster logs object storage `{}`",
                &cluster.prometheus_bucket_name()
            ),
            Some(e.to_string()),
        ));
    }

    Ok(())
}
