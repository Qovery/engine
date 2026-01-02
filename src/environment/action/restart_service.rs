use crate::environment::action::{DeploymentAction, K8sResourceType};
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::runtime::block_on;
use chrono::Utc;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::ListParams;
use kube::{Api, Client, Error};
use std::time::Duration;

pub struct RestartServiceAction {
    selector: String,
    k8s_resource_type: K8sResourceType,
    event_details: EventDetails,
    is_cluster_wide_resources_allowed: bool,
}

impl RestartServiceAction {
    pub fn new(selector: String, is_statefulset: bool, event_details: EventDetails) -> RestartServiceAction {
        RestartServiceAction {
            selector,
            k8s_resource_type: if is_statefulset {
                K8sResourceType::StateFulSet
            } else {
                K8sResourceType::Deployment
            },
            event_details,
            is_cluster_wide_resources_allowed: false,
        }
    }

    pub fn new_with_resource_type(
        selector: String,
        k8s_resource_type: K8sResourceType,
        event_details: EventDetails,
        is_cluster_wide_resources_allowed: bool,
    ) -> RestartServiceAction {
        RestartServiceAction {
            selector,
            k8s_resource_type,
            event_details,
            is_cluster_wide_resources_allowed,
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
            &target.kube,
            target.environment.namespace(),
            &self.selector,
            self.k8s_resource_type.clone(),
            self.is_cluster_wide_resources_allowed,
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
    kube: &kube::Client,
    namespace: &str,
    selector: &str,
    k8s_resource_type: K8sResourceType,
    is_cluster_wide_resources_allowed: bool,
) -> Result<(), kube::Error> {
    match k8s_resource_type {
        K8sResourceType::StateFulSet => {
            let (pods_api, pods_list_params, most_recent_pod_start_time_before_restart) =
                get_most_recent_pod_start_time(kube, namespace, selector, is_cluster_wide_resources_allowed).await?;
            let expected_number_of_replicas =
                restart_statefulset(kube, namespace, &pods_list_params, is_cluster_wide_resources_allowed).await?;
            wait_until_service_pods_are_restarted(
                &pods_api,
                &pods_list_params,
                expected_number_of_replicas,
                &most_recent_pod_start_time_before_restart,
            )
            .await?;
        }
        K8sResourceType::Deployment => {
            let (pods_api, pods_list_params, most_recent_pod_start_time_before_restart) =
                get_most_recent_pod_start_time(kube, namespace, selector, is_cluster_wide_resources_allowed).await?;
            let expected_number_of_replicas =
                restart_deployment(kube, namespace, &pods_list_params, is_cluster_wide_resources_allowed).await?;
            wait_until_service_pods_are_restarted(
                &pods_api,
                &pods_list_params,
                expected_number_of_replicas,
                &most_recent_pod_start_time_before_restart,
            )
            .await?;
        }
        K8sResourceType::DaemonSet => {
            restart_daemon_set(
                kube,
                namespace,
                &ListParams::default().labels(selector),
                is_cluster_wide_resources_allowed,
            )
            .await?;
        }
        K8sResourceType::CronJob => {}
        K8sResourceType::Job => {}
    };

    Ok(())
}

async fn get_most_recent_pod_start_time(
    kube: &Client,
    namespace: &str,
    selector: &str,
    is_cluster_wide_resources_allowed: bool,
) -> Result<(Api<Pod>, ListParams, Time), Error> {
    // find current service pods running before restart
    let pods_api: Api<Pod> = if is_cluster_wide_resources_allowed {
        Api::all(kube.clone())
    } else {
        Api::namespaced(kube.clone(), namespace)
    };
    let pods_list_params = ListParams::default().labels(selector);
    let service_pods_before_restart = pods_api.list(&pods_list_params).await?.items;

    // prepare predicate to wait after resource restart
    let most_recent_pod_start_time_before_restart = service_pods_before_restart
        .into_iter()
        .filter_map(|pod| pod.status.and_then(|it| it.start_time))
        .reduce(|acc, current| if acc.gt(&current) { acc } else { current })
        // If no pod exists, returns current date
        .or_else(|| Some(Time(Utc::now())))
        .expect("Should retrieve either most recent pod.status.time.date_time or use DateTime<Utc> now");
    Ok((pods_api, pods_list_params, most_recent_pod_start_time_before_restart))
}

async fn restart_deployment(
    kube: &kube::Client,
    namespace: &str,
    pods_list_params: &ListParams,
    is_cluster_wide_resources_allowed: bool,
) -> Result<i32, kube::Error> {
    let deployments_api: Api<Deployment> = if is_cluster_wide_resources_allowed {
        Api::all(kube.clone())
    } else {
        Api::namespaced(kube.clone(), namespace)
    };
    let deployments = deployments_api.list(pods_list_params).await?;

    if deployments.items.is_empty() {
        return Ok(0);
    }

    if deployments.items.len() != 1 {
        let unexpected_list_of_deployments_error =
            kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(format!(
                "Unexpected list of deployments: found {} instead of 1",
                deployments.items.len()
            )));
        return Err(unexpected_list_of_deployments_error);
    }

    let deployment = deployments.items.first().unwrap();

    let number_of_replicas = match deployment.spec.as_ref().and_then(|it| it.replicas) {
        None => {
            let undefined_number_of_replicas = kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(
                "Undefined number of replicas (deployment.spec.replicas)",
            ));
            return Err(undefined_number_of_replicas);
        }
        Some(replicas) => replicas,
    };

    let deployment_name = deployment.metadata.clone().name.unwrap_or_default();
    let deployment_namespace = deployment.metadata.clone().namespace.unwrap_or_default();
    let deployments_api: Api<Deployment> = Api::namespaced(kube.clone(), &deployment_namespace);
    deployments_api.restart(&deployment_name).await?;

    Ok(number_of_replicas)
}

async fn restart_statefulset(
    kube: &kube::Client,
    namespace: &str,
    pods_list_params: &ListParams,
    is_cluster_wide_resources_allowed: bool,
) -> Result<i32, kube::Error> {
    let statefulset_api: Api<StatefulSet> = if is_cluster_wide_resources_allowed {
        Api::all(kube.clone())
    } else {
        Api::namespaced(kube.clone(), namespace)
    };
    let statefulsets = statefulset_api.list(pods_list_params).await?;

    if statefulsets.items.is_empty() {
        return Ok(0);
    }

    if statefulsets.items.len() != 1 {
        let manual_kube_error = kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(format!(
            "Unexpected list of statefulsets: found {} instead of 1",
            statefulsets.items.len()
        )));
        return Err(manual_kube_error);
    }

    let statefulset = statefulsets.items.first().unwrap();

    let number_of_replicas = match statefulset.spec.as_ref().and_then(|it| it.replicas) {
        None => {
            let undefined_number_of_replicas = kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(
                "Undefined number of replicas (statefulset.spec.replicas)",
            ));
            return Err(undefined_number_of_replicas);
        }
        Some(replicas) => replicas,
    };
    let statefulset_name = statefulset.metadata.clone().name.unwrap_or_default();
    let deployment_namespace = statefulset.metadata.clone().namespace.unwrap_or_default();
    let statefulset_api: Api<StatefulSet> = Api::namespaced(kube.clone(), &deployment_namespace);
    statefulset_api.restart(&statefulset_name).await?;

    Ok(number_of_replicas)
}

async fn restart_daemon_set(
    kube: &kube::Client,
    namespace: &str,
    pods_list_params: &ListParams,
    is_cluster_wide_resources_allowed: bool,
) -> Result<(), kube::Error> {
    let daemon_set_api: Api<DaemonSet> = if is_cluster_wide_resources_allowed {
        Api::all(kube.clone())
    } else {
        Api::namespaced(kube.clone(), namespace)
    };
    let daemon_sets = daemon_set_api.list(pods_list_params).await?;

    if daemon_sets.items.is_empty() {
        return Ok(());
    }

    if daemon_sets.items.len() != 1 {
        let manual_kube_error = kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(format!(
            "Unexpected list of daemon set: found {} instead of 1",
            daemon_sets.items.len()
        )));
        return Err(manual_kube_error);
    }

    let daemon_set = daemon_sets.items.first().unwrap();
    let daemon_set_name = daemon_set.metadata.clone().name.unwrap_or_default();
    let deployment_namespace = daemon_set.metadata.clone().namespace.unwrap_or_default();
    let daemon_set_api: Api<DaemonSet> = Api::namespaced(kube.clone(), &deployment_namespace);
    daemon_set_api.restart(&daemon_set_name).await?;

    Ok(())
}

async fn wait_until_service_pods_are_restarted(
    pods_api: &Api<Pod>,
    pods_list_params: &ListParams,
    pods_length_before_restart: i32,
    most_recent_pod_start_time_before_restart: &Time,
) -> Result<(), kube::Error> {
    loop {
        let running_service_pods = pods_api.list(pods_list_params).await?;

        let number_of_pods_running: i32 = running_service_pods
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
            .count() as i32;

        if number_of_pods_running == pods_length_before_restart {
            break;
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
    }

    Ok(())
}

#[cfg(feature = "test-local-kube")]
#[cfg(test)]
mod tests {
    use crate::environment::action::restart_service::restart_daemon_set;
    use crate::environment::action::test_utils::NamespaceForTest;
    use crate::environment::action::test_utils::get_simple_daemon_set;
    use function_name::named;
    use k8s_openapi::api::apps::v1::DaemonSet;
    use kube::Api;
    use kube::api::ListParams;
    use kube::api::PostParams;
    use kube::runtime::wait::{Condition, await_condition};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn has_daemon_set_number_ready() -> impl Condition<DaemonSet> {
        move |daemon_set: Option<&DaemonSet>| {
            daemon_set
                .and_then(|d| d.status.as_ref())
                .map(|status| status.number_ready)
                .unwrap_or(0)
                == 1
        }
    }

    fn has_daemon_set_generation_annotation(expected_generation: String) -> impl Condition<DaemonSet> {
        move |daemon_set: Option<&DaemonSet>| {
            daemon_set
                .and_then(|d| d.metadata.annotations.as_ref())
                .and_then(|annotations| annotations.get("deprecated.daemonset.template.generation"))
                == Some(&expected_generation)
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_restart_daemon_set() -> Result<(), Box<dyn std::error::Error>> {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Cannot install rustls crypto provider");

        let kube_client = kube::Client::try_default().await.unwrap();
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let daemon_sets: Api<DaemonSet> = Api::namespaced(kube_client.clone(), &namespace);
        let daemon_set: DaemonSet = get_simple_daemon_set();
        let daemon_set_name = daemon_set.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={daemon_set_name}");

        // create simple daemon set and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;
        daemon_sets.create(&PostParams::default(), &daemon_set).await.unwrap();
        tokio::time::timeout(
            timeout,
            await_condition(daemon_sets.clone(), &daemon_set_name, has_daemon_set_number_ready()),
        )
        .await??;

        let daemonsets: Api<DaemonSet> = Api::namespaced(kube_client.clone(), &namespace);
        tokio::time::timeout(
            timeout,
            await_condition(
                daemonsets.clone(),
                &daemon_set_name,
                has_daemon_set_generation_annotation("1".to_string()),
            ),
        )
        .await??;

        // restarting our daemon set
        tokio::time::timeout(
            timeout,
            restart_daemon_set(&kube_client, &namespace, &ListParams::default().labels(&selector), false),
        )
        .await??;
        tokio::time::sleep(Duration::from_secs(2)).await;

        let daemonsets: Api<DaemonSet> = Api::namespaced(kube_client.clone(), &namespace);
        tokio::time::timeout(
            timeout,
            await_condition(
                daemonsets.clone(),
                &daemon_set_name,
                has_daemon_set_generation_annotation("2".to_string()),
            ),
        )
        .await??;

        Ok(())
    }
}
