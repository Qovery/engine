use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::runtime::block_on;
use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
use k8s_openapi::api::autoscaling::v1::{Scale, ScaleSpec};
use kube::api::{ListParams, Patch, PatchParams};
use kube::runtime::wait::{await_condition, Condition};
use kube::Api;
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

async fn scale_service(
    kube: &kube::Client,
    namespace: &str,
    selector: &str,
    desired_size: usize,
    is_statefulset: bool,
) -> Result<(), kube::Error> {
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

    if is_statefulset {
        let statefulsets: Api<StatefulSet> = Api::namespaced(kube.clone(), namespace);
        for statefulset in statefulsets.list(&list_params).await? {
            if let Some(name) = statefulset.metadata.name {
                statefulsets.patch_scale(&name, &patch_params, &patch).await?;
                let _ = await_condition(statefulsets.clone(), &name, has_statefulset_ready_replicas(0)).await;
            }
        }
    } else {
        let deployments: Api<Deployment> = Api::namespaced(kube.clone(), namespace);
        for deployment in deployments.list(&list_params).await? {
            if let Some(name) = deployment.metadata.name {
                deployments.patch_scale(&name, &patch_params, &patch).await?;
                let _ = await_condition(deployments.clone(), &name, has_deployment_ready_replicas(0)).await;
            }
        }
    };

    Ok(())
}

pub struct PauseServiceAction {
    selector: String,
    is_statefulset: bool,
    event_details: EventDetails,
    timeout: Duration,
}

impl PauseServiceAction {
    pub fn new(
        selector: String,
        is_statefulset: bool,
        timeout: Duration,
        event_details: EventDetails,
    ) -> PauseServiceAction {
        PauseServiceAction {
            selector,
            is_statefulset,
            timeout,
            event_details,
        }
    }
}

impl DeploymentAction for PauseServiceAction {
    fn on_create(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let fut = scale_service(
            &target.kube,
            target.environment.namespace(),
            &self.selector,
            0,
            self.is_statefulset,
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
                return Err(EngineError::new_k8s_scale_replicas(
                    self.event_details.clone(),
                    self.selector.clone(),
                    target.environment.namespace().to_string(),
                    0,
                    command_error,
                ));
            }
            // timeout
            Err(_) => {
                let command_error = CommandError::new_from_safe_message(format!(
                    "Timout of {}s exceeded while scaling down service",
                    self.timeout.as_secs()
                ));
                return Err(EngineError::new_k8s_scale_replicas(
                    self.event_details.clone(),
                    self.selector.clone(),
                    target.environment.namespace().to_string(),
                    0,
                    command_error,
                ));
            }
        }

        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }
}

#[cfg(feature = "test-local-kube")]
#[cfg(test)]
mod tests {
    use crate::deployment_action::pause_service::{
        has_deployment_ready_replicas, has_statefulset_ready_replicas, scale_service,
    };
    use crate::deployment_action::test_utils::{get_simple_deployment, get_simple_statefulset, NamespaceForTest};
    use function_name::named;
    use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
    use kube::api::PostParams;
    use kube::runtime::wait::await_condition;
    use kube::Api;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[tokio::test(flavor = "multi_thread")]
    #[named]
    async fn test_scale_deployment() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = kube::Client::try_default().await.unwrap();
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let deployments: Api<Deployment> = Api::namespaced(kube_client.clone(), &namespace);
        let deployment: Deployment = get_simple_deployment();
        let app_name = deployment.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={}", app_name);

        // create simple deployment and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        deployments.create(&PostParams::default(), &deployment).await.unwrap();
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(1)),
        )
        .await??;

        // Scaling a service that does not exist should not fail
        tokio::time::timeout(timeout, scale_service(&kube_client, &namespace, "app=totototo", 0, false)).await??;

        // Try to scale down our deployment
        tokio::time::timeout(timeout, scale_service(&kube_client, &namespace, &selector, 0, false)).await??;
        tokio::time::timeout(
            timeout,
            await_condition(deployments.clone(), &app_name, has_deployment_ready_replicas(0)),
        )
        .await??;

        // Try to scale up our deployment
        tokio::time::timeout(timeout, scale_service(&kube_client, &namespace, &selector, 1, false)).await??;
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
        let kube_client = kube::Client::try_default().await.unwrap();
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let timeout = Duration::from_secs(30);
        let statefulsets: Api<StatefulSet> = Api::namespaced(kube_client.clone(), &namespace);
        let statefulset: StatefulSet = get_simple_statefulset();
        let app_name = statefulset.metadata.name.clone().unwrap_or_default();
        let selector = format!("app={}", app_name);

        // create simple deployment and wait for it to be ready
        let _ns = NamespaceForTest::new(kube_client.clone(), namespace.to_string()).await?;

        statefulsets.create(&PostParams::default(), &statefulset).await.unwrap();
        tokio::time::timeout(
            timeout,
            await_condition(statefulsets.clone(), &app_name, has_statefulset_ready_replicas(1)),
        )
        .await??;

        // Scaling a service that does not exist should not fail
        tokio::time::timeout(timeout, scale_service(&kube_client, &namespace, "app=totototo", 0, true)).await??;

        // Try to scale down our deployment
        tokio::time::timeout(timeout, scale_service(&kube_client, &namespace, &selector, 0, true)).await??;
        tokio::time::timeout(
            timeout,
            await_condition(statefulsets.clone(), &app_name, has_statefulset_ready_replicas(0)),
        )
        .await??;

        // Try to scale up our deployment
        tokio::time::timeout(timeout, scale_service(&kube_client, &namespace, &selector, 1, true)).await??;
        tokio::time::timeout(
            timeout,
            await_condition(statefulsets.clone(), &app_name, has_statefulset_ready_replicas(1)),
        )
        .await??;

        Ok(())
    }
}
