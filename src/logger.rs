use crate::events::EngineEvent;
use tracing;

#[derive(Debug, Clone)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

pub trait Logger: Send {
    fn log(&self, log_level: LogLevel, event: EngineEvent);
    fn clone_dyn(&self) -> Box<dyn Logger>;
}

impl Clone for Box<dyn Logger> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

#[derive(Clone)]
pub struct StdIoLogger {}

impl StdIoLogger {
    pub fn new() -> StdIoLogger {
        // TODO(benjaminch): configure tracing library in here, should be transparent for parent caller.
        StdIoLogger {}
    }
}

impl Default for StdIoLogger {
    fn default() -> Self {
        StdIoLogger::new()
    }
}

impl Logger for StdIoLogger {
    fn log(&self, log_level: LogLevel, event: EngineEvent) {
        let event_details = event.get_details();
        let stage = event_details.stage();

        tracing::span!(
            tracing::Level::INFO,
            "std_io_logger",
            organization_id = event_details.organisation_id().short(),
            cluster_id = event_details.cluster_id().short(),
            execution_id = event_details.execution_id().short(),
            stage = stage.to_string().as_str(),
            step = stage.sub_step_name().as_str(),
            transmitter = event_details.transmitter().to_string().as_str(),
        )
        .in_scope(|| {
            match log_level {
                LogLevel::Debug => debug!("{}", event.get_message()),
                LogLevel::Info => info!("{}", event.get_message()),
                LogLevel::Warning => warn!("{}", event.get_message()),
                LogLevel::Error => error!("{}", event.get_message()),
            };
        });
    }

    fn clone_dyn(&self) -> Box<dyn Logger> {
        Box::new(self.clone())
    }
}
