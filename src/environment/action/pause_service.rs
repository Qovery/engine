use crate::environment::action::{DeploymentAction, K8sResourceType};
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::runtime::block_on;
use json_patch::{AddOperation, PatchOperation, RemoveOperation};
use jsonptr::{Pointer, PointerBuf};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::autoscaling::v1::{Scale, ScaleSpec};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{ListParams, Patch, PatchParams};
use kube::runtime::wait::{Condition, await_condition};
use kube::{Api, Client};
use serde_json::Value;
use std::time::Duration;

fn has_deployment_ready_replicas(nb_ready_replicas: usize) -> impl Condition<Deployment> {
    move |deployment: Option<&Deployment>| {
        deployment
            .and_then(|d| d.status.as_ref())
            .and_then(|status| status.ready_replicas.as_ref())
            .unwrap_or(&0)
            == &(nb_ready_replicas as i32)
    }
}

fn has_statefulset_ready_replicas(nb_ready_replicas: usize) -> impl Condition<StatefulSet> {
    move |deployment: Option<&StatefulSet>| {
        deployment
            .and_then(|d| d.status.as_ref())
            .and_then(|status| status.ready_replicas.as_ref())
            .unwrap_or(&0)
            == &(nb_ready_replicas as i32)
    }
}

fn has_cron_job_suspended_value(suspend: bool) -> impl Condition<CronJob> {
    move |cron_job: Option<&CronJob>| {
        cron_job
            .and_then(|d| d.spec.as_ref())
            .and_then(|spec| spec.suspend.as_ref())
            == Some(&suspend)
    }
}

fn has_daemonset_node_selector(selector_key: String, selector_value: String) -> impl Condition<DaemonSet> {
    move |daemonset: Option<&DaemonSet>| {
        daemonset
            .and_then(|d| d.spec.as_ref())
            .and_then(|spec| spec.template.spec.as_ref())
            .and_then(|pod_spec| pod_spec.node_selector.as_ref())
            .map(|selector| selector.get(&selector_key) == Some(&selector_value))
            .unwrap_or(false)
    }
}

fn has_not_daemonset_node_selector(selector_key: String) -> impl Condition<DaemonSet> {
    move |daemonset: Option<&DaemonSet>| {
        daemonset
            .and_then(|d| d.spec.as_ref())
            .and_then(|spec| spec.template.spec.as_ref())
            .and_then(|pod_spec| pod_spec.node_selector.as_ref())
            .is_none_or(|node_selector| node_selector.get(&selector_key).is_none())
    }
}

const PAUSE_SELECTOR_KEY: &str = "qovery-pause";
const PAUSE_SELECTOR_VALUE: &str = "true";

async fn pause_service(
    kube: &kube::Client,
    namespace: &str,
    selector: &str,
    desired_size: usize, // only for test, normal behavior assume 0
    k8s_resource_type: K8sResourceType,
    is_cluster_wide_resources_allowed: bool,
    wait_for_pods: bool,
) -> Result<(), kube::Error> {
    // We don't need to remove HPA, if we set desired replicas to 0, hpa disable itself until we change it back
    // https://kubernetes.io/docs/tasks/run-application/horizontal-pod-autoscale/#implicit-maintenance-mode-deactivation

    match k8s_resource_type {
        K8sResourceType::StateFulSet => {
            let (list_params, patch_params, patch) = get_patch_merge(selector, desired_size);
            let statefulsets: Api<StatefulSet> = if is_cluster_wide_resources_allowed {
                Api::all(kube.clone())
            } else {
                Api::namespaced(kube.clone(), namespace)
            };
            for statefulset in statefulsets.list(&list_params).await? {
                if let (Some(namespace), Some(name)) = (statefulset.metadata.namespace, statefulset.metadata.name) {
                    let statefulsets: Api<StatefulSet> = Api::namespaced(kube.clone(), &namespace); // patch_scale need to have statefulsets with namespace
                    statefulsets.patch_scale(&name, &patch_params, &patch).await?;
                    let _ = await_condition(statefulsets.clone(), &name, has_statefulset_ready_replicas(0)).await;
                }
            }
            if wait_for_pods {
                wait_for_pods_to_be_in_correct_state(
                    kube,
                    namespace,
                    desired_size,
                    is_cluster_wide_resources_allowed,
                    &list_params,
                )
                .await;
            }
        }
        K8sResourceType::Deployment => {
            let (list_params, patch_params, patch) = get_patch_merge(selector, desired_size);
            let deployments: Api<Deployment> = if is_cluster_wide_resources_allowed {
                Api::all(kube.clone())
            } else {
                Api::namespaced(kube.clone(), namespace)
            };
            for deployment in deployments.list(&list_params).await? {
                if let (Some(namespace), Some(name)) = (deployment.metadata.namespace, deployment.metadata.name) {
                    let deployments: Api<Deployment> = Api::namespaced(kube.clone(), &namespace); // patch_scale needs to have deployments with namespace
                    deployments.patch_scale(&name, &patch_params, &patch).await?;
                    let _ = await_condition(deployments.clone(), &name, has_deployment_ready_replicas(0)).await;
                }
            }
            if wait_for_pods {
                wait_for_pods_to_be_in_correct_state(
                    kube,
                    namespace,
                    desired_size,
                    is_cluster_wide_resources_allowed,
                    &list_params,
                )
                .await;
            }
        }
        K8sResourceType::CronJob => {
            let (list_params, patch_params, patch) = get_patch_suspend(selector, desired_size == 0);
            let cron_jobs: Api<CronJob> = if is_cluster_wide_resources_allowed {
                Api::all(kube.clone())
            } else {
                Api::namespaced(kube.clone(), namespace)
            };
            for cron_job in cron_jobs.list(&list_params).await? {
                if let (Some(namespace), Some(name)) = (cron_job.metadata.namespace, cron_job.metadata.name) {
                    let cron_jobs: Api<CronJob> = Api::namespaced(kube.clone(), &namespace); // patch needs to have cron_jobs with namespace
                    cron_jobs.patch(&name, &patch_params, &patch).await?;
                    let _ = await_condition(cron_jobs.clone(), &name, has_cron_job_suspended_value(desired_size == 0))
                        .await;
                }
            }
        }
        K8sResourceType::DaemonSet => {
            let list_params = ListParams::default().labels(selector);
            let daemonsets: Api<DaemonSet> = if is_cluster_wide_resources_allowed {
                Api::all(kube.clone())
            } else {
                Api::namespaced(kube.clone(), namespace)
            };

            for daemonset in daemonsets.list(&list_params).await? {
                if let (Some(namespace), Some(name)) = (daemonset.metadata.namespace, daemonset.metadata.name) {
                    let already_have_some_selectors = daemonset
                        .spec
                        .and_then(|spec| spec.template.spec)
                        .and_then(|pod_spec| pod_spec.node_selector)
                        .is_some();

                    let (patch_params, patch) = get_patch_add_node_selector(
                        already_have_some_selectors,
                        PAUSE_SELECTOR_KEY,
                        PAUSE_SELECTOR_VALUE,
                    );

                    let daemon_sets: Api<DaemonSet> = Api::namespaced(kube.clone(), &namespace);
                    daemon_sets.patch(&name, &patch_params, &patch).await?;
                    let _ = await_condition(
                        daemon_sets.clone(),
                        &name,
                        has_daemonset_node_selector(PAUSE_SELECTOR_KEY.to_string(), PAUSE_SELECTOR_VALUE.to_string()),
                    )
                    .await;
                }
            }
            if wait_for_pods {
                wait_for_pods_to_be_in_correct_state(
                    kube,
                    namespace,
                    desired_size,
                    is_cluster_wide_resources_allowed,
                    &list_params,
                )
                .await;
            }
        }
        K8sResourceType::Job => {}
    };

    Ok(())
}

async fn unpause_service_if_needed(
    kube: &kube::Client,
    namespace: &str,
    selector: &str,
    k8s_resource_type: K8sResourceType,
    is_cluster_wide_resources_allowed: bool,
) -> Result<(), kube::Error> {
    match k8s_resource_type {
        K8sResourceType::StateFulSet => {
            let (list_params, patch_params, patch) = get_patch_merge(selector, 1);
            let statefulsets: Api<StatefulSet> = if is_cluster_wide_resources_allowed {
                Api::all(kube.clone())
            } else {
                Api::namespaced(kube.clone(), namespace)
            };
            for statefulset in statefulsets.list(&list_params).await? {
                if statefulset.status.map(|s| s.replicas).unwrap_or(0) == 0 {
                    if let (Some(namespace), Some(name)) = (statefulset.metadata.namespace, statefulset.metadata.name) {
                        let statefulsets: Api<StatefulSet> = Api::namespaced(kube.clone(), &namespace); // patch_scale needs to have statefulsets with namespace
                        statefulsets.patch_scale(&name, &patch_params, &patch).await?;
                    }
                }
            }
        }
        K8sResourceType::Deployment => {
            let (list_params, patch_params, patch) = get_patch_merge(selector, 1);
            let deployments: Api<Deployment> = if is_cluster_wide_resources_allowed {
                Api::all(kube.clone())
            } else {
                Api::namespaced(kube.clone(), namespace)
            };
            for deployment in deployments.list(&list_params).await? {
                if deployment.status.and_then(|s| s.replicas).unwrap_or(0) == 0 {
                    if let (Some(namespace), Some(name)) = (deployment.metadata.namespace, deployment.metadata.name) {
                        let deployments: Api<Deployment> = Api::namespaced(kube.clone(), &namespace); // patch_scale needs to have deployments with namespace
                        deployments.patch_scale(&name, &patch_params, &patch).await?;
                    }
                }
            }
        }
        K8sResourceType::CronJob => {
            let (list_params, patch_params, patch) = get_patch_suspend(selector, false);
            let cron_jobs: Api<CronJob> = if is_cluster_wide_resources_allowed {
                Api::all(kube.clone())
            } else {
                Api::namespaced(kube.clone(), namespace)
            };
            for cron_job in cron_jobs.list(&list_params).await? {
                if let (Some(namespace), Some(name)) = (cron_job.metadata.namespace, cron_job.metadata.name) {
                    let cron_jobs: Api<CronJob> = Api::namespaced(kube.clone(), &namespace); // patch needs to have cron_jobs with namespace
                    cron_jobs.patch(&name, &patch_params, &patch).await?;
                }
            }
        }
        K8sResourceType::DaemonSet => {
            let list_params = ListParams::default().labels(selector);
            let daemonsets: Api<DaemonSet> = if is_cluster_wide_resources_allowed {
                Api::all(kube.clone())
            } else {
                Api::namespaced(kube.clone(), namespace)
            };
            for daemonset in daemonsets.list(&list_params).await? {
                if let (Some(namespace), Some(name)) = (daemonset.metadata.namespace, daemonset.metadata.name) {
                    let current_selectors = daemonset
                        .spec
                        .and_then(|spec| spec.template.spec)
                        .and_then(|pod_spec| pod_spec.node_selector)
                        .unwrap_or_default();

                    if current_selectors.contains_key(PAUSE_SELECTOR_KEY) {
                        let (patch_params, patch) = get_patch_remove_node_selector(PAUSE_SELECTOR_KEY);

                        let daemon_sets: Api<DaemonSet> = Api::namespaced(kube.clone(), &namespace);
                        daemon_sets.patch(&name, &patch_params, &patch).await?;
                        let _ = await_condition(
                            daemon_sets.clone(),
                            &name,
                            has_not_daemonset_node_selector(PAUSE_SELECTOR_KEY.to_string()),
                        )
                        .await;
                    }
                }
            }
        }
        K8sResourceType::Job => {}
    }

    Ok(())
}

fn get_patch_merge(selector: &str, desired_size: usize) -> (ListParams, PatchParams, Patch<Scale>) {
    let list_params = ListParams::default().labels(selector);
    let patch_params = PatchParams::default();
    let new_scale = Scale {
        metadata: Default::default(),
        spec: Some(ScaleSpec {
            replicas: Some(desired_size as i32),
        }),
        status: None,
    };
    let patch = Patch::Merge(new_scale);
    (list_params, patch_params, patch)
}

fn get_patch_suspend(selector: &str, desired_suspend_value: bool) -> (ListParams, PatchParams, Patch<Scale>) {
    let list_params = ListParams::default().labels(selector);
    let patch_params = PatchParams::default();
    let json_patch = json_patch::Patch(vec![json_patch::PatchOperation::Replace(json_patch::ReplaceOperation {
        path: Pointer::from_static("/spec/suspend").to_buf(),
        value: Value::Bool(desired_suspend_value),
    })]);
    let patch = Patch::Json(json_patch);
    (list_params, patch_params, patch)
}

fn get_patch_add_node_selector(
    already_have_some_selectors: bool,
    node_selector_key: &str,
    node_selector_value: &str,
) -> (PatchParams, Patch<Value>) {
    let patch_params = PatchParams::apply("node-selector-add-patch");

    let mut patch_operations = vec![];
    if !already_have_some_selectors {
        patch_operations.push(PatchOperation::Add(AddOperation {
            path: Pointer::from_static("/spec/template/spec/nodeSelector").to_buf(),
            value: Value::Object(serde_json::Map::new()),
        }));
    }
    let patch_path_str = format!("/spec/template/spec/nodeSelector/{}", node_selector_key);
    patch_operations.push(PatchOperation::Add(AddOperation {
        path: PointerBuf::parse(&patch_path_str).unwrap_or_default(),
        value: Value::String(node_selector_value.to_string()),
    }));

    (patch_params, Patch::Json(json_patch::Patch(patch_operations)))
}

fn get_patch_remove_node_selector(key: &str) -> (PatchParams, Patch<Value>) {
    let patch_params = PatchParams::apply("node-selector-remove-patch");

    let patch_path_str = format!("/spec/template/spec/nodeSelector/{}", key);
    let patch_operations = vec![PatchOperation::Remove(RemoveOperation {
        path: PointerBuf::parse(&patch_path_str).unwrap_or_default(),
    })];

    (patch_params, Patch::Json(json_patch::Patch(patch_operations)))
}

async fn wait_for_pods_to_be_in_correct_state(
    kube: &Client,
    namespace: &str,
    desired_size: usize,
    is_cluster_wide_resources_allowed: bool,
    list_params: &ListParams,
) {
    // Wait for pod to be destroyed/correctly scaled
    // Checking for readyness is not enough, as when downscaling pods in terminating are not listed in (ready_)replicas
    let pods: Api<Pod> = if is_cluster_wide_resources_allowed {
        Api::all(kube.clone())
    } else {
        Api::namespaced(kube.clone(), namespace)
    };
    while let Ok(pod) = pods.list(list_params).await {
        if pod.items.len() == desired_size {
            break;
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

pub struct PauseServiceAction {
    selector: String,
    k8s_resource_type: K8sResourceType,
    event_details: EventDetails,
    timeout: Duration,
    is_cluster_wide_resources_allowed: bool,
    wait_for_pods: bool,
}

impl PauseServiceAction {
    pub fn new(
        selector: String,
        is_stateful: bool,
        timeout: Duration,
        event_details: EventDetails,
        wait_for_pods: bool,
    ) -> PauseServiceAction {
        PauseServiceAction {
            selector,
            k8s_resource_type: if is_stateful {
                K8sResourceType::StateFulSet
            } else {
                K8sResourceType::Deployment
            },
            timeout,
            event_details,
            is_cluster_wide_resources_allowed: false,
            wait_for_pods,
        }
    }

    pub fn new_with_resource_type(
        selector: String,
        k8s_resource_type: K8sResourceType,
        timeout: Duration,
        event_details: EventDetails,
        is_cluster_wide_resources_allowed: bool,
        wait_for_pods: bool,
    ) -> PauseServiceAction {
        PauseServiceAction {
            selector,
            k8s_resource_type,
            timeout,
            event_details,
            is_cluster_wide_resources_allowed,
            wait_for_pods,
        }
    }

    pub fn unpause_if_needed(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let fut = unpause_service_if_needed(
            &target.kube,
            target.environment.namespace(),
            &self.selector,
            self.k8s_resource_type.clone(),
            self.is_cluster_wide_resources_allowed,
        );

        match block_on(async { tokio::time::timeout(self.timeout, fut).await }) {
            // Happy path
            Ok(Ok(())) => {}

            // error during scaling
            Ok(Err(kube_err)) => {
                let command_error = CommandError::new_from_safe_message(kube_err.to_string());
                return Err(Box::new(EngineError::new_k8s_scale_replicas(
                    self.event_details.clone(),
                    self.selector.clone(),
                    target.environment.namespace().to_string(),
                    0,
                    command_error,
                )));
            }
            // timeout
            Err(_) => {
                let command_error = CommandError::new_from_safe_message(format!(
                    "Timeout of {}s exceeded while un-pausing service",
                    self.timeout.as_secs()
                ));
                return Err(Box::new(EngineError::new_k8s_scale_replicas(
                    self.event_details.clone(),
                    self.selector.clone(),
                    target.environment.namespace().to_string(),
                    0,
                    command_error,
                )));
            }
        }

        Ok(())
    }
}

impl DeploymentAction for PauseServiceAction {
    fn on_create(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let fut = pause_service(
            &target.kube,
            target.environment.namespace(),
            &self.selector,
            0,
            self.k8s_resource_type.clone(),
            self.is_cluster_wide_resources_allowed,
            self.wait_for_pods,
        );

        // Async block is necessary because tokio::time::timeout require a living tokio runtime, which does not exist
        // outside of the block_on. So must wrap it in an async task that will be exec inside the block_on
        let ret = block_on(async { tokio::time::timeout(self.timeout, fut).await });

        match ret {
            // Happy path
            Ok(Ok(())) => {}

            // error during scaling
            Ok(Err(kube_err)) => {
                let command_error = CommandError::new_from_safe_message(kube_err.to_string());
                return Err(Box::new(EngineError::new_k8s_scale_replicas(
                    self.event_details.clone(),
                    self.selector.clone(),
                    target.environment.namespace().to_string(),
                    0,
                    command_error,
                )));
            }
            // timeout
            Err(_) => {
                let command_error = CommandError::new_from_safe_message(format!(
                    "Timeout of {}s exceeded while scaling down service",
                    self.timeout.as_secs()
                ));
                return Err(Box::new(EngineError::new_k8s_scale_replicas(
                    self.event_details.clone(),
                    self.selector.clone(),
                    target.environment.namespace().to_string(),
                    0,
                    command_error,
                )));
            }
        }

        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }
}

#[cfg(feature = "test-local-kube")]
#[cfg(test)]
mod tests {
    use crate::environment::action::pause_service::{
        K8sResourceType, PAUSE_SELECTOR_KEY, PAUSE_SELECTOR_VALUE, has_cron_job_suspended_value,
        has_daemonset_node_selector, has_deployment_ready_replicas, has_not_daemonset_node_selector,
        has_statefulset_ready_replicas, pause_service, unpause_service_if_needed,
    };
    use crate::environment::action::test_utils::{
        NamespaceForTest, get_simple_cron_job, get_simple_daemon_set, get_simple_daemonset_with_node_selector,
        get_simple_deployment, get_simple_hpa, get_simple_statefulset,
    };
    use function_name::named;
    use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
    use k8s_openapi::api::autoscaling::v1::HorizontalPodAutoscaler;
    use k8s_openapi::api::batch::v1::CronJob;
    use kube::Api;
    use kube::api::PostParams;
    use kube::runtime::conditions::Condition;
    use kube::runtime::wait::await_condition;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    async fn get_kube_client() -> kube::Client {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Cannot install rustls crypto provider");

        kube::Client::try_default().await.expect("create client")
    }
    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_scale_deployment() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = get_kube_client().await;
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let deployments: Api<Deployment> = Api::namespaced(kube_client.clone(), &namespace);
        let deployment: Deployment = get_simple_deployment();
        let hpas: Api<HorizontalPodAutoscaler> = Api::namespaced(kube_client.clone(), &namespace);
        let hpa = get_simple_hpa();

        let app_name = deployment.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={app_name}");

        // create simple deployment and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        hpas.create(&PostParams::default(), &hpa).await.expect("create hpas");
        deployments
            .create(&PostParams::default(), &deployment)
            .await
            .expect("create deployment");
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(1)),
        )
        .await??;

        // Scaling a service that does not exist should not fail
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                "app=totototo",
                0,
                K8sResourceType::Deployment,
                false,
                true,
            ),
        )
        .await??;

        // Try to scale down our deployment
        tokio::time::timeout(
            timeout,
            pause_service(&kube_client, &namespace, &selector, 0, K8sResourceType::Deployment, false, true),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(0)),
        )
        .await??;

        // Try to scale up our deployment
        tokio::time::timeout(
            timeout,
            pause_service(&kube_client, &namespace, &selector, 1, K8sResourceType::Deployment, false, true),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(1)),
        )
        .await??;

        drop(_ns);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_scale_deployment_with_statefulset() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = get_kube_client().await;
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let deployments: Api<Deployment> = Api::namespaced(kube_client.clone(), &namespace);
        let deployment: Deployment = get_simple_deployment();
        let statefulsets: Api<StatefulSet> = Api::namespaced(kube_client.clone(), &namespace);
        let statefulset: StatefulSet = get_simple_statefulset();
        let hpas: Api<HorizontalPodAutoscaler> = Api::namespaced(kube_client.clone(), &namespace);
        let hpa = get_simple_hpa();

        let app_name = deployment.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={app_name}");

        // create simple deployment and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        hpas.create(&PostParams::default(), &hpa).await.expect("create hpas");
        deployments
            .create(&PostParams::default(), &deployment)
            .await
            .expect("create deployment");
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(1)),
        )
        .await??;

        statefulsets
            .create(&PostParams::default(), &statefulset)
            .await
            .expect("create statefulset");
        tokio::time::timeout(
            timeout,
            await_condition(statefulsets.clone(), &app_name, has_statefulset_ready_replicas(1)),
        )
        .await??;

        // Scaling a service that does not exist should not fail
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                "app=totototo",
                0,
                K8sResourceType::Deployment,
                false,
                false,
            ),
        )
        .await??;

        // Try to scale down our deployment
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                &selector,
                0,
                K8sResourceType::Deployment,
                false,
                false,
            ),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(0)),
        )
        .await??;

        // Try to scale up our deployment
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                &selector,
                1,
                K8sResourceType::Deployment,
                false,
                false,
            ),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(1)),
        )
        .await??;

        drop(_ns);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_scale_statefulset() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = get_kube_client().await;
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let statefulsets: Api<StatefulSet> = Api::namespaced(kube_client.clone(), &namespace);
        let statefulset: StatefulSet = get_simple_statefulset();
        let app_name = statefulset.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={app_name}");

        // create simple deployment and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        statefulsets
            .create(&PostParams::default(), &statefulset)
            .await
            .expect("create statefulset");
        tokio::time::timeout(
            timeout,
            await_condition(statefulsets.clone(), &app_name, has_statefulset_ready_replicas(1)),
        )
        .await??;

        // Scaling a service that does not exist should not fail
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                "app=totototo",
                0,
                K8sResourceType::StateFulSet,
                false,
                true,
            ),
        )
        .await??;

        // Try to scale down our deployment
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                &selector,
                0,
                K8sResourceType::StateFulSet,
                false,
                true,
            ),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(statefulsets.clone(), &app_name, has_statefulset_ready_replicas(0)),
        )
        .await??;

        // Try to scale up our deployment
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                &selector,
                1,
                K8sResourceType::StateFulSet,
                false,
                true,
            ),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(statefulsets.clone(), &app_name, has_statefulset_ready_replicas(1)),
        )
        .await??;

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_scale_cron_job() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = get_kube_client().await;
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let cron_jobs: Api<CronJob> = Api::namespaced(kube_client.clone(), &namespace);
        let cron_job: CronJob = get_simple_cron_job();
        let app_name = cron_job.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={app_name}");

        // create simple cron job and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        cron_jobs
            .create(&PostParams::default(), &cron_job)
            .await
            .expect("create cron job");
        tokio::time::timeout(
            timeout,
            await_condition(cron_jobs.clone(), &app_name, has_cron_job_suspended_value(false)),
        )
        .await??;

        // Scaling a cron job that does not exist should not fail
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                "app=totototo",
                0,
                K8sResourceType::CronJob,
                false,
                true,
            ),
        )
        .await??;

        // Try to suspend our cron-job
        tokio::time::timeout(
            timeout,
            pause_service(&kube_client, &namespace, &selector, 0, K8sResourceType::CronJob, false, true),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(cron_jobs.clone(), &app_name, has_cron_job_suspended_value(true)),
        )
        .await??;

        // Try to resume our cron-job
        tokio::time::timeout(
            timeout,
            pause_service(&kube_client, &namespace, &selector, 1, K8sResourceType::CronJob, false, true),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(cron_jobs.clone(), &app_name, has_cron_job_suspended_value(false)),
        )
        .await??;

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_unpause_deployment() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = get_kube_client().await;
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let deployments: Api<Deployment> = Api::namespaced(kube_client.clone(), &namespace);
        let deployment: Deployment = get_simple_deployment();
        let hpas: Api<HorizontalPodAutoscaler> = Api::namespaced(kube_client.clone(), &namespace);
        let hpa = get_simple_hpa();

        let app_name = deployment.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={app_name}");

        // create simple deployment and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        hpas.create(&PostParams::default(), &hpa).await.expect("create hpas");
        deployments
            .create(&PostParams::default(), &deployment)
            .await
            .expect("create deployment");
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(1)),
        )
        .await??;

        // Try to scale down our deployment
        tokio::time::timeout(
            timeout,
            pause_service(&kube_client, &namespace, &selector, 0, K8sResourceType::Deployment, false, true),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(0)),
        )
        .await??;

        tokio::time::timeout(
            timeout,
            unpause_service_if_needed(&kube_client, &namespace, &selector, K8sResourceType::Deployment, false),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(1)),
        )
        .await??;

        Ok(())
    }

    fn has_daemon_set_number_ready() -> impl Condition<DaemonSet> {
        move |daemon_set: Option<&DaemonSet>| {
            daemon_set
                .and_then(|d| d.status.as_ref())
                .map(|status| status.number_ready)
                .unwrap_or(0)
                == 1
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_pause_daemonset() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = get_kube_client().await;
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let daemonsets: Api<DaemonSet> = Api::namespaced(kube_client.clone(), &namespace);
        let daemonset: DaemonSet = get_simple_daemon_set();

        let app_name = daemonset.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={app_name}");

        // create simple deployment and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        daemonsets
            .create(&PostParams::default(), &daemonset)
            .await
            .expect("daemonset created");
        tokio::time::timeout(
            timeout,
            await_condition(daemonsets.clone(), &app_name, has_daemon_set_number_ready()),
        )
        .await??;

        //pause a daemonset that does not exist
        tokio::time::timeout(
            timeout,
            pause_service(
                &kube_client,
                &namespace,
                "app=totototo",
                0,
                K8sResourceType::DaemonSet,
                false,
                true,
            ),
        )
        .await??;

        // Try to pause our daemonset
        tokio::time::timeout(
            timeout,
            pause_service(&kube_client, &namespace, &selector, 0, K8sResourceType::DaemonSet, false, true),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(
                daemonsets.clone(),
                &app_name,
                has_daemonset_node_selector(PAUSE_SELECTOR_KEY.to_string(), PAUSE_SELECTOR_VALUE.to_string()),
            ),
        )
        .await??;

        // Try to restart our daemonset
        tokio::time::timeout(
            timeout,
            unpause_service_if_needed(&kube_client, &namespace, &selector, K8sResourceType::DaemonSet, false),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(daemonsets.clone(), &app_name, has_daemon_set_number_ready()),
        )
        .await??;

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_pause_daemonset_having_node_selector() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = get_kube_client().await;
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let daemonsets: Api<DaemonSet> = Api::namespaced(kube_client.clone(), &namespace);
        let daemonset: DaemonSet = get_simple_daemonset_with_node_selector();

        let app_name = daemonset.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={app_name}");

        // create simple deployment and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        daemonsets
            .create(&PostParams::default(), &daemonset)
            .await
            .expect("daemonset created");
        tokio::time::timeout(
            timeout,
            await_condition(
                daemonsets.clone(),
                &app_name,
                has_daemonset_node_selector("test-key".to_string(), "test-value".to_string()),
            ),
        )
        .await??;

        // Try to pause our daemonset
        tokio::time::timeout(
            timeout,
            pause_service(&kube_client, &namespace, &selector, 0, K8sResourceType::DaemonSet, false, true),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(
                daemonsets.clone(),
                &app_name,
                has_daemonset_node_selector(PAUSE_SELECTOR_KEY.to_string(), PAUSE_SELECTOR_VALUE.to_string()),
            ),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(
                daemonsets.clone(),
                &app_name,
                has_daemonset_node_selector("test-key".to_string(), "test-value".to_string()),
            ),
        )
        .await??;

        // Try to restart our daemonset
        tokio::time::timeout(
            timeout,
            unpause_service_if_needed(&kube_client, &namespace, &selector, K8sResourceType::DaemonSet, false),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(
                daemonsets.clone(),
                &app_name,
                has_daemonset_node_selector("test-key".to_string(), "test-value".to_string()),
            ),
        )
        .await??;
        tokio::time::timeout(
            timeout,
            await_condition(
                daemonsets.clone(),
                &app_name,
                has_not_daemonset_node_selector(PAUSE_SELECTOR_KEY.to_string()),
            ),
        )
        .await??;

        Ok(())
    }
}
