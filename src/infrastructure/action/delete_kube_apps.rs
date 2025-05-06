use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{Helm, to_engine_error};
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, EventMessage, InfrastructureStep};
use crate::helm::ChartInfo;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::kubectl_utils::{delete_completed_jobs, delete_crashlooping_pods};
use crate::infrastructure::helm_charts::metrics_server_chart::MetricsServerChart;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::gcp::GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES;
use crate::infrastructure::models::kubernetes::{Kubernetes, uninstall_cert_manager};
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::api::policy::v1::PodDisruptionBudget;
use kube::Api;
use kube::api::{DeleteParams, ListParams};
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

pub(super) fn delete_all_pdbs(
    infra_ctx: &InfrastructureContext,
    event_details: EventDetails,
    logger: &impl InfraLogger,
) -> Result<(), Vec<EngineError>> {
    logger.info("Deleting PDBs");

    let kube_client = infra_ctx.mk_kube_client().map_err(|e| vec![*e])?;
    let pdbs: Api<PodDisruptionBudget> = Api::all(kube_client.client());

    let list_params = ListParams::default();
    let pdb_list = block_on(pdbs.list(&list_params))
        .map_err(|e| {
            EngineError::new_k8s_cannot_get_pdbs(
                event_details.clone(),
                CommandError::new("Error listing PDBs".to_string(), Some(e.to_string()), None),
            )
        })
        .map_err(|e| vec![e])?;

    let mut errors = Vec::new();
    for pdb in pdb_list {
        if let Some(name) = pdb.metadata.name {
            let namespace = pdb.metadata.namespace.clone().unwrap_or_else(|| "default".to_string());
            let pdb_ns_api: Api<PodDisruptionBudget> = Api::namespaced(pdbs.clone().into_client(), &namespace);
            logger.info(format!("Deleting PDB: {}/{}", namespace, name));
            // if an error occurs while deleting PDB, just continue, it means PDB is managed by cloud provider
            if let Err(e) = block_on(pdb_ns_api.delete(&name, &DeleteParams::default())) {
                let safe_error_message = format!("Error deleting PDB {}/{}", namespace, name,);
                logger.warn(safe_error_message.to_string());
                errors.push(EngineError::new_k8s_cannot_delete_pdb(
                    namespace.as_str(),
                    name.as_str(),
                    event_details.clone(),
                    CommandError::new(safe_error_message.to_string(), Some(e.to_string()), None),
                ));
            }
        }
    }

    match errors.is_empty() {
        true => Ok(()),
        false => Err(errors),
    }
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
    let ns_api: Api<Namespace> = Api::all(kube.client());
    let all_namespaces = block_on(ns_api.list_metadata(&Default::default())).map(|ns| {
        ns.items
            .into_iter()
            .map(|ns| ns.metadata.name.unwrap_or_default())
            .collect::<Vec<String>>()
    });

    // Delete cert-manager objects first
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
        logger.warn(EventMessage::from(to_engine_error(&event_details, e)));
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

// this fn should implements the algorithm describe here: https://qovery.atlassian.net/secure/RapidBoard.jspa?rapidView=10&modal=detail&selectedIssue=DEV-283
pub fn get_firsts_namespaces_to_delete(namespaces: Vec<&str>) -> Vec<&str> {
    // from all namespaces remove managed and never delete namespaces
    namespaces
        .into_iter()
        .filter(|item| !get_qovery_managed_namespaces().contains(item))
        .filter(|item| !get_never_delete_namespaces().contains(item))
        .collect()
}

pub fn get_qovery_managed_namespaces() -> &'static [&'static str] {
    // order is very important because of dependencies
    &["logging", "nginx-ingress", "qovery", "cert-manager", "prometheus"]
}

fn get_never_delete_namespaces() -> &'static [&'static str] {
    &["default", "kube-node-lease", "kube-public", "kube-system"]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_never_delete_namespaces() {
        // setup:
        let expected = vec!["default", "kube-node-lease", "kube-public", "kube-system"];

        // execute:
        let result = get_never_delete_namespaces();

        // verify:
        assert_eq!(expected, result);
    }

    #[test]
    fn test_get_qovery_managed_namespaces() {
        // setup:
        let expected = vec!["logging", "nginx-ingress", "qovery", "cert-manager", "prometheus"];

        // execute:
        let result = get_qovery_managed_namespaces();

        // verify:
        assert_eq!(expected, result);
    }

    #[test]
    fn test_get_firsts_namespaces_to_delete() {
        // setup:
        struct TestCase<'a> {
            input: Vec<&'a str>,
            expected_output: Vec<&'a str>,
            description: &'a str,
        }

        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input: Vec::new(),
                expected_output: Vec::new(),
                description: "empty vec",
            },
            TestCase {
                input: vec!["a", "b", "c", "d"],
                expected_output: vec!["a", "b", "c", "d"],
                description: "everything can be deleted",
            },
            TestCase {
                input: vec![
                    "a",
                    "b",
                    "c",
                    "d",
                    "default",
                    "kube-node-lease",
                    "kube-public",
                    "kube-system",
                ],
                expected_output: vec!["a", "b", "c", "d"],
                description: "multiple elements among never to be deleted list",
            },
            TestCase {
                input: vec!["a", "b", "c", "d", "kube-system"],
                expected_output: vec!["a", "b", "c", "d"],
                description: "one element among never to be deleted list",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = get_firsts_namespaces_to_delete(tc.input.clone());

            // verify:
            assert_eq!(
                tc.expected_output,
                result,
                "case: {}, all: {:?} never_delete: {:?}",
                tc.description,
                tc.input,
                get_never_delete_namespaces()
            );
        }
    }
}
