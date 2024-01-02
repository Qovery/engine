use qovery_engine::events::EngineEvent;
use qovery_engine::logger::Logger;

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
    fn log(&self, event: EngineEvent) {
        for logger in &self.loggers {
            logger.log(event.clone());
        }
    }

    fn clone_dyn(&self) -> Box<dyn Logger> {
        Box::new(self.clone())
    }

    fn with_secrets(&self, secrets: Vec<String>) -> Box<dyn Logger> {
        let loggers = self
            .loggers
            .iter()
            .map(|logger| logger.with_secrets(secrets.clone()))
            .collect();

        Box::new(CompositeLogger { loggers })
    }
}
