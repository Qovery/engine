mod capi_backup;
mod cluster_config_git;
mod cluster_create;
mod cluster_install;
mod eksctl;
mod etcd_backup;
mod helm_charts;
mod provider;

use crate::environment::models::types::VersionsNumber;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::eksanywhere::cluster_create::create_eks_anywhere_cluster;
use crate::infrastructure::action::utils::mk_logger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhere;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesUpgradeStatus, send_progress_on_long_task};

impl InfrastructureAction for EksAnywhere {
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        _has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);

        send_progress_on_long_task(self, Action::Create, || create_eks_anywhere_cluster(self, infra_ctx, logger))
    }

    fn pause_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::PauseError)),
        )))
    }

    fn delete_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::DeleteError)),
        )))
    }

    fn upgrade_cluster(
        &self,
        _infra_ctx: &InfrastructureContext,
        _kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::UpgradeError)),
        )))
    }

    fn is_upgrade_required(&self, _infra_ctx: &InfrastructureContext) -> Option<KubernetesUpgradeStatus> {
        // EKS Anywhere lifecycle is driven by the cluster config and `eksctl anywhere` flows.
        // The generic Kubernetes version drift check would otherwise turn a create into an
        // unsupported cluster restart/upgrade path for this provider.
        None
    }

    fn post_create_deprecated_api_target_version(&self, _infra_ctx: &InfrastructureContext) -> Option<VersionsNumber> {
        // EKS Anywhere already runs a dedicated Pluto check based on the parsed
        // `eksctl anywhere upgrade plan` target version.
        // Skip the generic post-create Pluto check to avoid duplicate/conflicting logs.
        None
    }

    fn should_log_post_create_deprecated_api_check_skip(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EksAnywhereRunMode {
    DryRun,
    Apply,
}

impl EksAnywhereRunMode {
    pub(super) fn from_context(infra_ctx: &InfrastructureContext) -> Self {
        if infra_ctx.context().is_dry_run_deploy() {
            Self::DryRun
        } else {
            Self::Apply
        }
    }

    pub(super) fn install_missing_templates(self) -> bool {
        matches!(self, Self::Apply)
    }

    pub(super) fn should_execute_upgrade_cluster(self) -> bool {
        matches!(self, Self::Apply)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::DryRun => "dry-run",
            Self::Apply => "apply",
        }
    }
}
