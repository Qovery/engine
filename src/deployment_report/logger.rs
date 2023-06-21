use crate::cloud_provider::service::Service;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::logger::Logger;
use std::sync::Arc;

use crate::events::EnvironmentStep::JobOutput;
#[cfg(feature = "env-logger-check")]
use std::sync::atomic::AtomicUsize;
#[cfg(feature = "env-logger-check")]
use std::sync::atomic::Ordering;

pub struct EnvLogger {
    logger: Arc<Box<dyn Logger>>,
    event_details_progress: EventDetails,
    event_details_success: EventDetails,
    #[cfg(feature = "env-logger-check")]
    state: AtomicUsize,
}

impl EnvLogger {
    pub fn new(service: &(impl Service + ?Sized), step: EnvironmentStep, logger: Arc<Box<dyn Logger>>) -> Self {
        let (progress_step, success_step) = match step {
            EnvironmentStep::Deploy => (EnvironmentStep::Deploy, EnvironmentStep::Deployed),
            EnvironmentStep::Pause => (EnvironmentStep::Pause, EnvironmentStep::Paused),
            EnvironmentStep::Delete => (EnvironmentStep::Delete, EnvironmentStep::Deleted),
            EnvironmentStep::Build => (EnvironmentStep::Build, EnvironmentStep::Built),
            EnvironmentStep::Restart => (EnvironmentStep::Restart, EnvironmentStep::Restarted),
            _ => panic!("Invalid environment step for logger"),
        };
        let event_details_progress = service.get_event_details(Stage::Environment(progress_step));
        let event_details_success = service.get_event_details(Stage::Environment(success_step));

        EnvLogger {
            logger,
            event_details_progress,
            event_details_success,
            #[cfg(feature = "env-logger-check")]
            state: AtomicUsize::new(LoggerState::Progress as usize),
        }
    }

    pub fn send_progress(&self, msg: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) == LoggerState::Progress,
                "cannot send progress while a final state has been reached"
            );
        }

        self.logger.log(EngineEvent::Info(
            self.event_details_progress.clone(),
            EventMessage::new_from_safe(msg),
        ));
    }

    pub fn send_warning(&self, msg: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) == LoggerState::Progress,
                "cannot send warning while a final state has been reached"
            );
        }

        self.logger.log(EngineEvent::Warning(
            self.event_details_progress.clone(),
            EventMessage::new_from_safe(msg),
        ));
    }

    pub fn log(&self, engine_event: EngineEvent) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) == LoggerState::Progress,
                "cannot log while a final state has been reached"
            );
        }

        self.logger.log(engine_event);
    }

    pub fn send_success(&self, msg: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) != LoggerState::Error,
                "cannot send success while an error state has been reached"
            );
            self.state.store(LoggerState::Success as usize, Ordering::Release);
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
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) != LoggerState::Success,
                "cannot send error while an success state has been reached"
            );
            assert!(matches!(err.event_details().stage(), Stage::Environment(step) if step.is_error_step()));
            self.state.store(LoggerState::Error as usize, Ordering::Release);
        }

        self.logger
            .log(EngineEvent::Error(err.clone(), Some(EventMessage::new_from_engine_error(err))));
    }

    pub fn send_core_configuration(&self, safe_message: String, json: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) == LoggerState::Progress,
                "cannot send warning while a final state has been reached"
            );
        }

        self.logger.log(EngineEvent::Info(
            EventDetails::clone_changing_stage(self.event_details_progress.clone(), Stage::Environment(JobOutput)),
            EventMessage::new_for_sending_core_data(safe_message, json),
        ));
    }
}

pub struct EnvProgressLogger<'a> {
    logger: &'a EnvLogger,
}

impl<'a> EnvProgressLogger<'a> {
    pub fn new(env_logger: &EnvLogger) -> EnvProgressLogger {
        EnvProgressLogger { logger: env_logger }
    }

    pub fn info(&self, msg: String) {
        self.logger.send_progress(msg);
    }

    pub fn warning(&self, msg: String) {
        self.logger.send_warning(msg);
    }

    pub fn log(&self, engine_event: EngineEvent) {
        self.logger.log(engine_event);
    }

    pub fn core_configuration(&self, msg: String, json: String) {
        self.logger.send_core_configuration(msg, json)
    }
}

pub struct EnvSuccessLogger<'a> {
    logger: &'a EnvLogger,
}

impl<'a> EnvSuccessLogger<'a> {
    pub fn new(env_logger: &EnvLogger) -> EnvSuccessLogger {
        EnvSuccessLogger { logger: env_logger }
    }

    pub fn send_success(&self, msg: String) {
        self.logger.send_success(msg);
    }
}

#[cfg(feature = "env-logger-check")]
impl Drop for EnvLogger {
    fn drop(&mut self) {
        let state = LoggerState::from_usize(self.state.load(Ordering::Relaxed));
        assert!(
            state == LoggerState::Success || state == LoggerState::Error,
            "env logger dropped before reaching a final state"
        );
    }
}

#[cfg(feature = "env-logger-check")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum LoggerState {
    Progress = 0,
    Success = 1,
    Error = 2,
}

#[cfg(feature = "env-logger-check")]
impl LoggerState {
    fn from_usize(value: usize) -> LoggerState {
        match value {
            0 => LoggerState::Progress,
            1 => LoggerState::Success,
            2 => LoggerState::Error,
            _ => panic!("invalid usize value for LoggerState"),
        }
    }
}
