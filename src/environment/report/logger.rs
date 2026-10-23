use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::infrastructure::models::cloud_provider::service::{Service, ServiceType};
use crate::logger::Logger;
use crate::metrics_registry::StepName;
#[cfg(feature = "env-logger-check")]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::events::EnvironmentStep::{AgenticWorkflowOutput, DatabaseOutput, JobOutput, TerraformServiceOutput};

pub struct EnvLogger {
    logger: Arc<Box<dyn Logger>>,
    event_details_progress: EventDetails,
    event_details_progress_executing: EventDetails,
    event_details_success: EventDetails,
    #[cfg(feature = "env-logger-check")]
    state: AtomicUsize,
    executing: AtomicBool,
    current_step: Mutex<Option<StepName>>,
}

/// A step entered with `EnvLogger::enter_step`. `end()` restores the step before it. Dropped
/// without `end()`, as when an error returns early, the step stays, so the error line names it.
#[must_use = "call end() once the step succeeds"]
pub struct StepGuard<'a> {
    logger: &'a EnvLogger,
    previous: Option<StepName>,
}

impl StepGuard<'_> {
    pub fn end(self) {
        self.logger.set_current_step(self.previous);
    }
}

fn deployment_step(service_type: ServiceType) -> StepName {
    match service_type {
        ServiceType::Router => StepName::RouterDeployment,
        ServiceType::Application
        | ServiceType::Database(_)
        | ServiceType::Container
        | ServiceType::Job
        | ServiceType::HelmChart
        | ServiceType::Terraform
        | ServiceType::AgenticWorkflow => StepName::Deployment,
    }
}

/// Progress, success and first log step of a service action. Pause, delete and restart log under
/// the deployment step: their reporter times them as a deployment.
fn logger_steps(step: EnvironmentStep, service_type: ServiceType) -> (EnvironmentStep, EnvironmentStep, StepName) {
    match step {
        EnvironmentStep::Deploy => (
            EnvironmentStep::Deploy,
            EnvironmentStep::Deployed,
            deployment_step(service_type),
        ),
        EnvironmentStep::Pause => (EnvironmentStep::Pause, EnvironmentStep::Paused, deployment_step(service_type)),
        EnvironmentStep::Delete => (EnvironmentStep::Delete, EnvironmentStep::Deleted, deployment_step(service_type)),
        EnvironmentStep::Build => (EnvironmentStep::Build, EnvironmentStep::Built, StepName::Build),
        EnvironmentStep::Restart => (
            EnvironmentStep::Restart,
            EnvironmentStep::Restarted,
            deployment_step(service_type),
        ),
        _ => panic!("Invalid environment step for logger"),
    }
}

impl EnvLogger {
    pub fn new(service: &(impl Service + ?Sized), step: EnvironmentStep, logger: Arc<Box<dyn Logger>>) -> Self {
        let (progress_step, success_step, current_step) = logger_steps(step, service.service_type());
        let event_details_progress = service.get_event_details(Stage::Environment(progress_step));
        let event_details_progress_executing = EventDetails::clone_changing_stage(
            event_details_progress.clone(),
            Stage::Environment(EnvironmentStep::Executing),
        );
        let event_details_success = service.get_event_details(Stage::Environment(success_step));

        EnvLogger {
            logger,
            event_details_progress,
            event_details_progress_executing,
            event_details_success,
            executing: AtomicBool::new(false),
            #[cfg(feature = "env-logger-check")]
            state: AtomicUsize::new(LoggerState::Progress as usize),
            current_step: Mutex::new(Some(current_step)),
        }
    }

    /// Masks the secrets of this logger in a text that is about to be reshaped (cut, split) before it is logged.
    pub fn obfuscate(&self, text: String) -> String {
        self.logger.obfuscate_secrets(text)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(logger: Arc<Box<dyn Logger>>, event_details: EventDetails) -> Self {
        EnvLogger {
            logger,
            event_details_progress: event_details.clone(),
            event_details_progress_executing: event_details.clone(),
            event_details_success: event_details,
            #[cfg(feature = "env-logger-check")]
            state: AtomicUsize::new(LoggerState::Progress as usize),
            executing: AtomicBool::new(false),
            current_step: Mutex::new(None),
        }
    }

    /// Tags every line with `step` until the returned guard ends.
    pub fn enter_step(&self, step: StepName) -> StepGuard<'_> {
        StepGuard {
            logger: self,
            previous: self.set_current_step(Some(step)),
        }
    }

    fn set_current_step(&self, step: Option<StepName>) -> Option<StepName> {
        std::mem::replace(&mut *self.current_step.lock().unwrap_or_else(PoisonError::into_inner), step)
    }

    // Executing wins: the reporter thread switches to it while the task thread may hold a step
    fn current_step(&self) -> Option<StepName> {
        if self.executing.load(Ordering::Acquire) {
            return Some(StepName::Executing);
        }
        *self.current_step.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn switch_to_executing(&self) {
        self.executing.store(true, Ordering::Release);
    }

    pub fn send_progress(&self, msg: String) {
        self.send_progress_tagged(self.current_step(), msg);
    }

    /// Tags one line with `step` without changing the current one, for interleaved output.
    pub fn send_progress_in(&self, step: StepName, msg: String) {
        self.send_progress_tagged(Some(step), msg);
    }

    fn send_progress_tagged(&self, step: Option<StepName>, msg: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) == LoggerState::Progress,
                "cannot send progress while a final state has been reached"
            );
        }
        let event_details = if self.executing.load(Ordering::Acquire) {
            self.event_details_progress_executing.clone()
        } else {
            self.event_details_progress.clone()
        };

        self.logger.log(EngineEvent::Info(
            event_details.with_step_name_if_unset(step),
            EventMessage::new_from_safe(msg),
        ));
    }

    pub fn send_recap(&self, msg: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) == LoggerState::Progress,
                "cannot send progress while a final state has been reached"
            );
        }

        let mut recap_event_details = self.event_details_progress.clone();
        recap_event_details.mut_to_recap_stage();
        self.logger
            .log(EngineEvent::Info(recap_event_details, EventMessage::new_from_safe(msg)));
    }

    pub fn send_warning(&self, msg: String) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) == LoggerState::Progress,
                "cannot send warning while a final state has been reached"
            );
        }

        let details = if self.executing.load(Ordering::Acquire) {
            self.event_details_progress_executing.clone()
        } else {
            self.event_details_progress.clone()
        };
        self.logger.log(EngineEvent::Warning(
            details.with_step_name_if_unset(self.current_step()),
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

        self.logger
            .log(engine_event.with_step_name_if_unset(self.current_step()));
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
            self.event_details_success
                .clone()
                .with_step_name_if_unset(self.current_step()),
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

        let err = err.with_step_name_if_unset(self.current_step());
        self.logger
            .log(EngineEvent::Error(err.clone(), Some(EventMessage::from(err))));
    }

    pub fn send_core_configuration_for_job(&self, safe_message: String, json: String) {
        self.send_core_configuration(safe_message, json, JobOutput);
    }

    /// Send workflow outputs to core using the workflow variable update path.
    pub fn send_core_configuration_for_agentic_workflow(&self, safe_message: String, json: String) {
        self.send_core_configuration(safe_message, json, AgenticWorkflowOutput);
    }

    pub fn send_core_configuration_for_database(&self, safe_message: String, json: String) {
        self.send_core_configuration(safe_message, json, DatabaseOutput);
    }

    pub fn send_core_configuration_for_terraform_service(&self, safe_message: String, json: String) {
        self.send_core_configuration(safe_message, json, TerraformServiceOutput);
    }

    fn send_core_configuration(&self, safe_message: String, json: String, step: EnvironmentStep) {
        #[cfg(feature = "env-logger-check")]
        {
            assert!(
                LoggerState::from_usize(self.state.load(Ordering::Acquire)) == LoggerState::Progress,
                "cannot send warning while a final state has been reached"
            );
        }

        self.logger.log(EngineEvent::Info(
            EventDetails::clone_changing_stage(self.event_details_progress.clone(), Stage::Environment(step)),
            EventMessage::new_for_sending_core_data(safe_message, json),
        ));
    }
}

pub struct EnvProgressLogger<'a> {
    logger: &'a EnvLogger,
}

impl<'a> EnvProgressLogger<'a> {
    pub fn new(env_logger: &EnvLogger) -> EnvProgressLogger<'_> {
        EnvProgressLogger { logger: env_logger }
    }

    pub fn enter_step(&self, step: StepName) -> StepGuard<'a> {
        self.logger.enter_step(step)
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

    pub fn core_configuration_for_job(&self, msg: String, json: String) {
        self.logger.send_core_configuration_for_job(msg, json)
    }

    /// Send workflow outputs while the deployment is in progress.
    pub fn core_configuration_for_agentic_workflow(&self, msg: String, json: String) {
        self.logger.send_core_configuration_for_agentic_workflow(msg, json)
    }

    pub fn core_configuration_for_database(&self, msg: String, json: String) {
        self.logger.send_core_configuration_for_database(msg, json)
    }

    pub fn core_configuration_for_terraform_service(&self, msg: String, json: String) {
        self.logger.send_core_configuration_for_terraform_service(msg, json)
    }

    pub fn switch_to_executing_step(&self) {
        self.logger.switch_to_executing();
    }
}

pub struct EnvSuccessLogger<'a> {
    logger: &'a EnvLogger,
}

impl EnvSuccessLogger<'_> {
    pub fn new(env_logger: &EnvLogger) -> EnvSuccessLogger<'_> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Transmitter, io};
    use crate::io_models::QoveryIdentifier;
    use crate::logger::UnboundedSenderLogger;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
    use uuid::Uuid;

    #[test]
    fn agentic_workflow_output_preserves_secrets_and_uses_dedicated_wire_step() {
        let workflow_id = Uuid::new_v4();
        let details = EventDetails::new(
            None,
            QoveryIdentifier::new(Uuid::new_v4()),
            QoveryIdentifier::new(Uuid::new_v4()),
            "execution".to_string(),
            Stage::Environment(EnvironmentStep::Deploy),
            Transmitter::Job(workflow_id, "workflow".to_string()),
        );
        let (sender, mut receiver) = unbounded_channel();
        let logger = EnvLogger {
            logger: Arc::new(Box::new(UnboundedSenderLogger::new(sender, vec!["secret-value".to_string()]))),
            event_details_progress: details.clone(),
            event_details_progress_executing: details.clone(),
            event_details_success: details,
            #[cfg(feature = "env-logger-check")]
            state: AtomicUsize::new(LoggerState::Progress as usize),
            executing: AtomicBool::new(false),
            current_step: Mutex::new(None),
        };
        let output = r#"{"COST":{"value":"0.75","sensitive":false},"TOKEN":{"value":"secret-value","sensitive":true}}"#;

        EnvProgressLogger::new(&logger)
            .core_configuration_for_agentic_workflow("Workflow output succeeded".to_string(), output.to_string());
        logger.send_success("Done".to_string());

        let event = receiver.try_recv().unwrap();
        assert_eq!(
            event.get_details().stage(),
            &Stage::Environment(EnvironmentStep::AgenticWorkflowOutput)
        );
        assert!(event.get_details().stage().is_core_output());
        let serialized = serde_json::to_value(io::EngineEvent::from(event)).unwrap();
        assert_eq!(serialized["details"]["stage"]["step"], "AgenticWorkflowOutput");
        assert_eq!(serialized["details"]["transmitter"]["id"], workflow_id.to_string());
        assert_eq!(serialized["message"]["full_details"], output);
    }

    fn details(step: EnvironmentStep) -> EventDetails {
        EventDetails::new(
            None,
            QoveryIdentifier::new(Uuid::new_v4()),
            QoveryIdentifier::new(Uuid::new_v4()),
            "execution".to_string(),
            Stage::Environment(step),
            Transmitter::Application(Uuid::new_v4(), "api".to_string()),
        )
    }

    fn logger_in(step: Option<StepName>) -> (EnvLogger, UnboundedReceiver<EngineEvent>) {
        let (sender, receiver) = unbounded_channel();
        let progress = details(EnvironmentStep::Build);
        let logger = EnvLogger {
            logger: Arc::new(Box::new(UnboundedSenderLogger::new(sender, vec![]))),
            event_details_progress: progress.clone(),
            event_details_progress_executing: progress.clone(),
            event_details_success: progress,
            #[cfg(feature = "env-logger-check")]
            state: AtomicUsize::new(LoggerState::Progress as usize),
            executing: AtomicBool::new(false),
            current_step: Mutex::new(step),
        };
        (logger, receiver)
    }

    fn sent_steps(receiver: &mut UnboundedReceiver<EngineEvent>) -> Vec<Option<StepName>> {
        std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|event| event.get_details().step_name())
            .collect()
    }

    #[test]
    fn lines_carry_the_entered_step_until_it_ends() {
        let (logger, mut receiver) = logger_in(Some(StepName::Build));

        logger.send_progress("🕵️ Checking if image already exists remotely".to_string());
        let clone = logger.enter_step(StepName::GitClone);
        logger.send_warning("🗝️ Unable to get credentials for git repository".to_string());
        let builder = logger.enter_step(StepName::ProvisionBuilder);
        logger.send_progress("builder pod is ready".to_string());
        builder.end();
        clone.end();
        logger.send_progress_in(StepName::ImagePush, "#12 pushing layers".to_string());
        logger.send_progress("#13 [3/5] RUN npm ci".to_string());
        logger.send_success("✅ Container image is built and ready to use".to_string());

        assert_eq!(
            sent_steps(&mut receiver),
            vec![
                Some(StepName::Build),
                Some(StepName::GitClone),
                Some(StepName::ProvisionBuilder),
                Some(StepName::ImagePush),
                Some(StepName::Build),
                Some(StepName::Build),
            ]
        );
    }

    #[test]
    fn errors_and_raw_events_take_the_current_step_unless_they_have_one() {
        let (logger, mut receiver) = logger_in(Some(StepName::Deployment));
        let _mirror = logger.enter_step(StepName::MirrorImage);

        logger.log(EngineEvent::Info(
            details(EnvironmentStep::Deploy),
            EventMessage::new_from_safe("pulling image".to_string()),
        ));
        logger.log(EngineEvent::Info(
            details(EnvironmentStep::Deploy).with_step_name_if_unset(Some(StepName::Deployment)),
            EventMessage::new_from_safe("rollout".to_string()),
        ));
        logger.send_error(EngineError::new_unknown(
            details(EnvironmentStep::DeployedError),
            "mirroring failed".to_string(),
            None,
            None,
            None,
        ));

        assert_eq!(
            sent_steps(&mut receiver),
            vec![
                Some(StepName::MirrorImage),
                Some(StepName::Deployment),
                Some(StepName::MirrorImage)
            ]
        );
    }

    #[test]
    fn a_step_left_by_an_error_tags_the_error_line() {
        let (logger, mut receiver) = logger_in(Some(StepName::Build));
        let clone_repository = || -> Result<(), ()> {
            let _clone = logger.enter_step(StepName::GitClone);
            Err(())
        };

        assert!(clone_repository().is_err());
        logger.send_error(EngineError::new_unknown(
            details(EnvironmentStep::BuiltError),
            "git clone failed".to_string(),
            None,
            None,
            None,
        ));

        assert_eq!(sent_steps(&mut receiver), vec![Some(StepName::GitClone)]);
    }

    #[test]
    fn executing_output_stays_executing_when_another_step_ends() {
        let (logger, mut receiver) = logger_in(Some(StepName::Deployment));

        logger.send_progress("terraform init".to_string());
        let mirror = logger.enter_step(StepName::MirrorImage);
        logger.switch_to_executing();
        logger.send_progress("terraform apply output".to_string());
        mirror.end();
        logger.send_progress("terraform apply output".to_string());
        logger.send_success("✅ Terraform applied".to_string());

        assert_eq!(
            sent_steps(&mut receiver),
            vec![
                Some(StepName::Deployment),
                Some(StepName::Executing),
                Some(StepName::Executing),
                Some(StepName::Executing)
            ]
        );
    }

    #[test]
    fn pause_delete_and_restart_log_under_the_deployment_step() {
        let steps = [
            EnvironmentStep::Pause,
            EnvironmentStep::Delete,
            EnvironmentStep::Restart,
        ]
        .map(|step| logger_steps(step, ServiceType::Application).2);

        assert_eq!(steps, [StepName::Deployment; 3]);
        assert_eq!(
            logger_steps(EnvironmentStep::Pause, ServiceType::Router).2,
            StepName::RouterDeployment
        );
    }

    #[test]
    fn a_router_deploys_under_its_own_step() {
        assert_eq!(
            [
                deployment_step(ServiceType::Router),
                deployment_step(ServiceType::Application),
                deployment_step(ServiceType::Container),
            ],
            [StepName::RouterDeployment, StepName::Deployment, StepName::Deployment]
        );
    }
}
