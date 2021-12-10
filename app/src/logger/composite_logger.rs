use qovery_engine::events::EngineEvent;
use qovery_engine::logger::{LogLevel, Logger};

#[derive(Clone)]
pub struct CompositeLogger {
    loggers: Vec<Box<dyn Logger>>,
}

impl CompositeLogger {
    pub fn new(loggers: Vec<Box<dyn Logger>>) -> Self {
        CompositeLogger { loggers }
    }
}

impl Logger for CompositeLogger {
    fn log(&self, log_level: LogLevel, event: EngineEvent) {
        for logger in &self.loggers {
            logger.log(log_level.clone(), event.clone());
        }
    }

    fn heartbeat_log_for_task(&self, _log_level: LogLevel, _event: EngineEvent, _f: &dyn Fn()) {
        todo!()
    }

    fn clone_dyn(&self) -> Box<dyn Logger> {
        Box::new(self.clone())
    }
}
