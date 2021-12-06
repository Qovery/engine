use crate::events::EngineEvent;

#[derive(Debug, Clone)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

pub trait Logger: Send {
    fn log(&self, log_level: LogLevel, event: EngineEvent);
    fn heartbeat_log_for_task(&self, log_level: LogLevel, event: EngineEvent, f: &dyn Fn());
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
        StdIoLogger {}
    }
}

impl Logger for StdIoLogger {
    fn log(&self, log_level: LogLevel, event: EngineEvent) {
        // TODO(benjaminch): make usage of tracing lik here, injecting orga, cluster and execution id
        match log_level {
            LogLevel::Debug => debug!("{:?}", event),
            LogLevel::Info => info!("{:?}", event),
            LogLevel::Warning => warn!("{:?}", event),
            LogLevel::Error => error!("{:?}", event),
        };
    }

    fn heartbeat_log_for_task(&self, _log_level: LogLevel, _event: EngineEvent, _f: &dyn Fn()) {
        todo!()
    }

    fn clone_dyn(&self) -> Box<dyn Logger> {
        Box::new(self.clone())
    }
}
