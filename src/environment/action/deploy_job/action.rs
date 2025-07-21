use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::common::{mk_deploy_post_run, mk_deploy_pre_run};
use crate::environment::action::deploy_job::cronjob::run_cronjob;
use crate::environment::action::deploy_job::job::mk_deploy_job_run;
use crate::environment::action::utils::{KubeObjectKind, delete_cached_image, get_last_deployed_image};
use crate::environment::models::job::{ImageSource, Job, JobService};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::job::reporter::JobDeploymentReporter;
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::environment::report::{DeploymentTaskImpl, execute_long_deployment};
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::io_models::job::JobSchedule;
use crate::runtime::block_on;
use std::path::PathBuf;

pub(super) struct TaskContext {
    pub last_deployed_image: Option<String>,
}

pub(super) type JobPreRun<'a> = Box<dyn Fn(&EnvProgressLogger) -> Result<TaskContext, Box<EngineError>> + 'a>;
pub(super) type JobPostRun<'a> = Box<dyn Fn(&EnvSuccessLogger, TaskContext) + 'a>;
pub(super) type JobRun<'a> = Box<dyn Fn(&EnvProgressLogger, TaskContext) -> Result<TaskContext, Box<EngineError>> + 'a>;

impl<T: CloudProvider> DeploymentAction for Job<T>
where
    Job<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(self.action().to_environment_step()));

        match self.schedule() {
            JobSchedule::OnStart { .. } => {
                let pre_run = mk_deploy_pre_run(self, target, &event_details);
                let post_run = mk_deploy_post_run(self, target);
                let run = mk_deploy_job_run(self, target, &event_details);
                let task = DeploymentTaskImpl {
                    pre_run: &pre_run,
                    run: &run,
                    post_run_success: &post_run,
                };

                execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Create), task)
            }
            JobSchedule::Cron { .. } => {
                let (pre_run, run, post_run) = run_cronjob(self, target, &event_details);
                let task = DeploymentTaskImpl {
                    pre_run: &pre_run,
                    run: &run,
                    post_run_success: &post_run,
                };

                execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Create), task)
            }
            JobSchedule::OnPause { .. } | JobSchedule::OnDelete { .. } => {
                let job_reporter = JobDeploymentReporter::new(self, target, Action::Create);
                execute_long_deployment(job_reporter, |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                    Ok(())
                })
            }
        }
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(self.action().to_environment_step()));
        match self.schedule() {
            JobSchedule::Cron { .. } => {
                let (pre_run, run, post_run) = delete_job(self, target, &event_details);
                let task = DeploymentTaskImpl {
                    pre_run: &pre_run,
                    run: &run,
                    post_run_success: &post_run,
                };
                execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Pause), task)
            }
            JobSchedule::OnPause { .. } => {
                let event_details = &event_details;
                let pre_run = mk_deploy_pre_run(self, target, event_details);
                let post_run = mk_deploy_post_run(self, target);
                let run = mk_deploy_job_run(self, target, event_details);
                let task = DeploymentTaskImpl {
                    pre_run: &pre_run,
                    run: &run,
                    post_run_success: &post_run,
                };

                execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Pause), task)
            }
            JobSchedule::OnStart { .. } | JobSchedule::OnDelete { .. } => {
                let job_reporter = JobDeploymentReporter::new(self, target, Action::Pause);
                execute_long_deployment(job_reporter, |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                    Ok(())
                })
            }
        }
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(self.action().to_environment_step()));
        match self.schedule() {
            JobSchedule::OnDelete { .. } => {
                let event_details = &event_details;
                let pre_run = mk_deploy_pre_run(self, target, event_details);
                let post_run = mk_deploy_post_run(self, target);
                let run = mk_deploy_job_run(self, target, event_details);
                let task = DeploymentTaskImpl {
                    pre_run: &pre_run,
                    run: &run,
                    post_run_success: &post_run,
                };

                // We dont send final status when on_delete is executed because we want to keep the job in the Deleting state
                // while we are sure we have cleaned up all the resources
                execute_long_deployment(
                    JobDeploymentReporter::new_without_final_deleted(self, target, Action::Delete),
                    task,
                )
            }
            JobSchedule::Cron { .. } | JobSchedule::OnStart { .. } | JobSchedule::OnPause { .. } => Ok(()),
        }?;

        let (pre_run, run, post_run) = delete_job(self, target, &event_details);
        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };
        execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Delete), task)
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let command_error = CommandError::new_from_safe_message("Cannot restart Job service".to_string());
        Err(Box::new(EngineError::new_cannot_restart_service(
            self.get_event_details(Stage::Environment(EnvironmentStep::Restart)),
            target.environment.namespace(),
            &self.kube_label_selector(),
            command_error,
        )))
    }
}

pub(super) fn delete_job<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
) -> (JobPreRun<'a>, JobRun<'a>, JobPostRun<'a>)
where
    Job<T>: JobService,
{
    let pre_run = move |_logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> {
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
    };

    let task = move |_logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
        let chart = ChartInfo {
            name: job.helm_release_name(),
            path: job.workspace_directory().to_string(),
            namespace: HelmChartNamespaces::Custom,
            custom_namespace: Some(target.environment.namespace().to_string()),
            timeout_in_seconds: job.startup_timeout().as_secs() as i64,
            k8s_selector: Some(job.kube_label_selector()),
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

        Ok(state)
    };

    let post_run = move |logger: &EnvSuccessLogger, state: TaskContext| {
        match &job.image_source {
            // Delete previous image from cache to cleanup resources
            ImageSource::Registry { source } => {
                let mirrored_image_tag = source.tag_for_mirror(job.long_id());
                if let Err(err) = delete_cached_image(
                    job.long_id(),
                    mirrored_image_tag,
                    state.last_deployed_image,
                    true,
                    target,
                    &|msg| logger.send_success(msg),
                ) {
                    let user_msg = format!("Failed to delete previous image from cache: {err}");
                    logger.send_success(user_msg);
                }
            }

            // Delete shared container repository if needed (depending on flag computed on core)
            ImageSource::Build { source } => {
                if job.should_delete_shared_registry() {
                    logger.send_success("🪓 Terminating shared container registry of the job".to_string());
                    if let Err(err) = target
                        .container_registry
                        .delete_repository(source.image.shared_repository_name())
                    {
                        let user_msg =
                            format!("❌ Failed to delete shared container registry of the application: {err}");
                        logger.send_success(user_msg);
                    }
                }

                // Deleting the repository dedicated to this job
                logger.send_success("🪓 Terminating container registry of the job".to_string());
                if let Err(err) = target
                    .container_registry
                    .delete_repository(source.image.legacy_repository_name())
                {
                    let user_msg = format!("❌ Failed to delete container registry of the application: {err}");
                    logger.send_success(user_msg);
                }
            }
        };
    };

    (Box::new(pre_run), Box::new(task), Box::new(post_run))
}
