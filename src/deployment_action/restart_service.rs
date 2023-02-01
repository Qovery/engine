use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::runtime::block_on;
use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::ListParams;
use kube::Api;
use std::thread::sleep;
use std::time::Duration;

pub struct RestartServiceAction {
    selector: String,
    is_statefulset: bool,
    event_details: EventDetails,
}

impl RestartServiceAction {
    pub fn new(selector: String, is_statefulset: bool, event_details: EventDetails) -> RestartServiceAction {
        RestartServiceAction {
            selector,
            is_statefulset,
            event_details,
        }
    }
}

impl DeploymentAction for RestartServiceAction {
    fn on_create(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let future = restart_service(
            self.is_statefulset,
            &target.kube,
            target.environment.namespace(),
            &self.selector,
        );

        // Set timeout at 10min
        let timeout = Duration::from_secs(10 * 60);
        // Async block is necessary because tokio::time::timeout require a living tokio runtime, which does not exist
        // outside of the block_on. So must wrap it in an async task that will be exec inside the block_on
        let ret = block_on(async { tokio::time::timeout(timeout, future).await });

        match ret {
            Ok(Ok(())) => {}

            Ok(Err(kube_error)) => {
                let command_error =
                    CommandError::new("Cannot restart service".to_string(), Some(format!("{kube_error}")), None);
                return Err(Box::new(EngineError::new_cannot_restart_service(
                    self.event_details.clone(),
                    target.environment.namespace(),
                    &self.selector,
                    command_error,
                )));
            }

            // timeout
            Err(_) => {
                let command_error = CommandError::new_from_safe_message(format!(
                    "Timeout of {}s exceeded while restarting service",
                    timeout.as_secs()
                ));
                return Err(Box::new(EngineError::new_cannot_restart_service(
                    self.event_details.clone(),
                    target.environment.namespace(),
                    &self.selector,
                    command_error,
                )));
            }
        }

        Ok(())
    }
}

async fn restart_service(
    is_statefulset: bool,
    kube: &kube::Client,
    namespace: &str,
    selector: &str,
) -> Result<(), kube::Error> {
    // find current service pods running before restart
    let pods_api: Api<Pod> = Api::namespaced(kube.clone(), namespace);
    let pods_list_params = ListParams::default().labels(selector);
    let service_pods_before_restart = pods_api.list(&pods_list_params).await?.items;

    // restart either deployment or statefulset
    if is_statefulset {
        restart_statefulset(kube, namespace, &pods_list_params).await?
    } else {
        restart_deployment(kube, namespace, &pods_list_params).await?
    };

    // prepare predicate to wait after resource restart
    let pods_length_before_restart = service_pods_before_restart.len();
    let most_recent_pod_start_time_before_restart = service_pods_before_restart
        .into_iter()
        .map(|pod| {
            pod.status
                .expect("pod.status should always be filled")
                .start_time
                .expect("pod.status.time should always be filled")
        })
        .reduce(|acc, current| if acc.gt(&current) { acc } else { current })
        .expect("Should always retrieve most recent pod.status.time.date_time");

    // wait
    wait_until_service_pods_are_restarted(
        &pods_api,
        &pods_list_params,
        pods_length_before_restart,
        &most_recent_pod_start_time_before_restart,
    )
    .await?;

    Ok(())
}

async fn restart_deployment(
    kube: &kube::Client,
    namespace: &str,
    pods_list_params: &ListParams,
) -> Result<(), kube::Error> {
    let deployments_api: Api<Deployment> = Api::namespaced(kube.clone(), namespace);
    let deployments = deployments_api.list(pods_list_params).await?;

    if deployments.items.len() != 1 {
        let unexpected_list_of_deployments_error =
            kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(format!(
                "Unexpected list of deployments: found {} instead of 1",
                deployments.items.len()
            )));
        return Err(unexpected_list_of_deployments_error);
    }

    let deployment = deployments.items.first().unwrap();
    let deployment_name = deployment.metadata.clone().name.unwrap_or_default();
    deployments_api.restart(&deployment_name).await?;

    Ok(())
}

async fn restart_statefulset(
    kube: &kube::Client,
    namespace: &str,
    pods_list_params: &ListParams,
) -> Result<(), kube::Error> {
    let statefulset_api: Api<StatefulSet> = Api::namespaced(kube.clone(), namespace);
    let statefulsets = statefulset_api.list(pods_list_params).await?;

    if statefulsets.items.len() != 1 {
        let manual_kube_error = kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(format!(
            "Unexpected list of statefulsets: found {} instead of 1",
            statefulsets.items.len()
        )));
        return Err(manual_kube_error);
    }

    let statefulset = statefulsets.items.first().unwrap();
    let statefulset_name = statefulset.metadata.clone().name.unwrap_or_default();
    statefulset_api.restart(&statefulset_name).await?;

    Ok(())
}

async fn wait_until_service_pods_are_restarted(
    pods_api: &Api<Pod>,
    pods_list_params: &ListParams,
    pods_length_before_restart: usize,
    most_recent_pod_start_time_before_restart: &Time,
) -> Result<(), kube::Error> {
    loop {
        let running_service_pods = pods_api.list(pods_list_params).await?;

        let number_of_pods_running = running_service_pods
            .into_iter()
            .filter_map(|pod| pod.status)
            .filter(|status| match status.clone().start_time {
                None => false,
                Some(pod_start_time) => pod_start_time.gt(most_recent_pod_start_time_before_restart),
            })
            .filter_map(|pod_status| pod_status.container_statuses)
            .filter(|container_statuses| {
                container_statuses.iter().all(|container_status| {
                    let is_running = if let Some(state) = &container_status.state {
                        state.running.is_some()
                    } else {
                        false
                    };
                    is_running && container_status.ready
                })
            })
            .count();

        if number_of_pods_running == pods_length_before_restart {
            break;
        }

        sleep(Duration::from_secs(10));
    }

    Ok(())
}
