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
use crate::services::kubernetes_api_deprecation_service::KubernetesApiDeprecationServiceGranuality;

pub fn upgrade_kapsule_cluster(
    cluster: &Kapsule,
    infra_ctx: &InfrastructureContext,
    kubernetes_upgrade_status: KubernetesUpgradeStatus,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
    let cloud_provider = infra_ctx.cloud_provider();
    let kube_client = infra_ctx.mk_kube_client()?;
    logger.info("Start preparing Kapsule cluster upgrade process");

    logger.info("Check if cluster has no calls to deprecated kubernetes API in next version");
    match infra_ctx
        .kubernetes_api_deprecation_service()
        .is_cluster_fully_compatible_with_kubernetes_version(
            cluster.kubeconfig_local_file_path().as_path(),
            Some(&kubernetes_upgrade_status.requested_version),
            &cloud_provider.credentials_environment_variables(),
            KubernetesApiDeprecationServiceGranuality::WithQoveryMetadata {
                kube_client: kube_client.client(),
            },
        ) {
        Ok(_) => logger.info("Cluster is compatible with the next version"),
        Err(e) => {
            return Err(Box::new(EngineError::new_k8s_deprecated_api_calls_found(
                event_details.clone(),
                &kubernetes_upgrade_status.requested_version,
                e,
            )))
        }
    }

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
