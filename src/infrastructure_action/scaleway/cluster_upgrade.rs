use crate::cloud_provider::kubectl_utils::{check_workers_on_upgrade, delete_completed_jobs, delete_crashlooping_pods};
use crate::cloud_provider::kubernetes::{Kubernetes, KubernetesUpgradeStatus};
use crate::cloud_provider::scaleway::kubernetes::Kapsule;
use crate::cmd::terraform::terraform_init_validate_plan_apply;
use crate::cmd::terraform_validators::TerraformValidators;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventMessage, InfrastructureStep};
use crate::infrastructure_action::scaleway::tera_context::kapsule_tera_context;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;

pub fn upgrade_kapsule_cluster(
    cluster: &Kapsule,
    infra_ctx: &InfrastructureContext,
    kubernetes_upgrade_status: KubernetesUpgradeStatus,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Start preparing cluster upgrade process".to_string()),
    ));

    let temp_dir = cluster.temp_dir();
    // generate terraform files and copy them into temp dir
    let mut context = kapsule_tera_context(cluster, infra_ctx)?;

    //
    // Upgrade nodes
    //
    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing nodes for upgrade for Kubernetes cluster.".to_string()),
    ));

    context.insert(
        "kubernetes_cluster_version",
        format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
    );

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(cluster.template_directory.as_str(), temp_dir, context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            cluster.template_directory.to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
    let common_bootstrap_charts = format!("{}/common/bootstrap/charts", cluster.context().lib_root_dir());
    if let Err(e) =
        crate::template::copy_non_template_files(common_bootstrap_charts.as_str(), common_charts_temp_dir.as_str())
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            common_bootstrap_charts,
            common_charts_temp_dir,
            e,
        )));
    }

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Upgrading Kubernetes nodes.".to_string()),
    ));

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Checking clusters content health".to_string()),
    ));

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
            cluster.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!(
                    "Deployment {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                    deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
                )),
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
            cluster.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!(
                    "Statefulset {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                    sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
                )),
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
        cluster.logger().log(EngineEvent::Error(*e.clone(), None));
        return Err(e);
    }

    if let Err(e) = delete_completed_jobs(
        cluster,
        infra_ctx.cloud_provider().credentials_environment_variables(),
        Infrastructure(InfrastructureStep::Upgrade),
        None,
    ) {
        cluster.logger().log(EngineEvent::Error(*e.clone(), None));
        return Err(e);
    }

    match terraform_init_validate_plan_apply(
        temp_dir.to_string_lossy().as_ref(),
        cluster.context().is_dry_run_deploy(),
        &[],
        &TerraformValidators::Default,
    ) {
        Ok(_) => match check_workers_on_upgrade(
            cluster,
            infra_ctx.cloud_provider(),
            kubernetes_upgrade_status.requested_version.to_string(),
            None,
        ) {
            Ok(_) => {
                cluster.logger().log(EngineEvent::Info(
                    event_details,
                    EventMessage::new_from_safe("Kubernetes nodes have been successfully upgraded.".to_string()),
                ));
            }
            Err(e) => {
                return Err(Box::new(EngineError::new_k8s_node_not_ready_with_requested_version(
                    event_details,
                    kubernetes_upgrade_status.requested_version.to_string(),
                    e,
                )));
            }
        },
        Err(e) => {
            return Err(Box::new(EngineError::new_terraform_error(event_details, e)));
        }
    }

    Ok(())
}
