mod delete_kube_apps;
mod deploy_terraform;
mod ec2_k3s;
mod eks;
mod gke;
mod scaleway;
mod self_managed;
mod utils;

use crate::cloud_provider::kubernetes::KubernetesUpgradeStatus;
use crate::cloud_provider::service::Action;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep};
use crate::infrastructure_action::utils::mk_logger;
use crate::logger::Logger;
use tera::Context as TeraContext;

pub trait InfrastructureAction: Send + Sync {
    fn create_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
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
            logger.warn("Dry run mode is enabled. No changes will be made to the infrastructure");
        }

        logger.info(format!(
            "{} {} cluster {}",
            action,
            infra_ctx.kubernetes().kind(),
            infra_ctx.kubernetes().name()
        ));
        match action {
            Action::Create => self.create_cluster(infra_ctx),
            Action::Pause => self.pause_cluster(infra_ctx),
            Action::Delete => self.delete_cluster(infra_ctx),
            Action::Restart => Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
                infra_ctx
                    .kubernetes()
                    .get_event_details(Infrastructure(InfrastructureStep::RestartedError)),
            ))),
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
}
