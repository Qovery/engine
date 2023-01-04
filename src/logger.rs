use crate::events::{EngineEvent, EventMessageVerbosity};
use tokio::sync::mpsc::UnboundedSender;
use tracing;

pub trait Logger: Send + Sync {
    fn log(&self, event: EngineEvent);
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
    fn log(&self, event: EngineEvent) {
        let event_details = event.get_details();
        let stage = event_details.stage();
        let execution_id = event_details.execution_id().to_string();

        tracing::span!(
            tracing::Level::INFO,
            "std_io_logger",
            organization_id = event_details.organisation_id().short(),
            cluster_id = event_details.cluster_id().short(),
            execution_id = execution_id.as_str(),
            provider = match event_details.provider_kind() {
                Some(kind) => kind.to_string(),
                None => "".to_string(),
            }
            .as_str(),
            stage = stage.to_string().as_str(),
            step = stage.sub_step_name().as_str(),
            transmitter = event_details.transmitter().to_string().as_str(),
        )
        .in_scope(|| {
            match event {
                EngineEvent::Debug(_, _) => debug!("{}", event.message(EventMessageVerbosity::FullDetails)),
                EngineEvent::Info(_, _) => info!("{}", event.message(EventMessageVerbosity::FullDetails)),
                EngineEvent::Warning(_, _) => warn!("{}", event.message(EventMessageVerbosity::FullDetails)),
                EngineEvent::Error(_, _) => error!("{}", event.message(EventMessageVerbosity::FullDetails)),
            };
        });
    }

    fn clone_dyn(&self) -> Box<dyn Logger> {
        Box::new(self.clone())
    }
}

impl Logger for UnboundedSender<EngineEvent> {
    fn log(&self, event: EngineEvent) {
        match self.send(event) {
            Ok(_) => {}
            Err(_) => {
                error!("Unable to send engine event to logger channel");
            }
        }
    }

    fn clone_dyn(&self) -> Box<dyn Logger> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_provider::Kind;
    use crate::errors;
    use crate::errors::EngineError;
    use crate::events::{EnvironmentStep, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;
    use tracing_test::traced_test;
    use url::Url;
    use uuid::Uuid;

    struct TestCase<'a> {
        event: EngineEvent,
        description: &'a str,
    }

    #[traced_test]
    #[test]
    fn test_log() {
        // setup:
        let orga_id = QoveryIdentifier::new(Uuid::new_v4());
        let cluster_id = QoveryIdentifier::new(Uuid::new_v4());
        let cluster_name = format!("qovery-{}", cluster_id);
        let execution_id = QoveryIdentifier::new(Uuid::new_v4());
        let app_id = QoveryIdentifier::new(Uuid::new_v4());
        let app_name = format!("simple-app-{}", app_id);
        let user_message = "User message";
        let safe_message = "Safe message";
        let raw_message = "Raw message";
        let link = Url::parse("https://qovery.com").expect("cannot parse Url");
        let hint = "An hint !";

        let test_cases = vec![
            TestCase {
                event: EngineEvent::Error(
                    EngineError::new_unknown(
                        EventDetails::new(
                            Some(Kind::Scw),
                            orga_id.clone(),
                            cluster_id.clone(),
                            execution_id.to_string(),
                            Stage::Infrastructure(InfrastructureStep::Create),
                            Transmitter::Kubernetes(Uuid::new_v4(), cluster_name.to_string()),
                        ),
                        user_message.to_string(),
                        Some(errors::CommandError::new(
                            safe_message.to_string(),
                            Some(raw_message.to_string()),
                            None,
                        )),
                        Some(link),
                        Some(hint.to_string()),
                    ),
                    None,
                ),
                description: "Error event",
            },
            TestCase {
                event: EngineEvent::Info(
                    EventDetails::new(
                        Some(Kind::Scw),
                        orga_id.clone(),
                        cluster_id.clone(),
                        execution_id.to_string(),
                        Stage::Infrastructure(InfrastructureStep::Create),
                        Transmitter::Kubernetes(Uuid::new_v4(), cluster_name),
                    ),
                    EventMessage::new(raw_message.to_string(), Some(safe_message.to_string())),
                ),
                description: "Deploying info event",
            },
            TestCase {
                event: EngineEvent::Debug(
                    EventDetails::new(
                        Some(Kind::Scw),
                        orga_id.clone(),
                        cluster_id.clone(),
                        execution_id.to_string(),
                        Stage::Environment(EnvironmentStep::Pause),
                        Transmitter::Application(Uuid::new_v4(), app_name.to_string()),
                    ),
                    EventMessage::new(raw_message.to_string(), Some(safe_message.to_string())),
                ),
                description: "Pausing application debug event",
            },
            TestCase {
                event: EngineEvent::Warning(
                    EventDetails::new(
                        Some(Kind::Scw),
                        orga_id.clone(),
                        cluster_id.clone(),
                        execution_id.to_string(),
                        Stage::Environment(EnvironmentStep::Delete),
                        Transmitter::Application(Uuid::new_v4(), app_name),
                    ),
                    EventMessage::new(raw_message.to_string(), Some(safe_message.to_string())),
                ),
                description: "Deleting application warning event",
            },
        ];

        let logger = StdIoLogger::new();

        for tc in test_cases {
            // execute:
            logger.log(tc.event.clone());

            // validate:
            assert!(
                logs_contain(match tc.event {
                    EngineEvent::Debug(_, _) => "DEBUG",
                    EngineEvent::Info(_, _) => "INFO",
                    EngineEvent::Warning(_, _) => "WARN",
                    EngineEvent::Error(_, _) => "ERROR",
                }),
                "{}",
                tc.description
            );

            assert!(
                logs_contain(format!("organization_id=\"{}\"", orga_id.short()).as_str()),
                "{}",
                tc.description
            );
            assert!(
                logs_contain(format!("cluster_id=\"{}\"", cluster_id.short()).as_str()),
                "{}",
                tc.description
            );
            assert!(
                logs_contain(format!("execution_id=\"{}\"", execution_id).as_str()),
                "{}",
                tc.description
            );

            let details = tc.event.get_details();
            assert!(
                logs_contain(
                    format!(
                        "provider=\"{}\"",
                        match details.provider_kind() {
                            Some(k) => k.to_string(),
                            None => "".to_string(),
                        }
                    )
                    .as_str()
                ),
                "{}",
                tc.description
            );

            assert!(
                logs_contain(format!("stage=\"{}\"", details.stage()).as_str()),
                "{}",
                tc.description
            );
            assert!(
                logs_contain(format!("step=\"{}\"", details.stage().sub_step_name()).as_str()),
                "{}",
                tc.description
            );
            assert!(
                logs_contain(format!("transmitter=\"{}\"", details.transmitter()).as_str()),
                "{}",
                tc.description
            );

            // Logger should display everything
            assert!(logs_contain(safe_message), "{}", tc.description);
            assert!(logs_contain(raw_message), "{}", tc.description);
        }
    }
}
