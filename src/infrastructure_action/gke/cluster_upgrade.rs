use crate::cloud_provider::kubectl_utils::{
    check_control_plane_on_upgrade, delete_completed_jobs, delete_crashlooping_pods,
};
use crate::cloud_provider::kubernetes::{Kubernetes, KubernetesUpgradeStatus, KubernetesVersion};
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use std::path::PathBuf;

use crate::engine::InfrastructureContext;

use crate::cloud_provider::gcp::kubernetes::{Gke, GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES};
use crate::infrastructure_action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure_action::gke::GkeQoveryTerraformOutput;
use crate::infrastructure_action::{InfraLogger, ToInfraTeraContext};
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use std::str::FromStr;

pub(super) fn upgrade_gke_cluster(
    cluster: &Gke,
    infra_ctx: &InfrastructureContext,
    kubernetes_upgrade_status: KubernetesUpgradeStatus,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
    logger.info("Start preparing GKE cluster upgrade process");

    let temp_dir = cluster.temp_dir();
    logger.info("Upgrading GKE cluster.");

    //
    // Upgrade nodes
    //
    logger.info("Preparing nodes for upgrade for Kubernetes cluster.");
    logger.info("Upgrading Kubernetes nodes.");
    logger.info("Checking clusters content health.");

    let _ = cluster.configure_gcloud_for_cluster(infra_ctx); // TODO(benjaminch): properly handle this error
                                                             // disable all replicas with issues to avoid upgrade failures
    let kube_client = infra_ctx.mk_kube_client()?;
    let deployments = block_on(kube_client.get_deployments(event_details.clone(), None, SelectK8sResourceBy::All))?;
    for deploy in deployments {
        let status = match deploy.status {
            Some(s) => s,
            None => continue,
        };

        let replicas = status.replicas.unwrap_or(0);
        let ready_replicas = status.ready_replicas.unwrap_or(0);

        // if number of replicas > 0: it is not already disabled
        // ready_replicas == 0: there is something in progress (rolling restart...) so we should not touch it
        if replicas > 0 && ready_replicas == 0 {
            logger.info(format!(
                "Deployment {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
            ));
            block_on(kube_client.set_deployment_replicas_number(
                event_details.clone(),
                deploy.metadata.name.as_str(),
                deploy.metadata.namespace.as_str(),
                0,
            ))?;
        } else {
            info!(
                "Deployment {}/{} has {}/{} replicas ready. No action needed.",
                deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
            );
        }
    }
    // same with statefulsets
    let statefulsets = block_on(kube_client.get_statefulsets(event_details.clone(), None, SelectK8sResourceBy::All))?;
    for sts in statefulsets {
        let status = match sts.status {
            Some(s) => s,
            None => continue,
        };

        let ready_replicas = status.ready_replicas.unwrap_or(0);

        // if number of replicas > 0: it is not already disabled
        // ready_replicas == 0: there is something in progress (rolling restart...) so we should not touch it
        if status.replicas > 0 && ready_replicas == 0 {
            logger.info(format!(
                "Statefulset {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
            ));
            block_on(kube_client.set_statefulset_replicas_number(
                event_details.clone(),
                sts.metadata.name.as_str(),
                sts.metadata.namespace.as_str(),
                0,
            ))?;
        } else {
            info!(
                "Statefulset {}/{} has {}/{} replicas ready. No action needed.",
                sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
            );
        }
    }

    if let Err(e) = delete_crashlooping_pods(
        cluster,
        None,
        None,
        Some(3),
        infra_ctx.cloud_provider().credentials_environment_variables(),
        Infrastructure(InfrastructureStep::Upgrade),
    ) {
        logger.error(*e.clone(), None::<&str>);
        return Err(e);
    }

    if let Err(e) = delete_completed_jobs(
        cluster,
        infra_ctx.cloud_provider().credentials_environment_variables(),
        Infrastructure(InfrastructureStep::Upgrade),
        Some(GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES.to_vec()),
    ) {
        logger.error(*e.clone(), None::<&str>);
        return Err(e);
    }

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
        PathBuf::from(&cluster.template_directory).join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        cluster.context().is_dry_run_deploy(),
    );

    let _tf_output: GkeQoveryTerraformOutput = tf_resources.create(
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
        &logger,
    )?;

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
