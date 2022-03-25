use crate::nats;
use qovery_engine::errors::{CommandError, EngineError};
use qovery_engine::events::io::EngineEvent as EngineEventIo;
use qovery_engine::events::EngineEvent;
use qovery_engine::logger::{Logger, StdIoLogger};

#[derive(Clone)]
pub struct NatsLogger {
    std_logger: StdIoLogger,
    nats_connection: nats::Connection,
}

impl NatsLogger {
    pub fn new(std_logger: StdIoLogger, nats_connection: nats::Connection) -> Self {
        NatsLogger {
            std_logger,
            nats_connection,
        }
    }
}

impl Logger for NatsLogger {
    fn log(&self, event: EngineEvent) {
        let event_details = event.get_details();
        let event_io = EngineEventIo::from(event.clone());
        let error_message_qovery = "error trying to send message to NATS".to_string();
        let error_message_user = "error trying to report to Qovery backend".to_string();

        match serde_json::to_string(&event_io) {
            Ok(json_string) => {
                let subject = nats::subjects::Subject::new_for_engine_event(event.clone());

                if let Err(e) = self.nats_connection.publish(&subject, json_string.as_bytes()) {
                    let message_safe = "cannot publish event object to NATS subject";
                    let message_raw = format!("{}: {}", message_safe, e);
                    self.std_logger.log(EngineEvent::Error(
                        EngineError::new_unknown(
                            event_details.clone(),
                            error_message_qovery,
                            error_message_user,
                            Some(CommandError::new(message_raw, Some(message_safe.to_string()))),
                            None,
                            None,
                        ),
                        None,
                    ));
                }
            }
            Err(e) => {
                let message_safe = "cannot serialize event object to JSON";
                let message_raw = format!("{}: {}", message_safe, e);
                self.std_logger.log(EngineEvent::Error(
                    EngineError::new_unknown(
                        event_details.clone(),
                        error_message_qovery,
                        error_message_user,
                        Some(CommandError::new(message_raw.to_string(), Some(message_safe.to_string()))),
                        None,
                        None,
                    ),
                    None,
                ));
            }
        }
    }

    fn clone_dyn(&self) -> Box<dyn Logger> {
        Box::new(self.clone())
    }
}
