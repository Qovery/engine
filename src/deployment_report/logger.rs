use crate::cloud_provider::service::{Action, Service};
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::logger::Logger;
use std::sync::Arc;

pub struct Loggers {
    pub send_progress: Box<dyn Fn(String) + Send>,
    pub send_success: Box<dyn Fn(String) + Send>,
    pub send_error: Box<dyn Fn(EngineError) + Send>,
}

// All that for the logger, lol ...
pub fn get_loggers<Srv>(service: &Srv, action: Action, logger: &dyn Logger) -> Loggers
where
    Srv: Service,
{
    let logger = Arc::new(logger.clone_dyn());
    let log_progress = {
        let event_details = service.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        let logger = logger.clone();
        let step = match action {
            Action::Create => EnvironmentStep::Deploy,
            Action::Pause => EnvironmentStep::Pause,
            Action::Delete => EnvironmentStep::Delete,
            Action::Nothing => EnvironmentStep::Deploy, // should not hserviceen
        };
        let event_details = EventDetails::clone_changing_stage(event_details, Stage::Environment(step));

        move |msg: String| {
            logger.log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));
        }
    };

    let log_success = {
        let event_details = service.get_event_details(Stage::Environment(EnvironmentStep::Deployed));
        let logger = logger.clone();
        let step = match action {
            Action::Create => EnvironmentStep::Deployed,
            Action::Pause => EnvironmentStep::Paused,
            Action::Delete => EnvironmentStep::Deleted,
            Action::Nothing => EnvironmentStep::Deployed, // should not happens
        };
        let event_details = EventDetails::clone_changing_stage(event_details, Stage::Environment(step));

        move |msg: String| {
            logger.log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));
        }
    };

    let log_error = {
        let logger = logger.clone();
        move |err: EngineError| {
            let msg = err.user_log_message().to_string();
            logger.log(EngineEvent::Error(err, Some(EventMessage::new_from_safe(msg))));
        }
    };

    Loggers {
        send_progress: Box::new(log_progress),
        send_success: Box::new(log_success),
        send_error: Box::new(log_error),
    }
}
