use super::utils::delete_cached_image;
use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::utils::{KubeObjectKind, get_last_deployed_image, mirror_image_if_necessary};
use crate::environment::models::abort::Abort;
use crate::environment::models::job::{ImageSource, Job, JobService};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::job::reporter::JobDeploymentReporter;
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::environment::report::{DeploymentTaskImpl, execute_long_deployment};
use crate::errors::{CommandError, EngineError};
use crate::events::EngineEvent;
use crate::events::{EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::io_models::job::{JobSchedule, LifecycleType};
use crate::runtime::block_on;
use anyhow::{Context, anyhow};
use futures::pin_mut;
use itertools::Itertools;
use k8s_openapi::api::batch::v1::{CronJob, Job as K8sJob};
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::Api;
use kube::api::{AttachParams, ListParams, PostParams};
use kube::runtime::wait::{Condition, await_condition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

impl<T: CloudProvider> DeploymentAction for Job<T>
where
    Job<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(self.action().to_environment_step()));

        // Force job to run, if force trigger is requested
        let default = JobSchedule::OnStart {
            lifecycle_type: self.schedule().lifecycle_type().unwrap_or(LifecycleType::GENERIC),
        };
        let job_schedule = if self.should_force_trigger() {
            &default
        } else {
            self.schedule()
        };

        match job_schedule {
            JobSchedule::OnStart { .. } | JobSchedule::Cron { .. } => {
                let (pre_run, run, post_run) = run_job(self, target, &event_details);
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
                let (pre_run, run, post_run) = run_job(self, target, &event_details);
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
                let (pre_run, run, post_run) = run_job(self, target, &event_details);
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

struct TaskContext {
    last_deployed_image: Option<String>,
}

#[allow(clippy::type_complexity)]
fn run_job<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
) -> (
    impl Fn(&EnvProgressLogger) -> Result<TaskContext, Box<EngineError>> + 'a,
    impl Fn(&EnvProgressLogger, TaskContext) -> Result<TaskContext, Box<EngineError>> + 'a,
    impl Fn(&EnvSuccessLogger, TaskContext) + 'a,
)
where
    Job<T>: JobService,
{
    let metrics_registry = target.metrics_registry.clone();
    let pre_run = move |logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> {
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
    };

    let task = move |logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
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

        // Wait for the job to terminate in order to have his status
        // For cronjob we dont care as we don't control when it is executed
        if job.schedule().is_job() {
            // We first need to delete the old job, because job spec cannot be updated (due to be an immutable resources)
            helm.on_delete(target)?;

            // create job
            helm.on_create(target)?;

            let pod = block_on(await_user_job_to_terminate(
                job,
                target.environment.namespace(),
                target.kube.client(),
                target.abort,
            ));
            let pod =
                pod.map_err(|err| Box::new(EngineError::new_job_error(event_details.clone(), err.to_string())))?;
            let pod_name = pod.metadata.name.as_deref().unwrap_or("");
            info!("Targeting job pod name: {}", pod_name);

            // Fech Qovery Json output if any, and transmit it to the core for next deployment stage
            match block_on(retrieve_qovery_output_from_pod(
                target.kube.client(),
                target.environment.namespace(),
                pod_name,
            )) {
                Ok(None) => {}
                Ok(Some(output)) => logger.core_configuration_for_job(
                    "Job output succeeded. Environment variables will be synchronized.".to_string(),
                    serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string()),
                ),
                Err(err) => {
                    logger.log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::from(EngineError::new_invalid_job_output_cannot_be_serialized(
                            event_details.clone(),
                            err.to_string(),
                        )),
                    ));
                }
            }

            if let Err(err) = block_on(unstuck_qovery_output_waiter(
                target.kube.client(),
                target.environment.namespace(),
                pod_name,
            )) {
                warn!("Cannot unstuck qovery-output waiter: {}", err);
            }

            let job = block_on(await_job_to_complete(
                job,
                target.environment.namespace(),
                target.kube.client(),
                target.abort,
            ))
            .map_err(|err| Box::new(EngineError::new_job_error(event_details.clone(), err.to_string())))?;

            if let Some(ConditionStatus { reason, message }) = job_is_failed(&job) {
                let msg = format!("Job failed to correctly run due to {reason} {message}");
                debug!(msg);
                debug!("Job pod: {:?}", job);
                return Err(Box::new(EngineError::new_job_error(event_details.clone(), msg)));
            }

            // job completed successfully :party:
            return Ok(state);
        }

        // Cronjob will be installed
        if job.is_cron_job() && !job.is_force_trigger() {
            // create cronjob
            helm.on_create(target)?;
        }

        // Cronjob will be force triggered
        if job.is_cron_job() && job.is_force_trigger() {
            // check if cronjob is installed
            let k8s_cronjob_api: Api<CronJob> = Api::namespaced(target.kube.client(), target.environment.namespace());
            let cronjob_is_already_installed = block_on(k8s_cronjob_api.get(job.kube_name())).is_ok();

            // create cronjob
            helm.on_create(target)?;

            // Cronjob have been installed, in order to trigger it, we need to create a job from the cronjob manually.
            let k8s_job_api: Api<K8sJob> = Api::namespaced(target.kube.client(), target.environment.namespace());
            let cronjob = block_on(k8s_cronjob_api.get(job.kube_name())).map_err(|err| {
                EngineError::new_job_error(
                    event_details.clone(),
                    format!("Cannot get cronjob {}: {}", job.kube_name(), err),
                )
            })?;

            let mut job_template = cronjob.spec.expect("Cronjob should have a job_template").job_template;
            // For kube to automatically cleanup the job for us
            job_template
                .spec
                .as_mut()
                .expect("job_template should be editable")
                .ttl_seconds_after_finished = Some(10);
            // add a suffix in name to avoid conflict with jobs created by cronjob
            // truncate original name to 63 chars minus "-force-trigger" length to avoid k8s name length limit
            let job_name = format!("{}-force-trigger", job.kube_name().chars().take(63 - 14).join("").as_str(),);
            let job_to_start = K8sJob {
                metadata: ObjectMeta {
                    name: Some(job_name.to_string()),
                    ..job_template.metadata.expect("job_template should have metadata")
                },
                spec: job_template.spec,
                status: None,
            };
            block_on(k8s_job_api.create(&PostParams::default(), &job_to_start)).map_err(|err| {
                EngineError::new_job_error(event_details.clone(), format!("Cannot create job from cronjob: {err}"))
            })?;

            // FIXME(ENG-1942) correctly handle cancel
            let fut = async {
                match tokio::time::timeout(
                    // TODO is it the right duration to wait here? shouldn't we take the user-configured timeout value?
                    std::time::Duration::from_secs(3800), // We wait 1h + delta max for the job to be terminated
                    await_condition(k8s_job_api, &job_name, is_job_terminated()),
                )
                .await
                {
                    Ok(Ok(job_st)) => Ok(job_status(&job_st.as_ref())),
                    Ok(Err(err)) => Err(err),
                    Err(_) => Ok(JobStatus::Running), // timeout
                }
            };
            let job_status = block_on(fut).map_err(|_err| {
                EngineError::new_job_error(event_details.clone(), "Cannot find job for terminated pod".to_string())
            })?;

            let cronjob_result = match job_status {
                JobStatus::Success => Ok(()),
                JobStatus::Running => {
                    logger.info("Job is still running after 1h. Stopping waiting for it. Please check live-logs and service status to know its status".to_string());
                    Ok(())
                }
                JobStatus::NotRunning => {
                    let msg = "Job failed to correctly run due to `NotRunning`. This should not happen".to_string();
                    Err(EngineError::new_job_error(event_details.clone(), msg))
                }
                JobStatus::Failure { reason, message } => {
                    let msg = format!("Job failed to correctly run due to {reason} {message}");
                    Err(EngineError::new_job_error(event_details.clone(), msg))
                }
            };

            // uninstall cronjob if it was already present
            if !cronjob_is_already_installed {
                helm.on_delete(target)?;
            }

            // propagate if result is an error
            cronjob_result?;
        }

        Ok(state)
    };

    let post_run = move |logger: &EnvSuccessLogger, state: TaskContext| {
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
    };

    (pre_run, task, post_run)
}

async fn retrieve_qovery_output_from_pod(
    kube_client: kube::Client,
    namespace: &str,
    pod_name: &str,
) -> anyhow::Result<Option<HashMap<String, JobOutputVariable>>> {
    let pod_api: Api<Pod> = Api::namespaced(kube_client.clone(), namespace);

    // Write file in shared volume to let the waiting container terminate
    let mut process = pod_api
        .exec(
            pod_name,
            vec!["/qovery-job-output-waiter", "--display-output-file"],
            &AttachParams::default().container("qovery-wait-container-output"),
        )
        .await
        .with_context(|| format!("Cannot retrive qovery-output.json {}", &pod_name))?;

    let mut stdout = process
        .stdout()
        .with_context(|| format!("Cannot get stdout from waiting container for pod {}", &pod_name))?;

    // write stdout into buffer
    let mut buf = Vec::with_capacity(1024);
    tokio_util::io::read_buf(&mut stdout, &mut buf).await?;

    let Ok(_) = process.join().await else {
        debug!("No qovery JSON job output available");
        return Ok(None);
    };

    let json_str = String::from_utf8_lossy(&buf);
    if json_str.is_empty() {
        debug!("No qovery JSON job output available");
        return Ok(None);
    }

    let json = serialize_job_output(&json_str)
        .with_context(|| format!("qovery output json cannot be deserialized: {}", json_str))?
        .into_iter()
        .map(|(k, v)| (k.to_uppercase(), v))
        .collect();

    Ok(Some(json))
}

async fn unstuck_qovery_output_waiter(
    kube_client: kube::Client,
    namespace: &str,
    pod_name: &str,
) -> anyhow::Result<()> {
    info!("Write file in shared volume to let the waiting container terminate");

    let pod_api: Api<Pod> = Api::namespaced(kube_client.clone(), namespace);
    // Write file in shared volume to let the waiting container terminate
    pod_api
        .exec(
            pod_name,
            vec!["/qovery-job-output-waiter", "--terminate"],
            &AttachParams::default().container("qovery-wait-container-output"),
        )
        .await
        .with_context(|| format!("Cannot create terminate file inside waiting container for pod {}", &pod_name))?;

    Ok(())
}

async fn await_job_to_complete<T: CloudProvider>(
    qjob: &Job<T>,
    namespace: &str,
    kube_client: kube::Client,
    abort_handle: impl Abort,
) -> anyhow::Result<K8sJob>
where
    Job<T>: JobService,
{
    let job_api: Api<K8sJob> = Api::namespaced(kube_client.clone(), namespace);
    let max_execution_duration = Duration::from_secs(60) + qjob.max_duration * (qjob.max_nb_restart + 1);
    let execution_deadline = tokio::time::sleep_until(tokio::time::Instant::now() + max_execution_duration);
    pin_mut!(execution_deadline);

    let should_force_cancel = async || {
        while !abort_handle.status().should_force_cancel() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    let should_process_job = |job: Option<&K8sJob>| -> bool {
        let Some(job) = job else {
            return false;
        };

        if job_is_completed(job).is_some() {
            return true;
        }

        false
    };

    await_kube_condition(
        &mut execution_deadline,
        should_force_cancel,
        await_condition(job_api.clone(), qjob.kube_name(), should_process_job),
        max_execution_duration,
    )
    .await?
    .ok_or_else(|| anyhow!("Job not found"))
}

async fn await_user_job_to_terminate<T: CloudProvider>(
    qjob: &Job<T>,
    namespace: &str,
    kube_client: kube::Client,
    abort_handle: impl Abort,
) -> anyhow::Result<Pod>
where
    Job<T>: JobService,
{
    let job_api: Api<K8sJob> = Api::namespaced(kube_client.clone(), namespace);
    let pod_api: Api<Pod> = Api::namespaced(kube_client.clone(), namespace);
    let pod_selector = format!("job-name={}", qjob.kube_name());
    let max_execution_duration = Duration::from_secs(60) + qjob.max_duration * (qjob.max_nb_restart + 1);
    let execution_deadline = tokio::time::sleep_until(tokio::time::Instant::now() + max_execution_duration);
    pin_mut!(execution_deadline);

    let should_force_cancel = async || {
        while !abort_handle.status().should_force_cancel() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    let should_process_job = |job: Option<&K8sJob>| -> bool {
        let Some(job) = job else {
            return false;
        };

        if job_is_active(job) {
            return true;
        }

        if job_is_completed(job).is_some() {
            return true;
        }

        false
    };

    let should_process_pod = |pod: Option<&Pod>| -> bool {
        // if the pod is not present anymore, we want to unstuck the loop
        let Some(pod) = pod else {
            return true;
        };

        // We want the job to be half running. Meaning the task/job of the user is terminated
        // but our qovery-wait-container-output is still running, waiting for us.
        if user_job_terminated_exit_code(pod).is_some() {
            return true;
        }

        false
    };

    loop {
        // To avoid hot looping
        tokio::time::sleep(Duration::from_secs(1)).await;

        // We wait for an event of interest to be triggered
        let job = await_kube_condition(
            &mut execution_deadline,
            should_force_cancel,
            await_condition(job_api.clone(), qjob.kube_name(), should_process_job),
            max_execution_duration,
        )
        .await?
        .ok_or_else(|| anyhow!("Job not found"))?;

        if let Some(ConditionStatus { reason, message }) = job_is_failed(&job) {
            return Err(anyhow::anyhow!(
                "Job {} failed to run due to {} {}",
                qjob.kube_name(),
                reason,
                message
            ));
        }

        // fetch the correct pod
        let Ok(pod_name) = get_active_pod_of_job(pod_api.clone(), &pod_selector).await else {
            continue;
        };

        // wait for the pod to be completed
        let pod = await_kube_condition(
            &mut execution_deadline,
            should_force_cancel,
            await_condition(pod_api.clone(), &pod_name, should_process_pod),
            max_execution_duration,
        )
        .await?;

        // Pod is not present anymore, restart the loop
        let Some(pod) = pod else { continue };

        // user code has not terminated cleanly, restart the loop until backoff limit is reached
        if user_job_terminated_exit_code(&pod) != Some(0) {
            continue;
        }

        return Ok(pod);
    }
}

async fn await_kube_condition<T>(
    execution_deadline: &mut Pin<&mut tokio::time::Sleep>,
    await_force_cancel: impl AsyncFnOnce(),
    await_condition: impl Future<Output = Result<Option<T>, kube::runtime::wait::Error>>,
    max_duration: Duration,
) -> anyhow::Result<Option<T>> {
    tokio::select! {
        biased;
        _ = await_force_cancel() => {
            Err(anyhow::anyhow!(
            "Job execution has exceeded the maximum duration of {:?} seconds",
            max_duration
            ))
        },
        _ = execution_deadline => {
            Err(anyhow::anyhow!(
                "Job execution has exceeded the maximum duration of {:?} seconds",
                max_duration
            ))
        },
        kube_obj = await_condition => match kube_obj {
            Ok(obj) => Ok(obj),
            Err(err) => Err(anyhow!("Cannot get {}: {:?}", std::any::type_name::<T>(), err))
        }
    }
}

fn job_is_active(job: &K8sJob) -> bool {
    job.status.as_ref().and_then(|status| status.active).unwrap_or(0) as u32 > 0
}

struct ConditionStatus {
    pub reason: String,
    pub message: String,
}
fn job_is_failed(job: &K8sJob) -> Option<ConditionStatus> {
    // https://kubernetes.io/docs/concepts/workloads/controllers/job/#termination-of-job-pods
    job.status
        .as_ref()
        .and_then(|st| st.conditions.as_ref())
        .and_then(|conds| conds.iter().find(|c| &c.type_ == "FailureTarget"))
        .map(|c| ConditionStatus {
            reason: c.reason.clone().unwrap_or_default(),
            message: c.message.clone().unwrap_or_default(),
        })
}

fn job_is_completed(job: &K8sJob) -> Option<ConditionStatus> {
    // https://kubernetes.io/docs/concepts/workloads/controllers/job/#termination-of-job-pods
    job.status
        .as_ref()
        .and_then(|st| st.conditions.as_ref())
        .and_then(|conds| {
            conds
                .iter()
                .find(|c| &c.type_ == "FailureTarget" || &c.type_ == "SuccessCriteriaMet")
        })
        .map(|c| ConditionStatus {
            reason: c.reason.clone().unwrap_or_default(),
            message: c.message.clone().unwrap_or_default(),
        })
}

fn is_pod_running_or_pending(pod: &Pod) -> bool {
    let pod_phase = pod.status.as_ref().and_then(|s| s.phase.as_deref()).unwrap_or("");

    pod_phase == "Running" || pod_phase == "Pending"
}

fn user_job_terminated_exit_code(pod: &Pod) -> Option<u32> {
    pod.status
        .as_ref()
        .and_then(|st| st.container_statuses.as_ref())
        .map(|st| st.iter())
        .unwrap_or_default()
        .filter_map(|st| st.state.as_ref())
        .filter_map(|st| st.terminated.as_ref())
        .next()
        .map(|st| st.exit_code as u32)
}

async fn get_active_pod_of_job(pod_api: Api<Pod>, selector: &str) -> anyhow::Result<String> {
    let pods = pod_api.list(&ListParams::default().labels(selector)).await?;
    if pods.items.is_empty() {
        return Err(anyhow!("No pod found with this label selector {}", selector));
    }

    // Take only pods that are running or pending
    let mut active_job_pods: Vec<String> = pods
        .items
        .into_iter()
        .filter_map(|pod| {
            if is_pod_running_or_pending(&pod) {
                Some(pod.metadata.name?)
            } else {
                None
            }
        })
        .collect();

    match *active_job_pods.as_slice() {
        [] => Err(anyhow!("No pod found with this label selector {}", selector)),
        [_] => Ok(()),
        _ => Err(anyhow!(
            "More than one active pod found with this label selector {}. It should not be possible",
            selector
        )),
    }?;

    Ok(active_job_pods.remove(0))
}

#[allow(clippy::type_complexity)]
fn delete_job<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
) -> (
    impl Fn(&EnvProgressLogger) -> Result<TaskContext, Box<EngineError>> + 'a,
    impl Fn(&EnvProgressLogger, TaskContext) -> Result<TaskContext, Box<EngineError>> + 'a,
    impl Fn(&EnvSuccessLogger, TaskContext) + 'a,
)
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

    (pre_run, task, post_run)
}

pub enum JobStatus {
    NotRunning,
    Running,
    Success,
    Failure { reason: String, message: String },
}

pub fn job_status(job: &Option<&K8sJob>) -> JobStatus {
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
        // TODO (mzo) deadline exceeded ?
        return JobStatus::Running;
    }
    JobStatus::NotRunning
}

pub fn is_job_terminated() -> impl Condition<K8sJob> {
    |job: Option<&K8sJob>| match job_status(&job) {
        JobStatus::NotRunning => false,
        JobStatus::Running => false,
        JobStatus::Success => true,
        JobStatus::Failure { .. } => true,
    }
}

// Used to validate the job json output format with serde
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct JobOutputVariable {
    pub value: String,
    pub sensitive: bool,
    pub description: String,
}

impl Default for JobOutputVariable {
    fn default() -> Self {
        JobOutputVariable {
            value: String::new(),
            sensitive: true,
            description: String::new(),
        }
    }
}

pub fn serialize_job_output(json: &str) -> Result<HashMap<String, JobOutputVariable>, serde_json::Error> {
    let serde_hash_map: HashMap<&str, Value> = serde_json::from_str(json)?;
    let mut job_output_variables: HashMap<String, JobOutputVariable> = HashMap::new();

    for (key, value) in serde_hash_map {
        let job_output_variable_object = value.as_object();
        let job_output_variable_hashmap = match job_output_variable_object {
            None => continue,
            Some(hashmap) => hashmap,
        };

        let serde_value_default = &Value::default();
        let value = job_output_variable_hashmap.get("value").unwrap_or(serde_value_default);

        // Get job output 'value' as string or any other type
        let job_output_value = if value.is_string() {
            value.as_str().unwrap_or_default().to_string()
        } else {
            value.to_string()
        };
        let job_output_description = job_output_variable_hashmap
            .get("description")
            .unwrap_or(serde_value_default)
            .as_str()
            .unwrap_or_default()
            .to_string();

        job_output_variables.insert(
            key.to_string(),
            JobOutputVariable {
                value: job_output_value,
                sensitive: job_output_variable_hashmap
                    .get("sensitive")
                    .unwrap_or(serde_value_default)
                    .as_bool()
                    .unwrap_or(false),
                description: job_output_description,
            },
        );
    }
    Ok(job_output_variables)
}

#[cfg(test)]
mod test {
    use crate::environment::action::deploy_job::{JobOutputVariable, serialize_job_output};

    #[test]
    fn should_serialize_json_to_job_output_variable_with_string_value() {
        // given
        let json_output_with_string_values = r#"
        {"foo": { "value": "bar", "sensitive": true }, "foo_2": {"value": "bar_2"} }
        "#;

        // when
        let hashmap = serialize_job_output(json_output_with_string_values).unwrap();

        // then
        assert_eq!(
            hashmap.get("foo").unwrap(),
            &JobOutputVariable {
                value: "bar".to_string(),
                sensitive: true,
                description: "".to_string(),
            }
        );
        assert_eq!(
            hashmap.get("foo_2").unwrap(),
            &JobOutputVariable {
                value: "bar_2".to_string(),
                sensitive: false,
                description: "".to_string(),
            }
        );
    }

    #[test]
    fn should_serialize_json_to_job_output_variable_with_numeric_value() {
        // given
        let json_output_with_numeric_values = r#"
        {"foo": { "value": 123, "sensitive": true }, "foo_2": {"value": 123.456} }
        "#;

        // when
        let hashmap = serialize_job_output(json_output_with_numeric_values).unwrap();

        // then
        assert_eq!(
            hashmap.get("foo").unwrap(),
            &JobOutputVariable {
                value: "123".to_string(),
                sensitive: true,
                description: "".to_string(),
            }
        );
        assert_eq!(
            hashmap.get("foo_2").unwrap(),
            &JobOutputVariable {
                value: "123.456".to_string(),
                sensitive: false,
                description: "".to_string(),
            }
        );
        let json_final = serde_json::to_string(&hashmap).unwrap();
        println!("{json_final}");
    }

    #[test]
    fn should_serialize_json_to_job_output_variable_with_description() {
        // given
        let json_output_with_numeric_values = r#"
        {"foo": { "value": 123, "description": "a description" }}
        "#;

        // when
        let hashmap = serialize_job_output(json_output_with_numeric_values).unwrap();

        // then
        assert_eq!(
            hashmap.get("foo").unwrap(),
            &JobOutputVariable {
                value: "123".to_string(),
                sensitive: false,
                description: "a description".to_string(),
            }
        );
        let json_final = serde_json::to_string(&hashmap).unwrap();
        println!("{json_final}");
    }
}
