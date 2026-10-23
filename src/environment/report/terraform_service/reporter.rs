use crate::environment::models::terraform_service::TerraformServiceTrait;
use crate::environment::report::DeploymentReporter;
use crate::environment::report::failure_diagnostics::{Target, WorkloadKind, report_failure};
use crate::environment::report::logger::EnvLogger;
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::runtime::block_on;
use crate::utilities::to_short_id;
use futures::stream::BoxStream;
use futures::{AsyncBufReadExt, StreamExt, stream};
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, ResourceExt};
use std::future;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

pub struct TerraformServiceDeploymentReporter<T> {
    long_id: Uuid,
    logger: EnvLogger,
    metrics_registry: Arc<dyn MetricsRegistry>,
    action: Action,
    namespace: String,
    kube_client: kube::Client,
    pod_recv: Option<Mutex<mpsc::Receiver<Pod>>>,
    skip_terraform_job_execution: bool,
    _phantom: PhantomData<T>,
}

impl<T> TerraformServiceDeploymentReporter<T> {
    pub fn new(
        chart: &impl TerraformServiceTrait,
        deployment_target: &DeploymentTarget,
        action: Action,
        pod_recv: mpsc::Receiver<Pod>,
    ) -> Self {
        Self {
            long_id: *chart.long_id(),
            logger: deployment_target.env_logger(chart, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            action,
            kube_client: deployment_target.kube.client(),
            namespace: deployment_target.environment.namespace().to_string(),
            pod_recv: Some(Mutex::new(pod_recv)),
            skip_terraform_job_execution: false,
            _phantom: Default::default(),
        }
    }

    /// For actions that run no terraform job: there is no pod, so nothing to stream.
    pub fn without_terraform_job(
        chart: &impl TerraformServiceTrait,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> Self {
        Self {
            long_id: *chart.long_id(),
            logger: deployment_target.env_logger(chart, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            action,
            kube_client: deployment_target.kube.client(),
            namespace: deployment_target.environment.namespace().to_string(),
            pod_recv: None,
            skip_terraform_job_execution: true,
            _phantom: Default::default(),
        }
    }

    pub fn new_with_skip_terraform_job_execution(
        chart: &impl TerraformServiceTrait,
        deployment_target: &DeploymentTarget,
        action: Action,
        pod_recv: mpsc::Receiver<Pod>,
        skip_terraform_job_execution: bool,
    ) -> Self {
        Self {
            long_id: *chart.long_id(),
            logger: deployment_target.env_logger(chart, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            action,
            kube_client: deployment_target.kube.client(),
            namespace: deployment_target.environment.namespace().to_string(),
            pod_recv: Some(Mutex::new(pod_recv)),
            skip_terraform_job_execution,
            _phantom: Default::default(),
        }
    }
}

pub struct ReporterState {
    pod_api: Api<Pod>,
    pod_recv: Option<mpsc::Receiver<Pod>>,
    log_lines: Option<BoxStream<'static, Result<String, std::io::Error>>>,
    metrics_registry: Arc<dyn MetricsRegistry>,
    // Pod of the terraform job, once the deployer sent it: where the failure diagnostics look.
    pod_name: Option<String>,
}

impl ReporterState {
    fn terraform_output_stream_mut(
        &mut self,
        mut long_id_opt: Option<Uuid>,
    ) -> &mut BoxStream<'static, Result<String, std::io::Error>> {
        self.log_lines.get_or_insert_with(|| {
            // Wait for the pod sent by deployer thread; absent when the action runs no terraform job.
            let pod = match &self.pod_recv {
                Some(recv) => recv.recv().unwrap_or_default(),
                None => Pod::default(),
            };
            let log_params = LogParams {
                follow: true,
                ..Default::default()
            };
            self.pod_name = Some(pod.name_any()).filter(|name| !name.is_empty());
            if let Some(long_id) = long_id_opt.take() {
                self.metrics_registry
                    .stop_record(long_id, StepName::Deployment, StepStatus::Success);
                self.metrics_registry
                    .start_record(long_id, StepLabel::Service, StepName::Executing);
            }
            block_on(self.pod_api.log_stream(&pod.name_any(), &log_params))
                .map(|s| s.lines().boxed())
                .unwrap_or_else(|err| {
                    error!("cannot retrieve terraform logs: {}", err);
                    stream::empty().boxed()
                })
        })
    }
}

impl<T: Send + Sync> DeploymentReporter for TerraformServiceDeploymentReporter<T> {
    type DeploymentResult = T;
    type DeploymentState = ReporterState;
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&mut self) -> Self::DeploymentState {
        let pod_api: Api<Pod> = Api::namespaced(self.kube_client.clone(), &self.namespace);
        ReporterState {
            pod_api,
            log_lines: None,
            pod_recv: self.pod_recv.take().and_then(|m| m.into_inner().ok()),
            metrics_registry: self.metrics_registry.clone(),
            pod_name: None,
        }
    }

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        self.metrics_registry
            .start_record(self.long_id, StepLabel::Service, StepName::Deployment);
        self.logger.send_progress(format!(
            "🚀 {} of terraform service `{}` is starting",
            self.action,
            to_short_id(&self.long_id)
        ));
    }

    fn deployment_in_progress(&self, reporter_state: &mut Self::DeploymentState) {
        if self.skip_terraform_job_execution {
            // When skipping terraform job execution, no pod is created, so we just report progress without waiting for logs
            self.logger.send_progress(
                "Skipping terraform job execution - performing cleanup without terraform execution".to_string(),
            );
            return;
        }

        let logs_stream = reporter_state.terraform_output_stream_mut(Some(self.long_id));
        // here reporter has received the pod event so we consider the execution started
        self.logger.switch_to_executing();
        let mut ctx = Context::from_waker(Waker::noop());
        // To not block the thread we loop until we have some line to log
        // if nothing available yet, we return/yield to check if the task is not terminated
        if let Ok(Some(Ok(log))) = block_on(async { timeout(Duration::from_secs(30), logs_stream.next()).await }) {
            self.logger.send_progress(log);
        }
        while let Poll::Ready(Some(Ok(log))) = logs_stream.poll_next_unpin(&mut ctx) {
            self.logger.send_progress(log);
        }
    }

    fn deployment_terminated(
        self,
        result: &Result<Self::DeploymentResult, Box<EngineError>>,
        mut reporter_state: Self::DeploymentState,
    ) -> EnvLogger {
        // Consume all the remaining logs of the terraform output only if we're not skipping terraform job execution
        if !self.skip_terraform_job_execution {
            block_on(reporter_state.terraform_output_stream_mut(None).for_each(|line| {
                if let Ok(line) = line {
                    self.logger.send_progress(line);
                }
                future::ready(())
            }));
        }

        let error = match result {
            Ok(_) => {
                self.stop_record(StepStatus::Success);
                self.logger
                    .send_success(format!("✅ {} of terraform service succeeded", self.action));
                return self.logger;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            self.stop_record(StepStatus::Cancel);
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(r#"🚫 {} has been cancelled."#, self.action).trim().to_string(),
                None,
            ));
            return self.logger;
        }
        self.stop_record(StepStatus::Error);

        // Say why before the errors. The job logs were streamed, but the last lines belong next to the cause.
        if let Some(pod_name) = reporter_state
            .pod_name
            .as_deref()
            .filter(|_| !self.skip_terraform_job_execution)
        {
            report_failure(
                &Target {
                    kube: &self.kube_client,
                    namespace: &self.namespace,
                    selector: pod_name,
                    kind: WorkloadKind::NamedPod,
                },
                &self.logger,
                None,
            );
        }
        self.logger.send_error(*error.clone());
        self.logger.send_error(EngineError::new_engine_error(
            *error.clone(),
            format!("❌ {} of terraform service failed !", self.action),
            None,
        ));

        self.logger
    }

    fn report_frequency(&self) -> Duration {
        Duration::from_secs(1)
    }
}

impl<T> TerraformServiceDeploymentReporter<T> {
    pub(crate) fn stop_record(&self, step_status: StepStatus) {
        self.metrics_registry
            .stop_record(self.long_id, StepName::Deployment, step_status.clone());
        self.metrics_registry
            .stop_record(self.long_id, StepName::Executing, step_status.clone());
        self.metrics_registry
            .stop_record(self.long_id, StepName::Total, step_status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::report::failure_diagnostics::tests::*;
    use crate::events::{EngineEvent, EventMessageVerbosity};
    use crate::metrics_registry::StdMetricsRegistry;
    use serde_json::json;

    #[test]
    fn terraform_after_streamed_logs_verifies_live_job_uid_and_keeps_terminal_order() {
        for uid in ["job-uid", "replaced-uid"] {
            let (kube, requests) = mock_client(move |path, _| match path {
                "pods" => list(vec![pod_json(crash())]),
                "job" => Reply::Json(
                    200,
                    json!({
                        "metadata": {"name": "job", "uid": uid},
                        "status": {"conditions": [{"type": "Failed", "status": "True", "reason": "JobFailed"}]},
                    }),
                ),
                "log" => Reply::Bytes(b"Error: apply failed\n".to_vec()),
                _ => list(vec![event_json("job-uid", "Job", "owner failure", "2026-01-01T00:00:01Z")]),
            });
            let (logger, mut receiver) = test_logger();
            let mut reporter = TerraformServiceDeploymentReporter::<()> {
                long_id: Uuid::new_v4(),
                logger,
                metrics_registry: Arc::new(StdMetricsRegistry::default()),
                action: Action::Create,
                namespace: "ns".to_string(),
                kube_client: kube,
                pod_recv: None,
                skip_terraform_job_execution: false,
                _phantom: Default::default(),
            };
            let mut state = reporter.new_state();
            state.pod_name = Some("api".to_string());
            state.log_lines = Some(stream::iter(vec![Ok("already streamed".to_string())]).boxed());
            let _ = reporter.deployment_terminated(&Err(Box::new(deployment_error())), state);
            let events = sent(&mut receiver);
            assert_eq!(events[0].message(EventMessageVerbosity::SafeOnly), "already streamed");
            let lines = block(&events);
            assert!(lines.last().unwrap().contains("Error: apply failed"));
            assert_eq!(lines[0].contains("JobFailed"), uid == "job-uid");
            assert_eq!(lines.iter().any(|line| line.contains("owner failure")), uid == "job-uid");
            assert!(
                events[events.len() - 2..]
                    .iter()
                    .all(|e| matches!(e, EngineEvent::Error(..)))
            );
            assert!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(_, q)| q.contains("metadata.name%3Dapi"))
            );
        }
    }
}
