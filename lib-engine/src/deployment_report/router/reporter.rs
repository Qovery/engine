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
}

impl RouterDeploymentReporter {
    pub fn new(router: &impl RouterService, deployment_target: &DeploymentTarget, action: Action) -> Self {
        RouterDeploymentReporter {
            long_id: *router.long_id(),
            logger: deployment_target.env_logger(router, action.to_environment_step()),
        }
    }
}

impl DeploymentReporter for RouterDeploymentReporter {
    type DeploymentResult = Result<(), EngineError>;

    fn before_deployment_start(&mut self) {
        self.logger
            .send_progress(format!("🚀 Deployment of router `{}` is starting", to_short_id(&self.long_id)));
    }

    fn deployment_in_progress(&mut self) {
        self.logger
            .send_progress("⌛️ Deployment of router in progress ...".to_string());
    }

    fn deployment_terminated(&mut self, result: &Self::DeploymentResult) {
        let error = match result {
            Ok(_) => {
                self.logger
                    .send_success("✅ Deployment of router succeeded".to_string());
                return;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            self.logger.send_error(EngineError::new_engine_error(
                error.clone(),
                r#"
                🚫 Deployment has been cancelled. Router has been rollback to previous version if rollout was on-going
                "#
                .trim()
                .to_string(),
                None,
            ));
            return;
        }
        self.logger.send_error(error.clone());
        self.logger.send_error(EngineError::new_engine_error(
            error.clone(),
            r#"
❌ Deployment of router failed ! Look at the report above and to understand why.
⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
                "#.trim().to_string(),
            None,
        ));
    }
}
