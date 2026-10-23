use qovery_engine::events::{EngineEvent, Stage, Transmitter};
use qovery_engine::metrics_registry::StepName;
use std::collections::HashMap;
use uuid::Uuid;

/// Places the environment logs of one deployment that carry no step: each takes the last step of its
/// own source, or the environment's when the source has none yet. Never another service's step.
pub struct LogStepTimeline {
    environment: StepName,
    by_transmitter: HashMap<Uuid, StepName>,
}

impl Default for LogStepTimeline {
    fn default() -> Self {
        LogStepTimeline {
            environment: StepName::Queueing,
            by_transmitter: HashMap::new(),
        }
    }
}

impl LogStepTimeline {
    pub fn place(&mut self, event: EngineEvent) -> EngineEvent {
        if !matches!(event.get_details().stage(), Stage::Environment(_)) {
            return event;
        }

        let transmitter = event.get_details().transmitter();
        let transmitter_id = transmitter.id();
        let previous_step = self
            .by_transmitter
            .get(&transmitter_id)
            .copied()
            .unwrap_or(self.environment);
        let event = event.with_step_name_if_unset(Some(previous_step));

        if let Some(step) = event.get_details().step_name() {
            if matches!(transmitter, Transmitter::Environment(..)) {
                self.environment = step;
            }
            self.by_transmitter.insert(transmitter_id, step);
        }
        event
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use qovery_engine::events::{EnvironmentStep, EventDetails, EventMessage, InfrastructureStep};
    use qovery_engine::io_models::QoveryIdentifier;

    fn event(stage: Stage, transmitter: Transmitter, step: Option<StepName>) -> EngineEvent {
        let details = EventDetails::new(
            None,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            "execution".to_string(),
            stage,
            transmitter,
        )
        .with_step_name_if_unset(step);
        EngineEvent::Info(details, EventMessage::new_from_safe("line".to_string()))
    }

    fn environment(step: Option<StepName>) -> EngineEvent {
        event(
            Stage::Environment(EnvironmentStep::Deployed),
            Transmitter::Environment(Uuid::nil(), "staging".to_string()),
            step,
        )
    }

    fn service(id: Uuid, step: Option<StepName>) -> EngineEvent {
        event(
            Stage::Environment(EnvironmentStep::Deploy),
            Transmitter::Application(id, "app".to_string()),
            step,
        )
    }

    fn placed_step(timeline: &mut LogStepTimeline, event: EngineEvent) -> Option<StepName> {
        timeline.place(event).get_details().step_name()
    }

    #[test]
    fn an_environment_line_without_a_step_takes_the_environment_last_step() {
        let mut timeline = LogStepTimeline::default();
        let api = Uuid::new_v4();

        placed_step(&mut timeline, environment(Some(StepName::Deployment)));
        placed_step(&mut timeline, service(api, Some(StepName::Build)));

        assert_eq!(placed_step(&mut timeline, environment(None)), Some(StepName::Deployment));
    }

    #[test]
    fn a_service_line_without_a_step_takes_its_own_last_step_never_another_service() {
        let mut timeline = LogStepTimeline::default();
        let (api, worker) = (Uuid::new_v4(), Uuid::new_v4());

        placed_step(&mut timeline, environment(Some(StepName::Build)));
        placed_step(&mut timeline, service(api, Some(StepName::Deployment)));

        assert_eq!(placed_step(&mut timeline, service(api, None)), Some(StepName::Deployment));
        assert_eq!(placed_step(&mut timeline, service(worker, None)), Some(StepName::Build));
    }

    #[test]
    fn a_line_keeps_the_step_it_carries() {
        let mut timeline = LogStepTimeline::default();

        placed_step(&mut timeline, environment(Some(StepName::Build)));

        assert_eq!(
            placed_step(&mut timeline, environment(Some(StepName::Deployment))),
            Some(StepName::Deployment)
        );
    }

    #[test]
    fn lines_before_the_engine_first_step_join_the_queueing_core_logged() {
        let mut timeline = LogStepTimeline::default();

        assert_eq!(placed_step(&mut timeline, environment(None)), Some(StepName::Queueing));
        assert_eq!(
            placed_step(&mut timeline, service(Uuid::new_v4(), None)),
            Some(StepName::Queueing)
        );
    }

    #[test]
    fn queueing_is_written_in_core_spelling() {
        assert_eq!(serde_json::to_value(StepName::Queueing).ok(), Some("QUEUEING".into()));
    }

    #[test]
    fn infrastructure_lines_stay_without_a_step() {
        let mut timeline = LogStepTimeline::default();
        placed_step(&mut timeline, environment(Some(StepName::Deployment)));

        let infrastructure = event(
            Stage::Infrastructure(InfrastructureStep::Create),
            Transmitter::Kubernetes(Uuid::new_v4(), "cluster".to_string()),
            None,
        );

        assert_eq!(placed_step(&mut timeline, infrastructure), None);
    }
}
