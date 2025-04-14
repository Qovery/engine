use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::kubectl_utils::check_control_plane_on_upgrade;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesUpgradeStatus, KubernetesVersion};

use crate::infrastructure::infrastructure_context::InfrastructureContext;

use crate::infrastructure::action::azure::AksQoveryTerraformOutput;
use crate::infrastructure::action::delete_kube_apps::prepare_kube_upgrade;
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::models::kubernetes::azure::aks::AKS;
use crate::utilities::envs_to_string;
use std::str::FromStr;

pub(super) fn upgrade_aks_cluster(
    cluster: &AKS,
    infra_ctx: &InfrastructureContext,
    kubernetes_upgrade_status: KubernetesUpgradeStatus,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
    logger.info("Start preparing AKS cluster upgrade process");

    let temp_dir = cluster.temp_dir();
    logger.info("Upgrading AKS cluster.");

    //
    // Upgrade nodes
    //
    logger.info("Preparing nodes for upgrade for Kubernetes cluster.");
    logger.info("Upgrading Kubernetes nodes.");
    logger.info("Checking clusters content health.");

    prepare_kube_upgrade(cluster as &dyn Kubernetes, infra_ctx, event_details.clone(), &logger)?;

    let requested_version = kubernetes_upgrade_status.requested_version.to_string();
    let kubernetes_version = match KubernetesVersion::from_str(requested_version.as_str()) {
        Ok(kubeversion) => kubeversion,
        Err(_) => {
            return Err(Box::new(EngineError::new_cannot_determine_k8s_master_version(
                event_details,
                kubernetes_upgrade_status.requested_version.to_string(),
            )));
        }
    };

    let mut tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    tera_context.insert(
        "kubernetes_cluster_version",
        format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
    );
    let tf_resources = TerraformInfraResources::new(
        tera_context,
        cluster.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        cluster.context().is_dry_run_deploy(),
    );

    let _tf_output: AksQoveryTerraformOutput = tf_resources.create(&logger)?;

    check_control_plane_on_upgrade(cluster, infra_ctx.cloud_provider(), kubernetes_version).map_err(|e| {
        Box::new(EngineError::new_k8s_node_not_ready_with_requested_version(
            event_details,
            kubernetes_upgrade_status.requested_version.to_string(),
            e,
        ))
    })?;

    logger.info("Kubernetes control plane has been successfully upgraded.");

    Ok(())
}
