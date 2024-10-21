use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cmd::kubectl::{kubectl_delete_completed_jobs, kubectl_exec_delete_pod, kubectl_get_crash_looping_pods};
use crate::errors::EngineError;
use crate::events::Stage;
use serde::de::DeserializeOwned;

pub fn from_terraform_value<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::de::Deserializer<'de>,
    T: DeserializeOwned,
{
    use serde::Deserialize;

    #[derive(serde_derive::Deserialize)]
    struct TerraformJsonValue<T> {
        value: T,
    }

    TerraformJsonValue::deserialize(deserializer).map(|o: TerraformJsonValue<T>| o.value)
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
                    kube.kubeconfig_local_file_path(),
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
