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

    fn obfuscate_secrets(&self, text: String) -> String {
        self.loggers
            .iter()
            .fold(text, |text, logger| logger.obfuscate_secrets(text))
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

#[cfg(test)]
mod tests {
    use super::*;
    use qovery_engine::logger::UnboundedSenderLogger;
    use tokio::sync::mpsc::unbounded_channel;

    fn secret_logger(secret: &str) -> Box<dyn Logger> {
        let (sender, _receiver) = unbounded_channel();
        Box::new(UnboundedSenderLogger::new(sender, vec![secret.to_string()]))
    }

    #[test]
    fn composite_logger_masks_the_secrets_of_every_logger() {
        let logger = CompositeLogger::new(vec![secret_logger("first-secret"), secret_logger("second-secret")]);

        assert_eq!(
            logger.obfuscate_secrets("a first-secret and a second-secret".to_string()),
            "a xxx and a xxx"
        );

        let logger = CompositeLogger::new(vec![Box::new(qovery_engine::logger::StdIoLogger::new())]);

        assert_eq!(logger.obfuscate_secrets("plain text".to_string()), "plain text");
    }
}
