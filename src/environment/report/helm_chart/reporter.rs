use crate::environment::models::helm_chart::HelmChartService;
use crate::environment::report::DeploymentReporter;
use crate::environment::report::failure_diagnostics::{Target, WorkloadKind, report_failure};
use crate::environment::report::logger::EnvLogger;
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::utilities::to_short_id;
use std::sync::Arc;
use uuid::Uuid;

pub struct HelmChartDeploymentReporter {
    long_id: Uuid,
    logger: EnvLogger,
    metrics_registry: Arc<dyn MetricsRegistry>,
    action: Action,
    namespace: String,
    kube_client: kube::Client,
    selector: String,
}

impl HelmChartDeploymentReporter {
    pub fn new(chart: &impl HelmChartService, deployment_target: &DeploymentTarget, action: Action) -> Self {
        Self {
            long_id: *chart.long_id(),
            logger: deployment_target.env_logger(chart, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            action,
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.client(),
            selector: chart.kube_label_selector(),
        }
    }
}

impl DeploymentReporter for HelmChartDeploymentReporter {
    type DeploymentResult = ();
    type DeploymentState = ();
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&mut self) -> Self::DeploymentState {}

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        self.metrics_registry
            .start_record(self.long_id, StepLabel::Service, StepName::Deployment);
        self.logger.send_progress(format!(
            "🚀 {} of helm chart `{}` is starting",
            self.action,
            to_short_id(&self.long_id)
        ));
    }

    fn deployment_in_progress(&self, _: &mut Self::DeploymentState) {
        // We use the output of helm directly
    }

    fn deployment_terminated(
        self,
        result: &Result<Self::DeploymentResult, Box<EngineError>>,
        _: Self::DeploymentState,
    ) -> EnvLogger {
        let error = match result {
            Ok(_) => {
                self.stop_record(StepStatus::Success);
                self.logger
                    .send_success(format!("✅ {} of helm chart succeeded", self.action));
                return self.logger;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            self.stop_record(StepStatus::Cancel);
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(
                    r#"
                🚫 {} has been cancelled. Helm Chart has been rollback to previous version if rollout was on-going
                "#,
                    self.action
                )
                .trim()
                .to_string(),
                None,
            ));
            return self.logger;
        }
        //self.logger.send_error(*error.clone());
        self.stop_record(StepStatus::Error);

        // Say why before the errors. A chart without the Qovery labels matches no pod: the block says so.
        report_failure(
            &Target {
                kube: &self.kube_client,
                namespace: &self.namespace,
                selector: &self.selector,
                kind: WorkloadKind::Pods,
            },
            &self.logger,
            None,
        );
        self.logger.send_error(*error.clone());
        self.logger.send_error(EngineError::new_engine_error(
            *error.clone(),
            format!("
❌ {} of helm chart failed !
⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️
⛑ Look at the Deployment Status Reports above and ask the Qovery AI Copilot for a diagnostic to fix it https://www.qovery.com/docs/copilot/overview
⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️
                ", self.action),
            None,
        ));

        self.logger
    }
}

impl HelmChartDeploymentReporter {
    pub(crate) fn stop_record(&self, step_status: StepStatus) {
        self.metrics_registry
            .stop_record(self.long_id, StepName::Deployment, step_status.clone());
        self.metrics_registry
            .stop_record(self.long_id, StepName::Total, step_status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::report::failure_diagnostics::tests::*;
    use crate::events::EngineEvent;
    use crate::metrics_registry::StdMetricsRegistry;

    #[test]
    fn helm_failure_recaps_the_live_cause_without_redundant_warnings_before_both_errors() {
        let (kube_client, _) = mock_client(|path, _| match path {
            "pods" => list(vec![pod_json(crash())]),
            "events" => list(vec![
                event_json("pod-uid", "Pod", "not ready", "2026-01-01T00:00:00Z"),
                event_json("job-uid", "Job", "owner failure", "2026-01-01T00:00:01Z"),
            ]),
            _ => Reply::Bytes(vec![]),
        });
        let (logger, mut receiver) = test_logger();
        let reporter = HelmChartDeploymentReporter {
            long_id: Uuid::new_v4(),
            logger,
            metrics_registry: Arc::new(StdMetricsRegistry::default()),
            action: Action::Create,
            namespace: "ns".to_string(),
            kube_client,
            selector: "service=svc".to_string(),
        };
        let _ = reporter.deployment_terminated(&Err(Box::new(deployment_error())), ());
        let events = sent(&mut receiver);
        let lines = block(&events);
        assert_eq!(
            lines,
            ["Why it failed: Container 'api' is in CrashLoopBackOff (last exit code 1)."]
        );
        assert_eq!(events.len(), lines.len() + 2);
        assert!(
            events[lines.len()..]
                .iter()
                .all(|e| matches!(e, EngineEvent::Error(..)))
        );
    }
}
