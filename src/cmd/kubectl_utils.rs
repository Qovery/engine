use crate::cmd::kubectl::kubectl_exec_get_pods;
use crate::cmd::structs::KubernetesPodStatusPhase;
use crate::errors::CommandError;
use retry::delay::Fixed;
use retry::OperationResult;
use std::path::Path;
use std::time::Duration;

pub fn kubectl_are_qovery_infra_pods_executed<P>(
    kubernetes_config: P,
    envs: &[(String, String)],
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let result = retry::retry(Fixed::from(Duration::from_secs(10)).take(60), || {
        match kubectl_exec_get_pods(
            &kubernetes_config,
            None,
            None,
            envs.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect(),
        ) {
            Ok(res) => {
                for pod in res.items {
                    if !pod.metadata.namespace.starts_with('z') && pod.status.phase == KubernetesPodStatusPhase::Pending
                    {
                        return OperationResult::Retry(CommandError::new_from_safe_message(
                            "Pods didn't restart yet. Waiting...".to_string(),
                        ));
                    };
                }
                OperationResult::Ok(())
            }
            Err(e) => OperationResult::Retry(e),
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(retry::Error { error, .. }) => Err(error),
    }
}
