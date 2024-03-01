use crate::cloud_provider::kubernetes::{
    check_master_version_status, check_workers_pause, check_workers_status, check_workers_upgrade_status,
    send_progress_on_long_task, Kubernetes, KubernetesVersion,
};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::CloudProvider;
use crate::cmd::kubectl::{kubectl_delete_completed_jobs, kubectl_exec_delete_pod, kubectl_get_crash_looping_pods};
use crate::errors::{CommandError, EngineError};
use crate::events::Stage;

pub fn check_workers_on_upgrade(
    kube: &dyn Kubernetes,
    cloud_provider: &dyn CloudProvider,
    targeted_version: String,
) -> Result<(), CommandError> {
    send_progress_on_long_task(kube, Action::Create, || {
        check_workers_upgrade_status(
            kube.kubeconfig_local_file_path(),
            cloud_provider.credentials_environment_variables(),
            targeted_version.clone(),
        )
    })
}

pub fn check_control_plane_on_upgrade(
    kube: &dyn Kubernetes,
    cloud_provider: &dyn CloudProvider,
    targeted_version: KubernetesVersion,
) -> Result<(), CommandError> {
    send_progress_on_long_task(kube, Action::Create, || {
        check_master_version_status(
            kube.kubeconfig_local_file_path(),
            cloud_provider.credentials_environment_variables(),
            &targeted_version,
        )
    })
}

pub fn check_workers_on_create(kube: &dyn Kubernetes, cloud_provider: &dyn CloudProvider) -> Result<(), CommandError> {
    send_progress_on_long_task(kube, Action::Create, || {
        check_workers_status(
            kube.kubeconfig_local_file_path(),
            cloud_provider.credentials_environment_variables(),
        )
    })
}

pub fn check_workers_on_pause(kube: &dyn Kubernetes, cloud_provider: &dyn CloudProvider) -> Result<(), CommandError> {
    send_progress_on_long_task(kube, Action::Create, || {
        check_workers_pause(
            kube.kubeconfig_local_file_path(),
            cloud_provider.credentials_environment_variables(),
        )
    })
}

pub fn delete_crashlooping_pods(
    kube: &dyn Kubernetes,
    namespace: Option<&str>,
    selector: Option<&str>,
    restarted_min_count: Option<usize>,
    envs: Vec<(&str, &str)>,
    stage: Stage,
) -> Result<(), Box<EngineError>> {
    let event_details = kube.get_event_details(stage);

    match kubectl_get_crash_looping_pods(
        kube.kubeconfig_local_file_path(),
        namespace,
        selector,
        restarted_min_count,
        envs.clone(),
    ) {
        Ok(pods) => {
            for pod in pods {
                if let Err(e) = kubectl_exec_delete_pod(
                    &kube.kubeconfig_local_file_path(),
                    pod.metadata.namespace.as_str(),
                    pod.metadata.name.as_str(),
                    envs.clone(),
                ) {
                    return Err(Box::new(EngineError::new_k8s_cannot_delete_pod(
                        event_details,
                        pod.metadata.name.to_string(),
                        e,
                    )));
                }
            }
        }
        Err(e) => {
            return Err(Box::new(EngineError::new_k8s_cannot_get_crash_looping_pods(event_details, e)));
        }
    };

    Ok(())
}

pub fn delete_completed_jobs(
    kube: &dyn Kubernetes,
    envs: Vec<(&str, &str)>,
    stage: Stage,
    ignored_namespaces: Option<Vec<&str>>,
) -> Result<(), Box<EngineError>> {
    let event_details = kube.get_event_details(stage);

    if let Err(e) = kubectl_delete_completed_jobs(kube.kubeconfig_local_file_path(), envs, ignored_namespaces) {
        return Err(Box::new(EngineError::new_k8s_cannot_delete_completed_jobs(event_details, e)));
    };

    Ok(())
}
