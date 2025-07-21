use crate::environment::action::deploy_job::action::{JobPostRun, JobPreRun, TaskContext};
use crate::environment::action::utils::{
    KubeObjectKind, delete_cached_image, get_last_deployed_image, mirror_image_if_necessary,
};
use crate::environment::models::job::{ImageSource, Job, JobService};
use crate::environment::models::types::CloudProvider;
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::runtime::block_on;

pub(super) fn mk_deploy_pre_run<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
) -> JobPreRun<'a>
where
    Job<T>: JobService,
{
    let metrics_registry = target.metrics_registry.clone();
    Box::new(move |logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> {
        match &job.image_source {
            // If image come from a registry, we mirror it to the cluster registry in order to avoid losing access to it due to creds expiration
            ImageSource::Registry { source } => {
                mirror_image_if_necessary(
                    job.long_id(),
                    source,
                    target,
                    logger,
                    event_details.clone(),
                    metrics_registry.clone(),
                )?;
            }
            ImageSource::Build { .. } => {}
        }

        let last_image = block_on(get_last_deployed_image(
            target.kube.client(),
            &job.kube_label_selector(),
            if job.is_cron_job() {
                KubeObjectKind::CronJob
            } else {
                KubeObjectKind::Job
            },
            target.environment.namespace(),
        ));

        Ok(TaskContext {
            last_deployed_image: last_image,
        })
    })
}

pub(super) fn mk_deploy_post_run<'a, T: CloudProvider>(job: &'a Job<T>, target: &'a DeploymentTarget) -> JobPostRun<'a>
where
    Job<T>: JobService,
{
    Box::new(move |logger: &EnvSuccessLogger, state: TaskContext| {
        // Delete previous image from cache to cleanup resources
        match &job.image_source {
            ImageSource::Registry { source } => {
                let mirrored_image_tag = source.tag_for_mirror(job.long_id());

                // In case the job is running due to the hook on on_delete
                // we don't to transition to a final state/deleted state
                // so we override the logger to not send a success message
                let empty_logger = |_: String| {};
                let normal_logger = |msg| logger.send_success(msg);
                let logger = if job.action() == &Action::Delete {
                    &empty_logger as &dyn Fn(String)
                } else {
                    &normal_logger as &dyn Fn(String)
                };
                if let Err(err) = delete_cached_image(
                    job.long_id(),
                    mirrored_image_tag,
                    state.last_deployed_image,
                    false,
                    target,
                    &logger,
                ) {
                    error!("Failed to delete previous image from cache: {}", err);
                }
            }
            ImageSource::Build { .. } => {}
        };
    })
}
