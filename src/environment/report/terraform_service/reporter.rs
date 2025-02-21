use crate::environment::models::terraform_service::TerraformServiceTrait;
use crate::environment::report::DeploymentReporter;
use crate::environment::report::logger::EnvLogger;
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::utilities::to_short_id;
use std::sync::Arc;
use uuid::Uuid;

pub struct TerraformServiceDeploymentReporter {
    long_id: Uuid,
    logger: EnvLogger,
    metrics_registry: Arc<dyn MetricsRegistry>,
    action: Action,
}

impl TerraformServiceDeploymentReporter {
    pub fn new(chart: &impl TerraformServiceTrait, deployment_target: &DeploymentTarget, action: Action) -> Self {
        Self {
            long_id: *chart.long_id(),
            logger: deployment_target.env_logger(chart, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            action,
        }
    }
}

impl DeploymentReporter for TerraformServiceDeploymentReporter {
    type DeploymentResult = ();
    type DeploymentState = ();
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&self) -> Self::DeploymentState {}

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        self.metrics_registry
            .start_record(self.long_id, StepLabel::Service, StepName::Deployment);
        self.logger.send_progress(format!(
            "🚀 {} of terraform service `{}` is starting",
            self.action,
            to_short_id(&self.long_id)
        ));
    }

    fn deployment_in_progress(&self, _: &mut Self::DeploymentState) {
        // TODO TF check if we need to add log here
        // We use the output of terraform directly
    }

    fn deployment_terminated(
        &self,
        result: &Result<Self::DeploymentResult, Box<EngineError>>,
        _: &mut Self::DeploymentState,
    ) {
        let error = match result {
            Ok(_) => {
                self.stop_record(StepStatus::Success);
                self.logger
                    .send_success(format!("✅ {} of terraform service succeeded", self.action));
                return;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            self.stop_record(StepStatus::Cancel);
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(
                    r#"
                🚫 {} has been cancelled.
                "#,
                    self.action
                )
                .trim()
                .to_string(),
                None,
            ));
            return;
        }
        self.stop_record(StepStatus::Error);
        self.logger.send_error(*error.clone());
        self.logger.send_error(EngineError::new_engine_error(
            *error.clone(),
            format!("
❌ {} of terraform service failed !
⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️
Look at the Deployment Status Reports above and use our troubleshooting guide to fix it https://hub.qovery.com/docs/using-qovery/troubleshoot/
⛑ Can't solve the issue? Please have a look at our forum https://discuss.qovery.com/
⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️
                ", self.action),
            None,
        ));
    }
}

impl TerraformServiceDeploymentReporter {
    pub(crate) fn stop_record(&self, step_status: StepStatus) {
        self.metrics_registry
            .stop_record(self.long_id, StepName::Deployment, step_status.clone());
        self.metrics_registry
            .stop_record(self.long_id, StepName::Total, step_status);
    }
}
