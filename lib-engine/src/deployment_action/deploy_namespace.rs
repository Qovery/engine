use crate::cloud_provider::kubernetes::{
    kube_copy_secret_to_another_namespace, kube_create_namespace_if_not_exists, kube_does_secret_exists, Kind,
};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::Namespace;
use kube::api::DeleteParams;
use kube::Api;
use std::collections::BTreeMap;
use std::time::Duration;

pub struct NamespaceDeployment {
    pub resource_expiration: Option<Duration>,
    pub event_details: EventDetails,
}

impl DeploymentAction for NamespaceDeployment {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let mut namespace_labels: BTreeMap<String, String> = BTreeMap::from([
            ("qovery.com/environment-id".to_string(), target.environment.long_id.to_string()),
            (
                "qovery.com/project-id".to_string(),
                target.environment.project_long_id.to_string(),
            ),
        ]);

        if let Some(resource_expiration) = &self.resource_expiration {
            namespace_labels.insert("ttl".to_string(), format!("{}", resource_expiration.as_secs()));
        };

        // create a namespace with labels if it does not exist
        block_on(kube_create_namespace_if_not_exists(
            &target.kube,
            target.environment.namespace(),
            namespace_labels,
        ))
        .map_err(|e| {
            EngineError::new_k8s_create_namespace(
                self.event_details.clone(),
                target.environment.namespace().to_string(),
                CommandError::new(
                    format!("Can't create namespace {}", target.environment.namespace()),
                    Some(e.to_string()),
                    None,
                ),
            )
        })?;

        // upmc-enterprises/registry-creds sometimes is too long to copy the secret to the namespace
        // this workaround speed up the process to avoid application fails with ImagePullError on the first deployment
        if target.kubernetes.kind() == Kind::Ec2 {
            let from_namespace = "default";
            match block_on(kube_does_secret_exists(&target.kube, "awsecr-cred", "default")) {
                Ok(x) if x => {
                    block_on(kube_copy_secret_to_another_namespace(
                        &target.kube,
                        "awsecr-cred",
                        from_namespace,
                        target.environment.namespace(),
                    ))
                    .map_err(|e| {
                        EngineError::new_copy_secrets_to_another_namespace_error(
                            self.event_details.clone(),
                            e,
                            from_namespace,
                            target.environment.namespace(),
                        )
                    })?;
                }
                _ => {}
            };
        };

        Ok(())
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        block_on(async {
            let api: Api<Namespace> = Api::all(target.kube.clone());
            if api.get(target.environment.namespace()).await.is_ok() {
                // do not catch potential error - to confirm
                let _ = api
                    .delete(target.environment.namespace(), &DeleteParams::foreground())
                    .await;
            }
        });

        Ok(())
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }
}
