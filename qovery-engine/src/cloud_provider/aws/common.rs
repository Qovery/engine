use std::io::Error;
use std::str::FromStr;

use rusoto_core::Region;

use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::ServiceError;
use crate::cmd::utilities::CmdError;
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};

pub fn kubernetes_config_path(
    workspace_directory: &str,
    organization_id: &str,
    kubernetes_cluster_id: &str,
    access_key_id: &str,
    secret_access_key: &str,
    region: &str,
) -> Result<String, Error> {
    let kubernetes_config_bucket_name = format!("qovery-kubeconfigs-{}", kubernetes_cluster_id);
    let kubernetes_config_object_key = format!("{}.yaml", kubernetes_cluster_id);

    let kubernetes_config_file_path = format!(
        "{}/kubernetes_config_{}",
        workspace_directory, kubernetes_cluster_id
    );

    let _region = Region::from_str(region).unwrap();

    let _ = crate::s3::get_kubernetes_config_file(
        access_key_id,
        secret_access_key,
        &_region,
        kubernetes_config_bucket_name.as_str(),
        kubernetes_config_object_key.as_str(),
        kubernetes_config_file_path.as_str(),
    )?;

    Ok(kubernetes_config_file_path)
}

pub type Logs = String;
pub type Describe = String;

/// show different output (kubectl describe, log..) for debug purpose
pub fn get_stateless_resource_information(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    workspace_dir: &str,
    selector: &str,
) -> Result<(Describe, Logs), CmdError> {
    let aws = kubernetes
        .cloud_provider()
        .as_any()
        .downcast_ref::<AWS>()
        .unwrap();

    let kubernetes_config_file_path = kubernetes_config_path(
        workspace_dir,
        environment.organization_id.as_str(),
        kubernetes.id(),
        aws.access_key_id.as_str(),
        aws.secret_access_key.as_str(),
        kubernetes.region(),
    )?;

    let aws_credentials_envs = vec![
        (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
        (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
    ];

    // exec describe pod...
    let describe = match crate::cmd::kubectl::kubectl_exec_describe_pod(
        kubernetes_config_file_path.as_str(),
        environment.namespace(),
        selector,
        aws_credentials_envs.clone(),
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
    let logs = match crate::cmd::kubectl::kubectl_exec_logs(
        kubernetes_config_file_path.as_str(),
        environment.namespace(),
        selector,
        aws_credentials_envs.clone(),
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

    Ok((describe, logs))
}

pub fn do_stateless_service_cleanup(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    workspace_dir: &str,
    helm_release_name: &str,
) -> Result<(), ServiceError> {
    let aws = kubernetes
        .cloud_provider()
        .as_any()
        .downcast_ref::<AWS>()
        .unwrap();

    let kubernetes_config_file_path = kubernetes_config_path(
        workspace_dir,
        environment.organization_id.as_str(),
        kubernetes.id(),
        aws.access_key_id.as_str(),
        aws.secret_access_key.as_str(),
        kubernetes.region(),
    )?;

    let aws_credentials_envs = vec![
        (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
        (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
    ];

    let history_rows = crate::cmd::helm::helm_exec_history(
        kubernetes_config_file_path.as_str(),
        environment.namespace(),
        helm_release_name,
        aws_credentials_envs.clone(),
    )?;

    // if there is no valid history - then delete the helm chart
    let first_valid_history_row = history_rows.iter().find(|x| x.is_successfully_deployed());

    if first_valid_history_row.is_some() {
        crate::cmd::helm::helm_exec_uninstall(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name,
            aws_credentials_envs,
        )?;
    }

    Ok(())
}
