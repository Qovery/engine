use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::scaleway::kubernetes::Kapsule;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure_action::delete_kube_apps::delete_kube_apps;
use crate::infrastructure_action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure_action::scaleway::ScalewayQoveryTerraformOutput;
use crate::infrastructure_action::{InfraLogger, ToInfraTeraContext};
use crate::secret_manager;
use crate::secret_manager::vault::QVaultClient;
use crate::utilities::envs_to_string;
use std::collections::HashSet;

pub fn delete_kapsule_cluster(
    cluster: &Kapsule,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Delete));

    logger.info("Preparing to delete cluster.");

    let temp_dir = cluster.temp_dir();

    // generate terraform files and copy them into temp dir
    // We re-update the cluster to be sure it is in a correct state before deleting it
    let tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    let tf_resources = TerraformInfraResources::new(
        tera_context.clone(),
        cluster.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        cluster.context().is_dry_run_deploy(),
    );

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    logger.info(format!(
        "Ensuring everything is up to date before deleting cluster {}/{}",
        cluster.name(),
        cluster.short_id()
    ));
    logger.info("Running Terraform apply before running a delete.");

    let _qovery_terraform_output: ScalewayQoveryTerraformOutput = tf_resources.create(&logger)?;

    delete_kube_apps(cluster, infra_ctx, event_details.clone(), &logger, HashSet::with_capacity(0))?;

    logger.info(format!("Deleting Kubernetes cluster {}/{}", cluster.name(), cluster.short_id()));
    logger.info("Running Terraform destroy");
    tf_resources.delete(&logger)?;

    // delete info on vault
    let vault_conn = QVaultClient::new(event_details.clone());
    if let Ok(vault_conn) = vault_conn {
        let mount = secret_manager::vault::get_vault_mount_name(cluster.context().is_test_cluster());

        // ignore on failure
        let _ = vault_conn.delete_secret(mount.as_str(), cluster.long_id().to_string().as_str());
    };

    logger.info("Kubernetes cluster successfully deleted");
    Ok(())
}
