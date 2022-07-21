use crate::cloud_provider::service::prepare_namespace;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
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
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let mut namespace_labels: Option<BTreeMap<String, String>> = None;
        if let Some(resource_expiration) = &self.resource_expiration {
            namespace_labels = Some(BTreeMap::from([(
                "ttl".to_string(),
                format!("{}", resource_expiration.as_secs()),
            )]));
        };

        prepare_namespace(
            target.environment,
            namespace_labels,
            self.event_details.clone(),
            target.kubernetes.kind(),
            &target.kube,
        )
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
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
}
