use crate::cloud_provider::service::Service;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::logger::Logger;
use std::sync::Arc;

#[cfg(feature = "env-logger-check")]
use std::cell::Cell;

pub struct EnvLogger {
    logger: Arc<Box<dyn Logger>>,
    event_details_progress: EventDetails,
    event_details_success: EventDetails,
    #[cfg(feature = "env-logger-check")]
    state: Cell<LoggerState>,
}

impl EnvLogger {
    pub fn new(service: &(impl Service + ?Sized), step: EnvironmentStep, logger: Arc<Box<dyn Logger>>) -> Self {
        let (progress_step, success_step) = match step {
            EnvironmentStep::Deploy => (EnvironmentStep::Deploy, EnvironmentStep::Deployed),
            EnvironmentStep::Pause => (EnvironmentStep::Pause, EnvironmentStep::Paused),
            EnvironmentStep::Delete => (EnvironmentStep::Delete, EnvironmentStep::Deleted),
            EnvironmentStep::Build => (EnvironmentStep::Build, EnvironmentStep::Built),
            _ => panic!("Invalid environment step for logger"),
        };
        let event_details_progress = service.get_event_details(Stage::Environment(progress_step));
        let event_details_success =
            EventDetails::clone_changing_stage(event_details_progress.clone(), Stage::Environment(success_step));

        EnvLogger {
            logger,
            event_details_progress,
            event_details_success,
            #[cfg(feature = "env-logger-check")]
            state: Cell::new(LoggerState::Progress),
        }
    }

    pub fn send_progress(&self, msg: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                self.state.get() == LoggerState::Progress,
                "cannot send progress while a final state has been reached"
            );
        }

        self.logger.log(EngineEvent::Info(
            self.event_details_progress.clone(),
            EventMessage::new_from_safe(msg),
        ));
    }

    pub fn send_success(&self, msg: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                self.state.get() != LoggerState::Error,
                "cannot send success while an error state has been reached"
            );
            self.state.set(LoggerState::Success);
        }

        self.logger.log(EngineEvent::Info(
            self.event_details_success.clone(),
            EventMessage::new_from_safe(msg),
        ));
    }

    pub fn send_error(&self, err: EngineError) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                self.state.get() != LoggerState::Success,
                "cannot send error while an success state has been reached"
            );
            assert!(matches!(err.event_details().stage(), Stage::Environment(step) if step.is_error_step()));
            self.state.set(LoggerState::Error);
        }

        let msg = err.user_log_message().to_string();
        self.logger
            .log(EngineEvent::Error(err, Some(EventMessage::new_from_safe(msg))));
    }
}

//#[cfg(feature = "env-logger-check")]
//impl Drop for EnvLogger {
//    fn drop(&mut self) {
//        assert!(
//            self.state.get() == LoggerState::Success || self.state.get() == LoggerState::Error,
//            "env logger dropped before reaching a final state"
//        );
//    }
//}

#[cfg(feature = "env-logger-check")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum LoggerState {
    Progress,
    Success,
    Error,
}
