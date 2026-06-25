mod cluster_create;
mod cluster_delete;
mod cluster_pause;
mod cluster_upgrade;
pub(crate) mod helm_charts;
mod tera_context;

use super::utils::{from_terraform_value, mk_logger};
use crate::environment::models::types::DeployedEngineVersion;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::azure::cluster_create::create_aks_cluster;
use crate::infrastructure::action::azure::cluster_delete::delete_aks_cluster;
use crate::infrastructure::action::azure::cluster_pause::pause_aks_cluster;
use crate::infrastructure::action::azure::cluster_upgrade::upgrade_aks_cluster;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::azure::aks::AKS;
use crate::infrastructure::models::kubernetes::{KubernetesUpgradeStatus, send_progress_on_long_task};
use serde_derive::{Deserialize, Serialize};

impl InfrastructureAction for AKS {
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        _has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);
        send_progress_on_long_task(self, Action::Create, || create_aks_cluster(self, infra_ctx, logger))
    }

    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Pause);
        send_progress_on_long_task(self, Action::Pause, || pause_aks_cluster(self, infra_ctx, logger))
    }

    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Delete);
        send_progress_on_long_task(self, Action::Delete, || delete_aks_cluster(self, infra_ctx, logger))
    }

    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Upgrade);

        send_progress_on_long_task(self, Action::Create, || {
            upgrade_aks_cluster(self, infra_ctx, kubernetes_upgrade_status, logger)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AksQoveryTerraformOutput {
    #[serde(deserialize_with = "from_terraform_value")]
    pub aks_cluster_public_hostname: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub main_storage_account_name: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub main_storage_account_primary_access_key: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub loki_logging_service_msi_client_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub kubeconfig: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_name: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_oidc_issuer: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub thanos_client_id: Option<String>,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub thanos_storage_account: Option<String>,
    #[serde(deserialize_with = "super::utils::from_terraform_optional_deployed_engine_version")]
    #[serde(default)]
    pub qovery_deployed_with_engine_version: Option<DeployedEngineVersion>,
}

#[cfg(test)]
mod tests {
    use super::AksQoveryTerraformOutput;

    // `terraform output -json` wraps each output as `{ "value": ... }`.
    fn tf_value(v: &str) -> String {
        format!(r#"{{"value":"{v}"}}"#)
    }

    #[test]
    fn full_terraform_output_deserializes() {
        let json = format!(
            r#"{{
                "aks_cluster_public_hostname": {host},
                "main_storage_account_name": {sa},
                "main_storage_account_primary_access_key": {key},
                "loki_logging_service_msi_client_id": {loki},
                "kubeconfig": {kubeconfig},
                "cluster_name": {name},
                "cluster_id": {id},
                "cluster_oidc_issuer": {oidc}
            }}"#,
            host = tf_value("host.example.com"),
            sa = tf_value("qoverystorage"),
            key = tf_value("secret-key"),
            loki = tf_value("loki-msi"),
            kubeconfig = tf_value("apiVersion: v1"),
            name = tf_value("qovery-zabcd1234"),
            id = tf_value("abcd1234"),
            oidc = tf_value("https://oidc.example.com"),
        );

        let output: AksQoveryTerraformOutput =
            serde_json::from_str(&json).expect("full terraform output should deserialize");
        assert_eq!(output.main_storage_account_name, "qoverystorage");
        assert_eq!(output.main_storage_account_primary_access_key, "secret-key");
    }

    // Regression (QOV-2045): once the cluster is deleted, `terraform output` no longer carries the
    // required outputs, so deserialization fails. The delete path must treat this as "no outputs"
    // (skip in-cluster cleanup) and still run destroy — see `TerraformInfraResources::create_or_read_output`.
    // If every field were `#[serde(default)]`, this would silently succeed and object-storage cleanup
    // would run with empty credentials.
    #[test]
    fn empty_terraform_output_is_not_deserializable() {
        let result: Result<AksQoveryTerraformOutput, _> = serde_json::from_str("{}");
        assert!(result.is_err(), "empty terraform output must not deserialize");
    }
}
