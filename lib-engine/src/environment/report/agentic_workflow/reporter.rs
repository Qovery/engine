use crate::environment::models::agentic_workflow::AgenticWorkflowService;
use crate::environment::report::agentic_workflow::renderer::render_agentic_workflow_deployment_report;
use crate::environment::report::logger::EnvLogger;
use crate::environment::report::recap_reporter::{RecapReporterDeploymentState, render_recap_events};
use crate::environment::report::utils::get_kube_events;
use crate::environment::report::{DeploymentReporter, MAX_ELAPSED_TIME_WITHOUT_REPORT};
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::runtime::block_on;
use crate::utilities::to_short_id;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use k8s_openapi::api::core::v1::{Event, Pod};
use kube::Api;
use kube::api::ListParams;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

pub struct AgenticWorkflowDeploymentReporter {
    long_id: Uuid,
    namespace: String,
    kube_client: kube::Client,
    selector: String,
    logger: EnvLogger,
    metrics_registry: Arc<dyn MetricsRegistry>,
    action: Action,
}

impl AgenticWorkflowDeploymentReporter {
    pub fn new(
        agentic_workflow: &impl AgenticWorkflowService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> Self {
        Self {
            long_id: *agentic_workflow.long_id(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.client(),
            selector: agentic_workflow.kube_label_selector(),
            logger: deployment_target.env_logger(agentic_workflow, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            action,
        }
    }
}

impl DeploymentReporter for AgenticWorkflowDeploymentReporter {
    type DeploymentResult = ();
    type DeploymentState = RecapReporterDeploymentState;
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&mut self) -> Self::DeploymentState {
        RecapReporterDeploymentState {
            report: "".to_string(),
            timestamp: Instant::now(),
            all_warning_events: vec![],
        }
    }

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        self.metrics_registry
            .start_record(self.long_id, StepLabel::Service, StepName::Deployment);
        self.logger.send_progress(format!(
            "🚀 {} of agentic workflow `{}` is starting",
            self.action,
            to_short_id(&self.long_id)
        ));
    }

    fn deployment_in_progress(&self, last_report: &mut Self::DeploymentState) {
        // The workflow runs as a job, so the agent stays silent for as long as it takes. Without
        // a periodic report, core sees no message from the engine and aborts the deployment.
        let report = match block_on(fetch_agentic_workflow_deployment_report(
            &self.kube_client,
            &self.long_id,
            &self.selector,
            &self.namespace,
        )) {
            Ok(deployment_info) => deployment_info,
            Err(err) => {
                self.logger
                    .send_warning(format!("Error while retrieving deployment information: {err}"));
                return;
            }
        };

        let rendered_report = match render_agentic_workflow_deployment_report(&self.action, &report) {
            Ok(deployment_status_report) => deployment_status_report,
            Err(err) => {
                self.logger
                    .send_progress(format!("Cannot render deployment status report. Please contact us: {err}"));
                return;
            }
        };

        // don't spam log same report unless it has been too long time elapsed without one
        if rendered_report == last_report.report && last_report.timestamp.elapsed() < MAX_ELAPSED_TIME_WITHOUT_REPORT {
            return;
        }

        // Keep only the events of our own pods/job, so unrelated warnings don't end up in the recap
        let mut event_uuids_to_keep: HashSet<String> = report
            .pods
            .into_iter()
            .filter_map(|it| it.metadata.uid)
            .collect::<HashSet<String>>();
        event_uuids_to_keep.extend(
            report
                .job
                .into_iter()
                .filter_map(|it| it.metadata.uid)
                .collect::<HashSet<String>>(),
        );

        report
            .events
            .clone()
            .into_iter()
            .filter_map(|event| {
                if !event_uuids_to_keep.contains(event.involved_object.uid.as_deref().unwrap_or_default()) {
                    return None;
                }
                if let Some(event_type) = &event.type_
                    && event_type == "Warning"
                {
                    return Some(event);
                }
                None
            })
            .for_each(|event| last_report.all_warning_events.push(event));

        *last_report = RecapReporterDeploymentState {
            report: rendered_report,
            timestamp: Instant::now(),
            all_warning_events: last_report.all_warning_events.clone(),
        };

        // Send it to user
        for line in last_report.report.trim_end().split('\n').map(str::to_string) {
            self.logger.send_progress(line);
        }
    }

    fn deployment_terminated(
        self,
        result: &Result<Self::DeploymentResult, Box<EngineError>>,
        last_report: Self::DeploymentState,
    ) -> EnvLogger {
        let error = match result {
            Ok(_) => {
                self.stop_record(StepStatus::Success);
                self.logger
                    .send_success(format!("✅ {} of agentic workflow succeeded", self.action));
                return self.logger;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            self.stop_record(StepStatus::Cancel);
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!("🚫 {} has been cancelled.", self.action),
                None,
            ));
            return self.logger;
        }
        self.stop_record(StepStatus::Error);

        // Send error recap
        match render_recap_events(&last_report.all_warning_events) {
            Ok(recap_report) => {
                for line in recap_report.trim_end().split('\n').map(str::to_string) {
                    self.logger.send_recap(line);
                }
            }
            Err(err) => {
                self.logger
                    .send_progress(format!("Cannot render deployment recap report. Please contact us: {err}"));
            }
        }

        self.logger.send_error(*error.clone());
        self.logger.send_error(EngineError::new_engine_error(
            *error.clone(),
            format!("
❌ {} of agentic workflow failed !
⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️
⛑ Look at the Deployment Status Reports above and use our troubleshooting guide to fix it https://hub.qovery.com/docs/using-qovery/troubleshoot/
⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️
                ", self.action),
            None,
        ));

        self.logger
    }
}

impl AgenticWorkflowDeploymentReporter {
    pub(crate) fn stop_record(&self, step_status: StepStatus) {
        self.metrics_registry
            .stop_record(self.long_id, StepName::Deployment, step_status.clone());
        self.metrics_registry
            .stop_record(self.long_id, StepName::Total, step_status);
    }
}

#[derive(Debug)]
pub(crate) struct AgenticWorkflowDeploymentReport {
    pub id: Uuid,
    pub job: Option<K8sJob>,
    pub pods: Vec<Pod>,
    pub events: Vec<Event>,
}

async fn fetch_agentic_workflow_deployment_report(
    kube: &kube::Client,
    service_id: &Uuid,
    selector: &str,
    namespace: &str,
) -> Result<AgenticWorkflowDeploymentReport, kube::Error> {
    let pods_api: Api<Pod> = Api::namespaced(kube.clone(), namespace);
    let jobs_api: Api<K8sJob> = Api::namespaced(kube.clone(), namespace);

    let list_params = ListParams::default().labels(selector).timeout(15);
    let pods = pods_api.list(&list_params);
    let jobs = jobs_api.list(&list_params);
    let (pods, jobs, events) = futures::future::try_join3(pods, jobs, get_kube_events(kube.clone(), namespace)).await?;

    Ok(AgenticWorkflowDeploymentReport {
        id: *service_id,
        pods: pods.items,
        job: jobs.items.into_iter().next(),
        events,
    })
}
