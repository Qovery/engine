use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cmd::kubectl::{kubectl_exec_create_namespace, kubectl_exec_delete_secret};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope, SimpleError, StringError};
use std::collections::HashMap;

// unfortunately some proposed versions are not SemVer like Elasticache (6.x)
// this is why we need ot have our own structure
pub struct VersionsNumber {
    major: String,
    minor: Option<String>,
    patch: Option<String>,
}

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
            error!(
                "Failed to generate the kubernetes config file path: {:?}",
                e
            );
            Err(e)
        }
    }
}

pub fn get_version_semver(version: &str) -> Result<VersionsNumber, StringError> {
    let mut version_split = version.split(".");

    let major = match version_split.next() {
        Some(major) => major.to_string(),
        _ => {
            return Err(StringError::new(
                "please check the version you've sent, it can't be checked".to_string(),
            ))
        }
    };

    let minor = match version_split.next() {
        Some(minor) => Some(minor.to_string()),
        _ => None,
    };

    let patch = match version_split.next() {
        Some(patch) => Some(patch.to_string()),
        _ => None,
    };

    Ok(VersionsNumber {
        major,
        minor,
        patch,
    })
}

pub fn get_supported_version_to_use(
    all_supported_versions: HashMap<&str, &str>,
    version_to_check: &str,
) -> Result<String, StringError> {
    let version = match get_version_semver(version_to_check) {
        Ok(version) => version,
        Err(e) => return Err(e),
    };

    match all_supported_versions.get(version.major.as_str()) {
        Some(version) => Ok(version.to_string()),
        None => {
            return Err(StringError::new(format!(
                "this {} version is not supported",
                version_to_check
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::databases::utilities::get_supported_version_to_use;
    use std::collections::HashMap;

    #[test]
    fn check_redis_version() {
        let mut redis_managed_versions = HashMap::with_capacity(1);
        redis_managed_versions.insert("6", "6.x");
        let mut redis_self_hosted_versions = HashMap::with_capacity(1);
        redis_self_hosted_versions.insert("6", "6.0.9-debian-10-r26");

        assert_eq!(
            get_supported_version_to_use(redis_managed_versions.clone(), "6").unwrap(),
            "6.x"
        );
        assert_eq!(
            get_supported_version_to_use(redis_self_hosted_versions.clone(), "6.0.0").unwrap(),
            "6.0.9-debian-10-r26"
        );
        assert_eq!(
            get_supported_version_to_use(redis_managed_versions.clone(), "1")
                .unwrap_err()
                .message
                .as_str(),
            "this 1 version is not supported"
        );
    }
}
