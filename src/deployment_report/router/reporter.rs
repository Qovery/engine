use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::logger::{get_loggers, Loggers};
use crate::deployment_report::DeploymentReporter;
use crate::errors::EngineError;
use crate::models::router::RouterService;
use crate::utilities::to_short_id;
use std::borrow::Borrow;
use uuid::Uuid;

pub struct RouterDeploymentReporter {
    long_id: Uuid,
    send_progress: Box<dyn Fn(String) + Send>,
    send_success: Box<dyn Fn(String) + Send>,
    send_error: Box<dyn Fn(EngineError) + Send>,
}

impl RouterDeploymentReporter {
    pub fn new(router: &impl RouterService, deployment_target: &DeploymentTarget, action: Action) -> Self {
        let Loggers {
            send_progress,
            send_success,
            send_error,
        } = get_loggers(router, action, deployment_target.logger.borrow());

        RouterDeploymentReporter {
            long_id: *router.long_id(),
            send_progress,
            send_success,
            send_error,
        }
    }
}

impl DeploymentReporter for RouterDeploymentReporter {
    type DeploymentResult = Result<(), EngineError>;

    fn before_deployment_start(&mut self) {
        (self.send_progress)(format!("🚀 Deployment of router `{}` is starting", to_short_id(&self.long_id)));
    }

    fn deployment_in_progress(&mut self) {
        (self.send_progress)("⌛️ Deployment of router in progress ...".to_string());
    }

    fn deployment_terminated(&mut self, result: &Self::DeploymentResult) {
        let error = match result {
            Ok(_) => {
                (self.send_success)("✅ Deployment of router succeeded".to_string());
                return;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            (self.send_error)(EngineError::new_engine_error(
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
        (self.send_error)(error.clone());
        (self.send_error)(EngineError::new_engine_error(
            error.clone(),
            r#"
❌ Deployment of router failed ! Look at the report above and to understand why.
⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
                "#.trim().to_string(),
            None,
        ));
    }
}
