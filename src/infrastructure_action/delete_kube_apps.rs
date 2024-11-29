use crate::cloud_provider::gcp::kubernetes::GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES;
use crate::cloud_provider::helm::ChartInfo;
use crate::cloud_provider::helm_charts::metrics_server_chart::MetricsServerChart;
use crate::cloud_provider::kubectl_utils::{delete_completed_jobs, delete_crashlooping_pods};
use crate::cloud_provider::kubernetes::{uninstall_cert_manager, Kubernetes};
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, EventMessage, InfrastructureStep};
use crate::infrastructure_action::InfraLogger;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use k8s_openapi::api::core::v1::Namespace;
use kube::api::DeleteParams;
use kube::Api;
use std::collections::HashSet;
use std::time::Duration;

const DELETE_TIMEOUT: Duration = Duration::from_secs(60 * 10);

fn delete_namespace(
    ns_to_delete: &str,
    ns_api: Api<Namespace>,
    event_details: &EventDetails,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    match block_on(async {
        tokio::time::timeout(DELETE_TIMEOUT, ns_api.delete(ns_to_delete, &DeleteParams::foreground())).await
    }) {
        Ok(Ok(_)) => logger.info(format!("Deleted successfully namespace `{}`", ns_to_delete)),
        Ok(Err(err)) => logger.warn(format!("Can't delete the namespace `{}`: {:?}", ns_to_delete, err)),
        Err(_timeout) => {
            let msg = format!(
                "Can't delete the namespace `{}`: due to {:?}s timeout elapsed",
                ns_to_delete, DELETE_TIMEOUT
            );
            logger.warn(&msg);
            return Err(Box::new(EngineError::new_k8s_delete_service_error(
                event_details.clone(),
                CommandError::new_from_safe_message(msg.clone()),
                msg,
            )));
        }
    }

    Ok(())
}

pub(super) fn delete_kube_apps(
    cluster: &dyn Kubernetes,
    infra_ctx: &InfrastructureContext,
    event_details: EventDetails,
    logger: &impl InfraLogger,
    skip_helm_releases: HashSet<String>,
) -> Result<(), Box<EngineError>> {
    // should make the diff between all namespaces and qovery managed namespaces
    let message = format!(
        "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
        cluster.name(),
        cluster.short_id()
    );
    logger.info(message);

    let kube = infra_ctx.mk_kube_client()?;
    let ns_api: Api<Namespace> = Api::all(kube.client().clone());
    let all_namespaces = block_on(ns_api.list_metadata(&Default::default())).map(|ns| {
        ns.items
            .into_iter()
            .map(|ns| ns.metadata.name.unwrap_or_default())
            .collect::<Vec<String>>()
    });

    match all_namespaces {
        Ok(namespaces) => {
            let namespaces_as_str = namespaces.iter().map(std::ops::Deref::deref).collect();
            for ns_to_delete in get_firsts_namespaces_to_delete(namespaces_as_str) {
                delete_namespace(ns_to_delete, ns_api.clone(), &event_details, logger)?;
            }
        }

        Err(e) => {
            let message_safe = format!(
                "Error while getting all namespaces for Kubernetes cluster {}",
                cluster.name_with_id()
            );
            logger.warn(EventMessage::new(message_safe, Some(e.to_string())));
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
        Some(cluster.kubeconfig_local_file_path()),
        &infra_ctx.cloud_provider().credentials_environment_variables(),
    )
    .map_err(|e| to_engine_error(&event_details, e))?;
    let chart = ChartInfo::new_from_release_name(&MetricsServerChart::chart_name(), "kube-system");

    if let Err(e) = helm.uninstall(
        &chart,
        &[],
        &CommandKiller::from_timeout(DELETE_TIMEOUT),
        &mut |_| {},
        &mut |_| {},
    ) {
        // this error is not blocking
        logger.warn(EventMessage::new_from_engine_error(to_engine_error(&event_details, e)));
    }

    // required to avoid namespace stuck on deletion
    if let Err(e) = uninstall_cert_manager(
        cluster.kubeconfig_local_file_path(),
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
            match helm.uninstall(
                &chart_info,
                &[],
                &CommandKiller::from_timeout(DELETE_TIMEOUT),
                &mut |_| {},
                &mut |_| {},
            ) {
                Ok(_) => logger.info(format!("Chart `{}` deleted", chart.name)),
                Err(e) => {
                    let message_safe = format!("Can't delete chart `{}`", chart.name);
                    logger.warn(EventMessage::new(message_safe, Some(e.to_string())));
                }
            }
        }
    }

    logger.info("Deleting Qovery managed namespaces");
    for ns_to_delete in qovery_namespaces.iter() {
        delete_namespace(ns_to_delete, ns_api.clone(), &event_details, logger)?;
    }

    logger.info("Deleting all remaining deployed helm applications");
    match helm.list_release(None, &[]) {
        Ok(helm_charts) => {
            for chart in helm_charts
                .into_iter()
                .filter(|helm_chart| !skip_helm_releases.contains(&helm_chart.name))
            {
                let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                match helm.uninstall(
                    &chart_info,
                    &[],
                    &CommandKiller::from_timeout(DELETE_TIMEOUT),
                    &mut |_| {},
                    &mut |_| {},
                ) {
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
