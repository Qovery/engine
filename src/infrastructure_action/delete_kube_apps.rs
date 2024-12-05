use crate::cloud_provider::gcp::kubernetes::GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES;
use crate::cloud_provider::helm::ChartInfo;
use crate::cloud_provider::kubectl_utils::{delete_completed_jobs, delete_crashlooping_pods};
use crate::cloud_provider::kubernetes::{uninstall_cert_manager, Kubernetes};
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::cmd::kubectl::{kubectl_exec_delete_namespace, kubectl_exec_get_all_namespaces};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::engine::InfrastructureContext;
use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, EventMessage, InfrastructureStep};
use crate::infrastructure_action::InfraLogger;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use std::collections::HashSet;

pub(super) fn delete_kube_apps(
    cluster: &dyn Kubernetes,
    infra_ctx: &InfrastructureContext,
    event_details: EventDetails,
    logger: &impl InfraLogger,
    skip_helm_releases: HashSet<String>,
) -> Result<(), Box<EngineError>> {
    let kubeconfig_path = cluster.kubeconfig_local_file_path();
    // should make the diff between all namespaces and qovery managed namespaces
    let message = format!(
        "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
        cluster.name(),
        cluster.short_id()
    );
    logger.info(message);

    let all_namespaces = kubectl_exec_get_all_namespaces(
        &kubeconfig_path,
        infra_ctx.cloud_provider().credentials_environment_variables(),
    );

    match all_namespaces {
        Ok(namespace_vec) => {
            let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
            let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

            logger.info("Deleting non Qovery namespaces");
            for namespace_to_delete in namespaces_to_delete.iter() {
                match kubectl_exec_delete_namespace(
                    &kubeconfig_path,
                    namespace_to_delete,
                    infra_ctx.cloud_provider().credentials_environment_variables(),
                ) {
                    Ok(_) => logger.info(format!("Namespace `{}` deleted successfully.", namespace_to_delete)),
                    Err(e) if !e.message(ErrorMessageVerbosity::FullDetails).contains("not found") => {
                        logger.warn(format!("Can't delete the namespace `{}`", namespace_to_delete));
                    }
                    _ => {}
                }
            }
        }
        Err(e) => {
            let message_safe = format!(
                "Error while getting all namespaces for Kubernetes cluster {}",
                cluster.name_with_id(),
            );
            logger.warn(EventMessage::new(
                message_safe,
                Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
            ));
        }
    }

    let message = format!(
        "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
        cluster.name(),
        cluster.short_id()
    );
    logger.info(message);

    // delete custom metrics api to avoid stale namespaces on deletion
    let helm = Helm::new(
        Some(&kubeconfig_path),
        &infra_ctx.cloud_provider().credentials_environment_variables(),
    )
    .map_err(|e| to_engine_error(&event_details, e))?;
    let chart = ChartInfo::new_from_release_name("metrics-server", "kube-system");

    if let Err(e) = helm.uninstall(&chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
        // this error is not blocking
        logger.warn(EventMessage::new_from_engine_error(to_engine_error(&event_details, e)));
    }

    // required to avoid namespace stuck on deletion
    if let Err(e) = uninstall_cert_manager(
        &kubeconfig_path,
        infra_ctx.cloud_provider().credentials_environment_variables(),
        event_details.clone(),
        cluster.logger(),
    ) {
        // this error is not blocking, logging a warning and move on
        logger.warn(EventMessage::new(
            "An error occurred while trying to uninstall cert-manager. This is not blocking.".to_string(),
            Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
        ));
    }

    logger.info("Deleting Qovery managed elements");
    let qovery_namespaces = get_qovery_managed_namespaces();
    for qovery_namespace in qovery_namespaces.iter() {
        let charts_to_delete = helm
            .list_release(Some(qovery_namespace), &[])
            .map_err(|e| to_engine_error(&event_details, e))?;

        for chart in charts_to_delete {
            let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
            match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                Ok(_) => logger.info(format!("Chart `{}` deleted", chart.name)),
                Err(e) => {
                    let message_safe = format!("Can't delete chart `{}`", chart.name);
                    logger.warn(EventMessage::new(message_safe, Some(e.to_string())));
                }
            }
        }
    }

    logger.info("Deleting Qovery managed namespaces");
    for qovery_namespace in qovery_namespaces.iter() {
        let deletion = kubectl_exec_delete_namespace(
            &kubeconfig_path,
            qovery_namespace,
            infra_ctx.cloud_provider().credentials_environment_variables(),
        );
        match deletion {
            Ok(_) => logger.info(format!("Namespace `{}` is fully deleted.", qovery_namespace)),
            Err(e) if !e.message(ErrorMessageVerbosity::FullDetails).contains("not found") => {
                logger.warn(format!("Can't delete the namespace `{}`", qovery_namespace));
            }
            _ => {}
        }
    }

    logger.info("Deleting all remaining deployed helm applications");
    match helm.list_release(None, &[]) {
        Ok(helm_charts) => {
            for chart in helm_charts
                .into_iter()
                .filter(|helm_chart| !skip_helm_releases.contains(&helm_chart.name))
            {
                let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                    Ok(_) => logger.info(format!("Chart `{}` deleted", chart.name)),
                    Err(e) => {
                        let message_safe = format!("Error deleting chart `{}`", chart.name);
                        logger.warn(EventMessage::new(message_safe, Some(e.to_string())));
                    }
                }
            }
        }
        Err(e) => {
            logger.warn(EventMessage::new("Unable to get helm list".to_string(), Some(e.to_string())));
        }
    }

    Ok(())
}

pub(super) fn prepare_kube_upgrade(
    cluster: &dyn Kubernetes,
    infra_ctx: &InfrastructureContext,
    event_details: EventDetails,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
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

    delete_crashlooping_pods(
        cluster,
        None,
        None,
        Some(3),
        infra_ctx.cloud_provider().credentials_environment_variables(),
        Infrastructure(InfrastructureStep::Upgrade),
    )?;

    delete_completed_jobs(
        cluster,
        infra_ctx.cloud_provider().credentials_environment_variables(),
        Infrastructure(InfrastructureStep::Upgrade),
        Some(GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES.to_vec()),
    )?;

    Ok(())
}
