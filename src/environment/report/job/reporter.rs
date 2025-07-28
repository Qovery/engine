use crate::environment::report::logger::EnvLogger;
use crate::environment::report::{DeploymentReporter, MAX_ELAPSED_TIME_WITHOUT_REPORT};
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Action;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;

use std::sync::Arc;

use k8s_openapi::api::core::v1::{Event, Pod};
use kube::Api;
use kube::api::ListParams;

use crate::environment::models::job::JobService;
use crate::environment::report::job::renderer::render_job_deployment_report;
use crate::environment::report::recap_reporter::{RecapReporterDeploymentState, render_recap_events};
use crate::environment::report::utils::to_job_render_context;
use crate::errors::Tag::JobFailure;
use crate::io_models::job::JobSchedule;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::runtime::block_on;
use itertools::Itertools;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(crate) enum JobType {
    CronJob(String),
    Job(Action),
}

impl Display for JobType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            JobType::CronJob(_) => f.write_str("cron-job"),
            JobType::Job(_) => f.write_str("job"),
        }
    }
}

pub struct JobDeploymentReporter<T> {
    long_id: Uuid,
    job_type: JobType,
    is_force_trigger: bool,
    max_duration: Duration,
    max_restarts: u32,
    action: Action,
    tag: String,
    namespace: String,
    kube_client: kube::Client,
    selector: String,
    logger: EnvLogger,
    metrics_registry: Arc<dyn MetricsRegistry>,
    send_final_deleted_status: bool,
    _phantom: PhantomData<T>,
}

impl<T> JobDeploymentReporter<T> {
    fn new_impl(
        job: &impl JobService,
        deployment_target: &DeploymentTarget,
        action: Action,
        send_final_delete_status: bool,
    ) -> JobDeploymentReporter<T> {
        let job_type = match job.job_schedule() {
            JobSchedule::OnStart { .. } => JobType::Job(Action::Create),
            JobSchedule::OnPause { .. } => JobType::Job(Action::Pause),
            JobSchedule::OnDelete { .. } => JobType::Job(Action::Delete),
            JobSchedule::Cron {
                schedule,
                timezone: _timezone,
            } => JobType::CronJob(schedule.to_string()),
        };

        JobDeploymentReporter {
            long_id: *job.long_id(),
            job_type,
            action,
            is_force_trigger: job.is_force_trigger(),
            max_duration: *job.max_duration(),
            max_restarts: job.max_restarts(),
            tag: job.version(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.client(),
            selector: job.kube_label_selector(),
            logger: deployment_target.env_logger(job, action.to_environment_step()),
            metrics_registry: deployment_target.metrics_registry.clone(),
            send_final_deleted_status: send_final_delete_status,
            _phantom: PhantomData,
        }
    }

    pub fn new(
        job: &impl JobService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> JobDeploymentReporter<T> {
        Self::new_impl(job, deployment_target, action, true)
    }

    // We dont send final status when on_delete is executed because we want to keep the job in the Deleting state
    // while we are sure we have cleaned up all the resources
    pub fn new_without_final_deleted(
        job: &impl JobService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> JobDeploymentReporter<T> {
        if action != Action::Delete {
            panic!("This method should only be used for delete action")
        }

        Self::new_impl(job, deployment_target, action, false)
    }

    fn max_duration_human_str(&self) -> String {
        format!("{0:.2} minutes", self.max_duration.as_secs_f64() / 60.0)
    }
}

impl<T: Send + Sync> DeploymentReporter for JobDeploymentReporter<T> {
    type DeploymentResult = T;
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
        // If job should be force triggered, display a specific message saying so
        self.metrics_registry
            .start_record(self.long_id, StepLabel::Service, StepName::Deployment);
        if self.is_force_trigger {
            match &self.job_type {
                JobType::CronJob(schedule) => self.logger.send_progress(format!(
                    "🚀 Force triggering deployment of cronjob with schedule `{}` at tag {} is starting",
                    schedule, self.tag
                )),
                JobType::Job(_) => self.logger.send_progress(format!(
                    "🚀 Force triggering deployment of Job at tag {} is starting with a timeout/max duration of {}",
                    self.tag,
                    self.max_duration_human_str()
                )),
            }

            return;
        }

        // Normal flow, checking if the job should be triggered on this event
        match &self.job_type {
            JobType::Job(trigger_on_action) => {
                if self.action == *trigger_on_action {
                    self.logger.send_progress(format!(
                        "🚀 Deployment of Job at tag {} is starting with a timeout/max duration of {}",
                        self.tag,
                        self.max_duration_human_str()
                    ));
                } else {
                    self.logger.send_progress(format!(
                        "🚀 Skipping deployment of Job as it should trigger on {trigger_on_action:?}"
                    ));
                }
            }
            JobType::CronJob(schedule) => self.logger.send_progress(format!(
                "🚀 Deployment of cronjob with schedule `{}` at tag {} is starting",
                schedule, self.tag
            )),
        }
    }

    fn deployment_in_progress(&self, last_report: &mut Self::DeploymentState) {
        // Fetch deployment information from kube api
        let report = match block_on(fetch_job_deployment_report(
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

        // Format the deployment information and send to it to user
        let rendered_report = match render_job_deployment_report(&self.job_type, &self.tag, &report) {
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

        // Compute events' involved object ids to keep only interesting events (e.g remove warning from Horizontal Pod Autoscaler)
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
                if let Some(event_type) = &event.type_ {
                    if event_type == "Warning" {
                        return Some(event);
                    }
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
                if self.action == Action::Delete && !self.send_final_deleted_status {
                    return self.logger;
                }

                self.logger
                    .send_success(format!("✅ {} of {} succeeded", self.action, self.job_type));
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
        let recap_report = match render_recap_events(&last_report.all_warning_events) {
            Ok(report) => report,
            Err(err) => {
                self.logger
                    .send_progress(format!("Cannot render deployment recap report. Please contact us: {err}"));
                return self.logger;
            }
        };
        for line in recap_report.trim_end().split('\n').map(str::to_string) {
            self.logger.send_recap(line);
        }

        // Retrieve last state of the job to display it in the final message.
        let job_failure_message = match block_on(fetch_job_deployment_report(
            &self.kube_client,
            &self.long_id,
            &self.selector,
            &self.namespace,
        )) {
            Ok(deployment_info) => {
                if let Some(job) = &deployment_info.job {
                    let job_ctx = to_job_render_context(job, &deployment_info.events);
                    job_ctx.message
                } else {
                    None
                }
            }
            Err(_) => None,
        };

        if error.tag() == &JobFailure {
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(r#"
❌ {} failed !

⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️
Either it couldn't be executed correctly after `{}` retries or its execution didn't finish after `{}`.
Underlying error: `{}`.
This most likely an issue with its configuration/code.
Look at your job logs in order to understand if the problem comes from the job code failure or if you just need to increase its max duration timeout.

⛑ Can't solve the issue? Please have a look at our forum https://discuss.qovery.com/
⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️
                "#, self.job_type, self.max_restarts, self.max_duration_human_str(), job_failure_message.unwrap_or_default()).trim().to_string(),
                None,
            ));
        } else {
            //self.logger.send_error(*error.clone());
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(r#"

❌ {} of {} failed but we rollbacked it to previous safe/running version !
⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️ ⬇️
Look at the Deployment Status Reports above and use our troubleshooting guide to fix it https://hub.qovery.com/docs/using-qovery/troubleshoot/
⛑ Can't solve the issue? Please have a look at our forum https://discuss.qovery.com/
⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️
                "#, self.action, self.job_type).trim().to_string(),
                None,
            ));
        }

        self.logger
    }
}

impl<T> JobDeploymentReporter<T> {
    pub(crate) fn stop_record(&self, step_status: StepStatus) {
        self.metrics_registry
            .stop_record(self.long_id, StepName::Deployment, step_status.clone());
        self.metrics_registry
            .stop_record(self.long_id, StepName::Total, step_status);
    }
}

#[derive(Debug)]
pub(crate) struct JobDeploymentReport {
    pub id: Uuid,
    pub job: Option<K8sJob>,
    pub pods: Vec<Pod>,
    pub events: Vec<Event>,
}

async fn fetch_job_deployment_report(
    kube: &kube::Client,
    service_id: &Uuid,
    selector: &str,
    namespace: &str,
) -> Result<JobDeploymentReport, kube::Error> {
    let pods_api: Api<Pod> = Api::namespaced(kube.clone(), namespace);
    let jobs_api: Api<K8sJob> = Api::namespaced(kube.clone(), namespace);
    let event_api: Api<Event> = Api::namespaced(kube.clone(), namespace);

    let list_params = ListParams::default().labels(selector).timeout(15);
    let pods = pods_api.list(&list_params);
    let events_params = ListParams::default().timeout(15);
    let events = event_api.list(&events_params);
    let jobs = jobs_api.list(&list_params);
    let (pods, jobs, events) = futures::future::try_join3(pods, jobs, events).await?;

    Ok(JobDeploymentReport {
        id: *service_id,
        pods: pods.items,
        job: jobs.items.into_iter().find_or_first(|_| true),
        events: events.items,
    })
}
