use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::cluster_outputs_helper::update_cluster_outputs;
use crate::infrastructure::action::delete_kube_apps::{delete_all_pdbs, delete_kube_apps};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::scaleway::ScalewayQoveryTerraformOutput;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::Kapsule;
use crate::utilities::envs_to_string;
use std::collections::HashSet;

pub fn delete_kapsule_cluster(
    cluster: &Kapsule,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Delete));

    logger.info("Preparing to delete cluster.");

    // generate terraform files and copy them into temp dir
    // We re-update the cluster to be sure it is in a correct state before deleting it
    let tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    let tf_resources = TerraformInfraResources::new(
        tera_context.clone(),
        cluster.template_directory.join("terraform"),
        cluster.temp_dir().join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        cluster.context().is_dry_run_deploy(),
    );

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    // Best-effort reconcile: apply may fail and the state may no longer hold deserializable outputs
    // once the cluster is gone. None => skip the cleanup steps that need a reachable cluster.
    // skipReconcile: skip the (hang/credential-prone) Terraform apply, but still init + read existing
    // outputs so in-cluster cleanup can run when the cluster is reachable; only skip cleanup when outputs
    // can't be read (cluster gone).
    let qovery_terraform_output: Option<ScalewayQoveryTerraformOutput> = if infra_ctx.context().is_skip_reconcile() {
        logger.info("Skip reconcile requested: skipping the pre-destroy Terraform apply; reading existing outputs so in-cluster cleanup can still run if the cluster is reachable.");
        tf_resources.init_and_read_output().ok()
    } else {
        logger.info(format!(
            "Ensuring everything is up to date before deleting cluster {}/{}",
            cluster.name(),
            cluster.short_id()
        ));
        logger.info("Running Terraform apply before running a delete.");
        tf_resources.create_or_read_output(&logger)
    };

    // kubeconfig + in-cluster cleanup only make sense when we have outputs (cluster still reachable).
    if let Some(output) = &qovery_terraform_output {
        // Best-effort under skipReconcile: attempt the in-cluster cleanup, but never let its failure
        // block the teardown below — force-delete must always proceed to destroy.
        let cleanup = (|| -> Result<(), Box<EngineError>> {
            update_cluster_outputs(cluster, output)?;

            // delete all PDBs first, because those will prevent node deletion
            if let Err(_errors) = delete_all_pdbs(infra_ctx, event_details.clone(), &logger) {
                logger.warn("Cannot delete all PDBs, this is not blocking cluster deletion.");
            }

            delete_kube_apps(cluster, infra_ctx, event_details.clone(), &logger, HashSet::with_capacity(0))?;
            Ok(())
        })();
        if let Err(e) = cleanup {
            if infra_ctx.context().is_skip_reconcile() {
                logger.warn(format!(
                    "Skip reconcile: in-cluster cleanup failed; continuing to cluster teardown: {e}"
                ));
            } else {
                return Err(e);
            }
        }
    } else {
        logger.warn("Skipping in-cluster cleanup (PDBs, apps): no Terraform outputs, cluster likely already deleted.");
    }

    logger.info(format!("Deleting Kubernetes cluster {}/{}", cluster.name(), cluster.short_id()));
    logger.info("Running Terraform destroy");
    tf_resources.delete(&[], &logger)?;

    logger.info("Kubernetes cluster successfully deleted");
    Ok(())
}
