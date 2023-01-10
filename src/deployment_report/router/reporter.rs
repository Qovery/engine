use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::logger::EnvLogger;
use crate::deployment_report::DeploymentReporter;
use crate::errors::EngineError;
use crate::models::router::RouterService;
use crate::utilities::to_short_id;

use uuid::Uuid;

pub struct RouterDeploymentReporter {
    long_id: Uuid,
    logger: EnvLogger,
    action: Action,
}

impl RouterDeploymentReporter {
    pub fn new(router: &impl RouterService, deployment_target: &DeploymentTarget, action: Action) -> Self {
        RouterDeploymentReporter {
            long_id: *router.long_id(),
            logger: deployment_target.env_logger(router, action.to_environment_step()),
            action,
        }
    }
}

impl DeploymentReporter for RouterDeploymentReporter {
    type DeploymentResult = ();
    type DeploymentState = ();
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&self) -> Self::DeploymentState {}

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        self.logger.send_progress(format!(
            "🚀 {} of router `{}` is starting",
            self.action,
            to_short_id(&self.long_id)
        ));
    }

    fn deployment_in_progress(&self, _: &mut Self::DeploymentState) {
        self.logger
            .send_progress(format!("⌛️ {} of router in progress ...", self.action));
    }

    fn deployment_terminated(
        &self,
        result: &Result<Self::DeploymentResult, Box<EngineError>>,
        _: &mut Self::DeploymentState,
    ) {
        let error = match result {
            Ok(_) => {
                self.logger
                    .send_success(format!("✅ {} of router succeeded", self.action));
                return;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(
                    r#"
                🚫 {} has been cancelled. Router has been rollback to previous version if rollout was on-going
                "#,
                    self.action
                )
                .trim()
                .to_string(),
                None,
            ));
            return;
        }
        self.logger.send_error(*error.clone());
        self.logger.send_error(EngineError::new_engine_error(
            *error.clone(),
            format!("
❌ {} of router failed ! Look at the report above and to understand why.
⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
                ", self.action),
            None,
        ));
    }
}
