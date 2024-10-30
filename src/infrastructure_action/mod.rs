mod deploy_terraform;
mod ec2_k3s;
mod eks;
mod gke;
mod scaleway;
mod self_managed;
mod utils;

use crate::cloud_provider::kubernetes::KubernetesUpgradeStatus;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EventDetails, EventMessage};
use tera::Context as TeraContext;

// TODO: Remove pub export if possible
use crate::logger::Logger;
pub use ec2_k3s::AwsEc2QoveryTerraformOutput;

pub trait InfrastructureAction: Send + Sync {
    fn create_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>>;
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
