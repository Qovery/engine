use crate::cloud_provider::service::{Action, RouterService};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::DeploymentReporter;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::io_models::ProgressLevel::Info;
use crate::io_models::{ListenersHelper, ProgressInfo};
use crate::utilities::to_short_id;
use uuid::Uuid;

pub struct RouterDeploymentReporter {
    long_id: Uuid,
    send_progress: Box<dyn Fn(String) + Send>,
}

impl RouterDeploymentReporter {
    pub fn new(router: &impl RouterService, _deployment_target: &DeploymentTarget, action: Action) -> Self {
        // For the logger, lol ...
        let log = {
            let event_details = router.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
            let logger = router.logger().clone_dyn();
            let execution_id = router.context().execution_id().to_string();
            let scope = router.progress_scope();
            let listeners = router.listeners().clone();
            let step = match action {
                Action::Create => EnvironmentStep::Deploy,
                Action::Pause => EnvironmentStep::Pause,
                Action::Delete => EnvironmentStep::Delete,
                Action::Nothing => EnvironmentStep::Deploy, // should not happen
            };
            let event_details = EventDetails::clone_changing_stage(event_details, Stage::Environment(step));

            move |msg: String| {
                let listeners_helper = ListenersHelper::new(&listeners);
                let info = ProgressInfo::new(scope.clone(), Info, Some(msg.clone()), execution_id.clone());
                match action {
                    Action::Create => listeners_helper.deployment_in_progress(info),
                    Action::Pause => listeners_helper.pause_in_progress(info),
                    Action::Delete => listeners_helper.delete_in_progress(info),
                    Action::Nothing => listeners_helper.deployment_in_progress(info),
                };
                logger.log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));
            }
        };

        RouterDeploymentReporter {
            long_id: *router.long_id(),
            send_progress: Box::new(log),
        }
    }
}

impl DeploymentReporter for RouterDeploymentReporter {
    fn before_deployment_start(&self) {
        (self.send_progress)(format!("🚀 Deployment of router `{}` is starting", to_short_id(&self.long_id)));
    }

    fn deployment_in_progress(&self) {
        (self.send_progress)("⌛️ Deployment of router in progress ...".to_string());
    }
}
