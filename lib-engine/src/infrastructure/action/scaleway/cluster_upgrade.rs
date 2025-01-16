use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::delete_kube_apps::prepare_kube_upgrade;
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::kubectl_utils::check_workers_on_upgrade;
use crate::infrastructure::action::scaleway::ScalewayQoveryTerraformOutput;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::Kapsule;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesUpgradeStatus};

pub fn upgrade_kapsule_cluster(
    cluster: &Kapsule,
    infra_ctx: &InfrastructureContext,
    kubernetes_upgrade_status: KubernetesUpgradeStatus,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
    logger.info("Preparing cluster upgrade process.");

    let temp_dir = cluster.temp_dir();
    // generate terraform files and copy them into temp dir

    //
    // Upgrade nodes
    //
    logger.info("Preparing nodes for upgrade for Kubernetes cluster.");
    logger.info("Checking clusters content health.");

    // disable all replicas with issues to avoid upgrade failures
    prepare_kube_upgrade(cluster as &dyn Kubernetes, infra_ctx, event_details.clone(), &logger)?;

    logger.info("Upgrading Kubernetes nodes.");
    let mut tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    tera_context.insert(
        "kubernetes_cluster_version",
        &kubernetes_upgrade_status.requested_version.to_string(),
    );
    let tf_resources = TerraformInfraResources::new(
        tera_context,
        cluster.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        vec![],
        cluster.context().is_dry_run_deploy(),
    );
    let _: ScalewayQoveryTerraformOutput = tf_resources.create(&logger)?;

    check_workers_on_upgrade(
        cluster,
        infra_ctx.cloud_provider(),
        kubernetes_upgrade_status.requested_version.to_string(),
        None,
    )
    .map_err(|e| {
        Box::new(EngineError::new_k8s_node_not_ready_with_requested_version(
            event_details.clone(),
            kubernetes_upgrade_status.requested_version.to_string(),
            e,
        ))
    })?;

    logger.info("Kubernetes nodes have been successfully upgraded.");

    Ok(())
}
