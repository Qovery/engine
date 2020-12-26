use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::Service;
use crate::error::{cast_simple_error_to_engine_error, EngineError};

pub type Logs = String;
pub type Describe = String;

/// return debug information line by line to help the user to understand what's going on,
/// and why its app does not start
pub fn get_stateless_resource_information_for_user<T>(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    service: &T,
) -> Result<Vec<String>, EngineError>
where
    T: Service + ?Sized,
{
    let selector = format!("app={}", service.name());

    let kubernetes_config_file_path = kubernetes.config_file_path()?;

    let mut result = Vec::with_capacity(50);

    // get logs
    let logs = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_logs(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector.as_str(),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    )?;

    let _ = result.extend(logs);

    // get pod state
    let pods = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_get_pod(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector.as_str(),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    )?;

    for pod in pods.items {
        for container_status in pod.status.container_statuses {
            if let Some(last_state) = container_status.last_state {
                if let Some(terminated) = last_state.terminated {
                    if let Some(message) = terminated.message {
                        result.push(format!("terminated state message: {}", message));
                    }

                    result.push(format!(
                        "terminated state exit code: {}",
                        terminated.exit_code
                    ));
                }

                if let Some(waiting) = last_state.waiting {
                    if let Some(message) = waiting.message {
                        result.push(format!("waiting state message: {}", message));
                    }
                }
            }
        }
    }

    // get pod events
    let events = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_get_event(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
            "involvedObject.kind=Pod",
        ),
    )?;

    let pod_name_start = format!("{}-", service.name());
    for event in events.items {
        if event.type_.to_lowercase() != "normal"
            && event.involved_object.name.starts_with(&pod_name_start)
        {
            if let Some(message) = event.message {
                result.push(format!("{}: {}", event.type_, message));
            }
        }
    }

    Ok(result)
}

/// show different output (kubectl describe, log..) for debug purpose
pub fn get_stateless_resource_information(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    selector: &str,
) -> Result<(Describe, Logs), EngineError> {
    let kubernetes_config_file_path = kubernetes.config_file_path()?;

    // exec describe pod...
    let describe = match cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_describe_pod(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector,
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    ) {
        Ok(output) => {
            info!("{}", output);
            output
        }
        Err(err) => {
            error!("{:?}", err);
            return Err(err);
        }
    };

    // exec logs...
    let logs = match cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_logs(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector,
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    ) {
        Ok(output) => {
            info!("{:?}", output);
            output.join("\n")
        }
        Err(err) => {
            error!("{:?}", err);
            return Err(err);
        }
    };

    Ok((describe, logs))
}

pub fn do_stateless_service_cleanup(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    helm_release_name: &str,
) -> Result<(), EngineError> {
    let kubernetes_config_file_path = kubernetes.config_file_path()?;

    let history_rows = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::helm::helm_exec_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name,
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    )?;

    // if there is no valid history - then delete the helm chart
    let first_valid_history_row = history_rows.iter().find(|x| x.is_successfully_deployed());

    if first_valid_history_row.is_some() {
        cast_simple_error_to_engine_error(
            kubernetes.engine_error_scope(),
            kubernetes.context().execution_id(),
            crate::cmd::helm::helm_exec_uninstall(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name,
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables(),
            ),
        )?;
    }

    Ok(())
}
