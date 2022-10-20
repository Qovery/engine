use super::utils::delete_cached_image;
use crate::cloud_provider::helm::{ChartInfo, HelmChartNamespaces};
use crate::cloud_provider::service::{Action, Service};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::utils::{get_last_deployed_image, mirror_image, KubeObjectKind};
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::job::reporter::JobDeploymentReporter;
use crate::deployment_report::logger::EnvLogger;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage};
use crate::io_models::job::JobSchedule;
use crate::models::job::{Job, JobService};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use kube::runtime::wait::{await_condition, Condition};
use kube::Api;
use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for Job<T>
where
    Job<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(self.action().to_environment_step()));
        let logger = target.env_logger(self, self.action().to_environment_step());

        execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Create), || {
            match self.schedule() {
                JobSchedule::OnStart => run_job(self, target, event_details.clone(), &logger),
                JobSchedule::OnPause => Ok(()),
                JobSchedule::OnDelete => Ok(()),
                JobSchedule::Cron(_) => run_job(self, target, event_details.clone(), &logger),
            }
        })
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(self.action().to_environment_step()));
        let logger = target.env_logger(self, self.action().to_environment_step());

        execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Pause), || {
            match self.schedule() {
                JobSchedule::OnStart => Ok(()),
                JobSchedule::OnPause => run_job(self, target, event_details.clone(), &logger),
                JobSchedule::OnDelete => Ok(()),
                JobSchedule::Cron(_) => delete_job(self, target, event_details.clone(), &logger),
            }
        })
    }
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(self.action().to_environment_step()));
        let logger = target.env_logger(self, self.action().to_environment_step());

        execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Delete), || {
            // Run the job first, if schedule event is on delete
            match self.schedule() {
                JobSchedule::OnStart => Ok(()),
                JobSchedule::OnPause => Ok(()),
                JobSchedule::OnDelete => run_job(self, target, event_details.clone(), &logger),
                JobSchedule::Cron(_) => Ok(()),
            }?;

            delete_job(self, target, event_details.clone(), &logger)
        })
    }
}

fn run_job<T: CloudProvider>(
    job: &Job<T>,
    target: &DeploymentTarget,
    event_details: EventDetails,
    logger: &EnvLogger,
) -> Result<(), EngineError>
where
    Job<T>: JobService,
{
    mirror_image(
        &job.registry,
        &job.image,
        &job.tag,
        job.tag_for_mirror(),
        target,
        logger,
        event_details.clone(),
    )?;

    let last_image = block_on(get_last_deployed_image(
        target.kube.clone(),
        &job.selector(),
        if job.is_cron_job() {
            KubeObjectKind::CronJob
        } else {
            KubeObjectKind::Job
        },
        target.environment.namespace(),
    ));

    let chart = ChartInfo {
        name: job.helm_release_name(),
        path: job.workspace_directory().to_string(),
        namespace: HelmChartNamespaces::Custom,
        custom_namespace: Some(target.environment.namespace().to_string()),
        timeout_in_seconds: job.startup_timeout().as_secs() as i64,
        k8s_selector: Some(job.selector()),
        ..Default::default()
    };

    let helm = HelmDeployment::new(
        event_details.clone(),
        job.to_tera_context(target)?,
        PathBuf::from(job.helm_chart_dir()),
        None,
        chart,
    );

    if !job.schedule().is_cronjob() {
        // We first need to delete the old job, because job spec cannot be updated (due to be an immutable resources)
        helm.on_delete(target)?;
    }
    helm.on_create(target)?;

    // Wait for the job to terminate in order to have his status
    // For cronjob we dont care as we don't control when it is executed
    if !job.schedule().is_cronjob() {
        let jobs: Api<K8sJob> = Api::namespaced(target.kube.clone(), target.environment.namespace());
        let ret = block_on(async { await_condition(jobs, &job.kube_service_name(), is_job_terminated()).await });
        let ret = ret.unwrap();
        match job_status(&ret.as_ref()) {
            JobStatus::Success => Ok(()),
            JobStatus::NotRunning | JobStatus::Running => unreachable!(),
            JobStatus::Failure { reason, message } => {
                let msg = format!("Job failed due to {} {}", reason, message);
                Err(EngineError::new_invalid_engine_payload(event_details.clone(), &msg))
            }
        }?;
    }

    // Delete previous image from cache to cleanup resources
    delete_cached_image(job.tag_for_mirror(), last_image, target, logger)
        .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;

    Ok(())
}

fn delete_job<T: CloudProvider>(
    job: &Job<T>,
    target: &DeploymentTarget,
    event_details: EventDetails,
    logger: &EnvLogger,
) -> Result<(), EngineError>
where
    Job<T>: JobService,
{
    let chart = ChartInfo {
        name: job.helm_release_name(),
        path: job.workspace_directory().to_string(),
        namespace: HelmChartNamespaces::Custom,
        custom_namespace: Some(target.environment.namespace().to_string()),
        timeout_in_seconds: job.startup_timeout().as_secs() as i64,
        k8s_selector: Some(job.selector()),
        ..Default::default()
    };

    let helm = HelmDeployment::new(
        event_details.clone(),
        job.to_tera_context(target)?,
        PathBuf::from(job.helm_chart_dir()),
        None,
        chart,
    );

    helm.on_delete(target)?;

    let last_image = block_on(get_last_deployed_image(
        target.kube.clone(),
        &job.selector(),
        if job.is_cron_job() {
            KubeObjectKind::CronJob
        } else {
            KubeObjectKind::Job
        },
        target.environment.namespace(),
    ));

    // Delete previous image from cache to cleanup resources
    delete_cached_image(job.tag_for_mirror(), last_image, target, logger)
        .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;

    Ok(())
}

enum JobStatus {
    NotRunning,
    Running,
    Success,
    Failure { reason: String, message: String },
}

fn job_status(job: &Option<&K8sJob>) -> JobStatus {
    if let Some(pod) = job {
        if let Some(status) = &pod.status {
            if status.succeeded.is_some() {
                return JobStatus::Success;
            }

            if status.failed.is_some() {
                let condition = status
                    .conditions
                    .as_ref()
                    .and_then(|conds| conds.iter().find(|c| c.type_ == "Failed").cloned())
                    .unwrap_or_default();
                return JobStatus::Failure {
                    reason: condition.reason.unwrap_or_default(),
                    message: condition.message.unwrap_or_default(),
                };
            }
        }
        return JobStatus::Running;
    }
    JobStatus::NotRunning
}

fn is_job_terminated() -> impl Condition<K8sJob> {
    |job: Option<&K8sJob>| match job_status(&job) {
        JobStatus::NotRunning => false,
        JobStatus::Running => false,
        JobStatus::Success => true,
        JobStatus::Failure { .. } => true,
    }
}
