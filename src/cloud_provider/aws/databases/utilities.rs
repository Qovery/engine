use tokio::io::Error;

use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cmd::kubectl::{kubectl_exec_create_namespace, kubectl_exec_delete_secret};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::error::SimpleError;
use tracing::{event, span, Level};
// generate the kubernetes config path
pub fn get_kubernetes_config_path(
    workspace: &str,
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
) -> Result<String, SimpleError> {
    let aws = kubernetes
        .cloud_provider()
        .as_any()
        .downcast_ref::<AWS>()
        .unwrap();

    common::kubernetes_config_path(
        workspace,
        environment.organization_id.as_str(),
        kubernetes.id(),
        aws.access_key_id.as_str(),
        aws.secret_access_key.as_str(),
        kubernetes.region(),
    )
}

pub fn create_namespace(namespace: &str, kube_config: &str, aws: &AWS) {
    let aws_credentials_envs = vec![
        (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
        (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
    ];
    kubectl_exec_create_namespace(kube_config, namespace, aws_credentials_envs);
}

pub fn delete_terraform_tfstate_secret(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    workspace_dir: &str,
) -> Result<(), SimpleError> {
    let aws = kubernetes
        .cloud_provider()
        .as_any()
        .downcast_ref::<AWS>()
        .unwrap();
    let aws_credentials_envs = vec![
        (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
        (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
    ];

    let kubernetes_config_file_path = common::kubernetes_config_path(
        workspace_dir,
        environment.organization_id.as_str(),
        kubernetes.id(),
        aws.access_key_id.as_str(),
        aws.secret_access_key.as_str(),
        kubernetes.region(),
    );

    match kubernetes_config_file_path {
        Ok(kube_config) => {
            //create the namespace to insert the tfstate in secrets
            kubectl_exec_delete_secret(kube_config, "tfstate-default-state", aws_credentials_envs);
            Ok(())
        }
        Err(e) => {
            event!(
                Level::ERROR,
                "Failed to generate the kubernetes config file path: {:?}",
                e
            );
            Err(e)
        }
    }
}
