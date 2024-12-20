mod delete_kube_apps;
mod deploy_helms;
mod deploy_terraform;
mod eks;
mod gke;
pub(super) mod kubeconfig_helper;
mod kubectl_utils;
mod scaleway;
mod self_managed;
mod utils;

use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureDiffType, InfrastructureStep};
use crate::infrastructure::action::utils::mk_logger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::{is_kubernetes_upgrade_required, KubernetesUpgradeStatus};
use crate::logger::Logger;
use tera::Context as TeraContext;

pub trait InfrastructureAction: Send + Sync {
    /// Will be called only if it is the first time the cluster is created.
    /// Otherwise, it will be skipped and the `create_cluster` method will be called directly.
    fn bootstap_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Ok(())
    }
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>>;
    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>>;

    fn run(&self, infra_ctx: &InfrastructureContext, action: Action) -> Result<(), Box<EngineError>> {
        let step = match action {
            Action::Create => InfrastructureStep::Create,
            Action::Pause => InfrastructureStep::Pause,
            Action::Delete => InfrastructureStep::Delete,
            Action::Restart => InfrastructureStep::RestartedError,
        };
        let logger = mk_logger(infra_ctx.kubernetes(), step);
        if infra_ctx.context().is_dry_run_deploy() {
            logger.warn("👻 Dry run mode is enabled. No changes will be made to the infrastructure");
        }

        logger.info(format!(
            "{} {} cluster {}",
            action,
            infra_ctx.kubernetes().kind(),
            infra_ctx.kubernetes().name()
        ));
        match action {
            Action::Create => {
                let mut cluster_has_been_upgraded = false;
                if infra_ctx.context().is_first_cluster_deployment() {
                    self.bootstap_cluster(infra_ctx)?;
                } else if let Some(upgrade_status) = self.is_upgrade_required(infra_ctx) {
                    cluster_has_been_upgraded = true;
                    self.upgrade_cluster(infra_ctx, upgrade_status)?;
                }
                self.create_cluster(infra_ctx, cluster_has_been_upgraded)
            }
            Action::Pause => self.pause_cluster(infra_ctx),
            Action::Delete => self.delete_cluster(infra_ctx),
            Action::Restart => Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
                infra_ctx
                    .kubernetes()
                    .get_event_details(Infrastructure(InfrastructureStep::RestartedError)),
            ))),
        }
    }

    // During upgrade check we may want to exclude some node as not pertinent/managed by us
    // I.e: fargate nodes are managed by karpenter, so we don't want to upgrade them
    fn upgrade_node_selector(&self) -> Option<&str> {
        None
    }

    fn is_upgrade_required(&self, infra_ctx: &InfrastructureContext) -> Option<KubernetesUpgradeStatus> {
        if infra_ctx.context().is_first_cluster_deployment() || infra_ctx.context().is_dry_run_deploy() {
            return None;
        }

        let event_details = infra_ctx
            .kubernetes()
            .get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Upgrade);
        match is_kubernetes_upgrade_required(
            infra_ctx.kubernetes().kubeconfig_local_file_path(),
            infra_ctx.kubernetes().version().clone(),
            infra_ctx.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            &logger,
            self.upgrade_node_selector(),
        ) {
            Ok(v) if v.required_upgrade_on.is_some() => Some(v),
            Ok(_) => None,
            Err(e) => {
                logger.warn(EventMessage::new(
                    "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                ));
                None
            }
        }
    }
}

pub trait ToInfraTeraContext {
    fn to_infra_tera_context(&self, target: &InfrastructureContext) -> Result<TeraContext, Box<EngineError>>;
}

pub trait InfraLogger {
    fn info(&self, message: impl Into<EventMessage>);
    fn warn(&self, message: impl Into<EventMessage>);
    fn error(self, error: EngineError, message: Option<impl Into<EventMessage>>);

    fn diff(&self, from: InfrastructureDiffType, message: String);
}

struct InfraLoggerImpl {
    event_details: EventDetails,
    logger: Box<dyn Logger>,
}

impl InfraLogger for InfraLoggerImpl {
    fn info(&self, message: impl Into<EventMessage>) {
        self.logger
            .log(EngineEvent::Info(self.event_details.clone(), message.into()));
    }

    fn warn(&self, message: impl Into<EventMessage>) {
        self.logger
            .log(EngineEvent::Warning(self.event_details.clone(), message.into()));
    }

    fn error(self, error: EngineError, message: Option<impl Into<EventMessage>>) {
        self.logger.log(EngineEvent::Error(error, message.map(|ev| ev.into())));
    }

    fn diff(&self, from: InfrastructureDiffType, message: String) {
        let ev = EventDetails::clone_changing_stage(
            self.event_details.clone(),
            Infrastructure(InfrastructureStep::InfrastructureDiff(from)),
        );
        self.logger.log(EngineEvent::Info(ev, EventMessage::from(message)));
    }
}
