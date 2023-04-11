use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_report::logger::EnvLogger;
use crate::deployment_report::{DeploymentReporter, MAX_ELASPED_TIME_WITHOUT_REPORT};
use crate::errors::EngineError;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;

use k8s_openapi::api::core::v1::{Event, Pod};
use kube::api::ListParams;
use kube::Api;

use crate::deployment_report::job::renderer::render_job_deployment_report;
use crate::deployment_report::utils::to_job_render_context;
use crate::errors::Tag::JobFailure;
use crate::io_models::job::JobSchedule;
use crate::models::job::JobService;
use crate::runtime::block_on;
use itertools::Itertools;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(super) enum JobType {
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
    _phantom: PhantomData<T>,
}

impl<T> JobDeploymentReporter<T> {
    pub fn new(
        job: &impl JobService,
        deployment_target: &DeploymentTarget,
        action: Action,
    ) -> JobDeploymentReporter<T> {
        let job_type = match job.job_schedule() {
            JobSchedule::OnStart { .. } => JobType::Job(Action::Create),
            JobSchedule::OnPause { .. } => JobType::Job(Action::Pause),
            JobSchedule::OnDelete { .. } => JobType::Job(Action::Delete),
            JobSchedule::Cron { schedule } => JobType::CronJob(schedule.to_string()),
        };

        JobDeploymentReporter {
            long_id: *job.long_id(),
            job_type,
            action,
            is_force_trigger: job.is_force_trigger(),
            max_duration: *job.max_duration(),
            max_restarts: job.max_restarts(),
            tag: job.image_full(),
            namespace: deployment_target.environment.namespace().to_string(),
            kube_client: deployment_target.kube.clone(),
            selector: job.selector().unwrap_or_default(),
            logger: deployment_target.env_logger(job, action.to_environment_step()),
            _phantom: PhantomData,
        }
    }

    fn max_duration_human_str(&self) -> String {
        format!("{0:.2} minutes", self.max_duration.as_secs_f64() / 60.0)
    }
}

impl<T: Send + Sync> DeploymentReporter for JobDeploymentReporter<T> {
    type DeploymentResult = T;
    type DeploymentState = (String, Instant);
    type Logger = EnvLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn new_state(&self) -> Self::DeploymentState {
        ("".to_string(), Instant::now())
    }

    fn deployment_before_start(&self, _: &mut Self::DeploymentState) {
        // If job should be force trigerred, display a specific message saying so
        if self.is_force_trigger {
            match &self.job_type {
                JobType::CronJob(schedule) => self.logger.send_progress(format!(
                    "🚀 Force trigerring deployment of cronjob with schedule `{}` at tag {} is starting",
                    schedule, self.tag
                )),
                JobType::Job(_) => self.logger.send_progress(format!(
                    "🚀 Force trigerring deployment of Job at tag {} is starting with a timeout/max duration of {}",
                    self.tag,
                    self.max_duration_human_str()
                )),
            }

            return;
        }

        // Normal flow, checking if the job should be trigerred on this event
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
        if rendered_report == last_report.0 && last_report.1.elapsed() < MAX_ELASPED_TIME_WITHOUT_REPORT {
            return;
        }
        *last_report = (rendered_report, Instant::now());

        // Send it to user
        for line in last_report.0.trim_end().split('\n').map(str::to_string) {
            self.logger.send_progress(line);
        }
    }

    fn deployment_terminated(
        &self,
        result: &Result<Self::DeploymentResult, Box<EngineError>>,
        _: &mut Self::DeploymentState,
    ) {
        let error = match result {
            Ok(_) => {
                self.logger
                    .send_success(format!("✅ {} of {} succeeded", self.action, self.job_type));
                return;
            }
            Err(err) => err,
        };

        if error.tag().is_cancel() {
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!("🚫 {} has been cancelled.", self.action),
                None,
            ));
            return;
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
❌ {} failed: either it couldn't be executed correctly after `{}` retries or its execution didn't finish after `{}`. Underlying error: `{}`.
This most likely an issue with its configuration/code.
Look at your job logs in order to understand if the problem comes from the job code failure or if you just need to increase its max duration timeout.

⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
                "#, self.job_type, self.max_restarts, self.max_duration_human_str(), job_failure_message.unwrap_or_default()).trim().to_string(),
                None,
            ));
        } else {
            self.logger.send_error(*error.clone());
            self.logger.send_error(EngineError::new_engine_error(
                *error.clone(),
                format!(r#"
❌ {} of {} failed ! Look at the report above and to understand why.
⛑ Need Help ? Please consult our FAQ to troubleshoot your deployment https://hub.qovery.com/docs/using-qovery/troubleshoot/ and visit the forum https://discuss.qovery.com/
                "#, self.action, self.job_type).trim().to_string(),
                None,
            ));
        }
    }
}

#[derive(Debug)]
pub(super) struct JobDeploymentReport {
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
