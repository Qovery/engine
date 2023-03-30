use super::utils::delete_cached_image;
use crate::cloud_provider::helm::{ChartInfo, HelmChartNamespaces};
use crate::cloud_provider::service::{Action, Service};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::kubectl::kubectl_get_job_pod_output;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::utils::{get_last_deployed_image, mirror_image, KubeObjectKind};
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::job::reporter::JobDeploymentReporter;
use crate::deployment_report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::deployment_report::{execute_long_deployment, DeploymentTaskImpl};
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::io_models::job::JobSchedule;
use crate::models::job::{ImageSource, Job, JobService};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use k8s_openapi::api::batch::v1::{CronJob, Job as K8sJob};
use k8s_openapi::api::core::v1::Pod;
use kube::api::{AttachParams, ListParams, PostParams};
use kube::runtime::wait::{await_condition, Condition};
use kube::Api;
use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::{Error, OperationResult};
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for Job<T>
where
    Job<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(self.action().to_environment_step()));

        // Force job to run, if force trigger is requested
        let job_schedule = if self.should_force_trigger() {
            &JobSchedule::OnStart {}
        } else {
            self.schedule()
        };

        match job_schedule {
            JobSchedule::OnStart {} | JobSchedule::Cron { .. } => {
                let (pre_run, run, post_run) = run_job(self, target, &event_details);
                let task = DeploymentTaskImpl {
                    pre_run: &pre_run,
                    run: &run,
                    post_run_success: &post_run,
                };

                execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Create), task)
            }
            JobSchedule::OnPause {} | JobSchedule::OnDelete {} => {
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
            JobSchedule::OnPause {} => {
                let (pre_run, run, post_run) = run_job(self, target, &event_details);
                let task = DeploymentTaskImpl {
                    pre_run: &pre_run,
                    run: &run,
                    post_run_success: &post_run,
                };

                execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Pause), task)
            }
            JobSchedule::OnStart {} | JobSchedule::OnDelete {} => {
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
            JobSchedule::OnDelete {} => {
                let (pre_run, run, post_run) = run_job(self, target, &event_details);
                let task = DeploymentTaskImpl {
                    pre_run: &pre_run,
                    run: &run,
                    post_run_success: &post_run,
                };

                execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Delete), task)
            }
            JobSchedule::Cron { .. } | JobSchedule::OnStart {} | JobSchedule::OnPause {} => Ok(()),
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
        return Err(Box::new(EngineError::new_cannot_restart_service(
            self.get_event_details(Stage::Environment(EnvironmentStep::Restart)),
            target.environment.namespace(),
            &self.selector(),
            command_error,
        )));
    }
}

struct TaskContext {
    last_deployed_image: Option<String>,
}

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
    let pre_run = move |logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> {
        match &job.image_source {
            // If image come from a registry, we mirror it to the cluster registry in order to avoid losing access to it due to creds expiration
            ImageSource::Registry { source } => {
                mirror_image(
                    job.long_id(),
                    &source.registry,
                    &source.image,
                    &source.tag,
                    source.tag_for_mirror(job.long_id()),
                    target,
                    logger,
                    event_details.clone(),
                )?;
            }
            ImageSource::Build { .. } => {}
        }

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

        if job.schedule().is_job() {
            // We first need to delete the old job, because job spec cannot be updated (due to be an immutable resources)
            helm.on_delete(target)?;
        }

        // Wait for the job to terminate in order to have his status
        // For cronjob we dont care as we don't control when it is executed
        if job.schedule().is_job() {
            // create job
            helm.on_create(target)?;

            // Get kube config file
            let kubernetes_config_file_path = target.kubernetes.get_kubeconfig_file_path()?;
            let job_pod_selector = format!("job-name={}", job.kube_service_name());
            let kube_pod_api: Api<Pod> = Api::namespaced(target.kube.clone(), target.environment.namespace());

            let job_max_nb_restart = job.max_nb_restart();
            let mut job_creation_iterations = 0;
            let mut set_of_pods_already_processed: HashSet<String> = HashSet::new();

            loop {
                // Wait for the pod to be started to get its name
                let pod_name = get_active_job_pod_by_selector(
                    kube_pod_api.clone(),
                    &job_pod_selector,
                    event_details,
                    &set_of_pods_already_processed,
                )?;
                set_of_pods_already_processed.insert(pod_name.clone());

                // Wait for the job container to be terminated
                logger.info(format!(
                    "Waiting for the job container {} to be processed...",
                    job.kube_service_name()
                ));
                let _ = block_on(await_condition(
                    kube_pod_api.clone(),
                    &pod_name,
                    is_job_pod_container_terminated(job.kube_service_name().as_str()),
                ));

                // Get JSON output from shared volume
                let result_json_output = kubectl_get_job_pod_output(
                    kubernetes_config_file_path.clone(),
                    target.cloud_provider.credentials_environment_variables(),
                    target.environment.namespace(),
                    &pod_name,
                );
                match result_json_output {
                    Ok(json) => {
                        let result_serde_json: serde_json::Result<HashMap<String, JobOutputVariable>> =
                            serde_json::from_str(&json);
                        match result_serde_json {
                            Ok(deserialized_json_hashmap) => {
                                let deserialized_json_hashmap_with_uppercase_keys: HashMap<String, JobOutputVariable> =
                                    deserialized_json_hashmap
                                        .iter()
                                        .map(|(key, value)| (key.to_uppercase(), value.clone()))
                                        .collect();
                                logger.core_configuration(
                                    "Job output succeeded. Environment variables will be synchronized.".to_string(),
                                    serde_json::to_string(&deserialized_json_hashmap_with_uppercase_keys)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                )
                            }
                            Err(_) => logger.warning("Cannot parse JSON output".to_string()),
                        }
                    }
                    Err(err) => {
                        logger.warning(format!(
                            "Cannot get JSON job output: {}",
                            err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)
                        ));
                    }
                };

                // Write file in shared volume to let the waiting container terminate
                block_on(kube_pod_api.clone().exec(
                    &pod_name,
                    vec!["touch", "/qovery-output/terminate"],
                    &AttachParams::default().container("qovery-wait-container-output"),
                ))
                .map_err(|_err| {
                    EngineError::new_job_error(
                        event_details.clone(),
                        format!("Cannot create terminate file inside waiting container for pod {}", &pod_name),
                    )
                })?;

                // wait for job to finish
                let jobs: Api<K8sJob> = Api::namespaced(target.kube.clone(), target.environment.namespace());
                let ret =
                    block_on(await_condition(jobs, &job.kube_service_name(), is_job_terminated())).map_err(|_err| {
                        EngineError::new_job_error(
                            event_details.clone(),
                            format!("Cannot find job for terminated pod {}", &pod_name),
                        )
                    })?;
                let job_status_result = match job_status(&ret.as_ref()) {
                    JobStatus::Success => return Ok(state),
                    JobStatus::NotRunning | JobStatus::Running => unreachable!(),
                    JobStatus::Failure { reason, message } => {
                        let msg = format!("Job failed to correctly run due to {reason} {message}");
                        Err(EngineError::new_job_error(event_details.clone(), msg))
                    }
                };

                // If job has restarted the maximum time, then return the result that should be an Err
                if job_creation_iterations == job_max_nb_restart {
                    job_status_result?;
                }
                job_creation_iterations += 1;
            }
        }

        // Cronjob will be installed
        if job.is_cron_job() && !job.is_force_trigger() {
            // create cronjob
            helm.on_create(target)?;
        }

        // Cronjob will be force triggered
        if job.is_cron_job() && job.is_force_trigger() {
            // check if cronjob is installed
            let k8s_cronjob_api: Api<CronJob> = Api::namespaced(target.kube.clone(), target.environment.namespace());
            let cronjob_is_already_installed = block_on(k8s_cronjob_api.get(&job.kube_service_name())).is_ok();

            // create cronjob
            helm.on_create(target)?;

            // Cronjob have been installed, in order to trigger it, we need to create a job from the cronjob manually.
            let k8s_job_api: Api<K8sJob> = Api::namespaced(target.kube.clone(), target.environment.namespace());
            let cronjob = block_on(k8s_cronjob_api.get(&job.kube_service_name())).map_err(|err| {
                EngineError::new_job_error(
                    event_details.clone(),
                    format!("Cannot get cronjob {}: {}", job.kube_service_name(), err),
                )
            })?;

            let mut job_template = cronjob.spec.unwrap().job_template;
            // For kube to automatically cleanup the job for us
            job_template.spec.as_mut().unwrap().ttl_seconds_after_finished = Some(10);
            let job_to_start = K8sJob {
                metadata: job_template.metadata.unwrap(),
                spec: job_template.spec,
                status: None,
            };
            block_on(k8s_job_api.create(&PostParams::default(), &job_to_start)).map_err(|err| {
                EngineError::new_job_error(event_details.clone(), format!("Cannot create job from cronjob: {err}"))
            })?;

            let job = block_on(await_condition(k8s_job_api, &job.kube_service_name(), is_job_terminated())).map_err(
                |_err| {
                    EngineError::new_job_error(event_details.clone(), "Cannot find job for terminated pod".to_string())
                },
            )?;

            let cronjob_result = match job_status(&job.as_ref()) {
                JobStatus::Success => Ok(()),
                JobStatus::NotRunning | JobStatus::Running => unreachable!(),
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
                if let Err(err) = delete_cached_image(
                    job.long_id(),
                    mirrored_image_tag,
                    state.last_deployed_image,
                    false,
                    target,
                    logger,
                ) {
                    error!("Failed to delete previous image from cache: {}", err);
                }
            }
            ImageSource::Build { .. } => {}
        };
    };

    (pre_run, task, post_run)
}

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
            target.kube.clone(),
            &job.selector(),
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
                    false,
                    target,
                    logger,
                ) {
                    let user_msg = format!("Failed to delete previous image from cache: {err}");
                    logger.send_success(user_msg);
                }
            }

            // Deleting the repository dedicated to this job
            ImageSource::Build { source } => {
                logger.send_success("🪓 Terminating container registry of the job".to_string());
                if let Err(err) = target
                    .container_registry
                    .delete_repository(source.image.repository_name())
                {
                    let user_msg = format!("❌ Failed to delete container registry of the application: {err}");
                    logger.send_success(user_msg);
                }
            }
        };
    };

    (pre_run, task, post_run)
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
        // TODO (mzo) deadline exceeded ?
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

fn get_active_job_pod_by_selector(
    kube_pod_api: Api<Pod>,
    job_pod_selector: &str,
    event_details: &EventDetails,
    set_of_pods_already_processed: &HashSet<String>,
) -> Result<String, Box<EngineError>> {
    let list_job_pods_result = retry::retry(Fibonacci::from_millis(1000).take(10), || {
        // List pods according to job label selector
        let pods = match block_on(kube_pod_api.list(&ListParams::default().labels(job_pod_selector))) {
            Ok(pods_list) => {
                if pods_list.items.is_empty() {
                    return OperationResult::Retry(EngineError::new_job_error(
                        event_details.clone(),
                        format!("No pod found when listing pods having label {}", &job_pod_selector),
                    ));
                } else {
                    pods_list
                }
            }
            Err(_) => {
                return OperationResult::Retry(EngineError::new_job_error(
                    event_details.clone(),
                    format!("Error when listing pods having label {} through Kube API", &job_pod_selector),
                ))
            }
        };
        // Retrieve active pods
        let active_job_pods: Vec<String> = pods
            .items
            .iter()
            .filter_map(|pod| {
                if let Some(pod_status) = &pod.status {
                    if let Some(pod_container_statuses) = &pod_status.container_statuses {
                        let job_container_is_active = &pod_container_statuses
                            .iter()
                            .filter_map(|container_status| container_status.borrow().clone().state)
                            .any(|status| status.running.is_some());
                        if *job_container_is_active {
                            return Some(pod.metadata.name.as_ref().unwrap().clone());
                        }
                    }
                }
                None
            })
            .collect();

        // There should never be more than 1 pod in 'Running' status
        let active_selected_pod_name = match active_job_pods.len() {
            1 => active_job_pods.get(0).unwrap().to_string(),
            _ => {
                return OperationResult::Retry(EngineError::new_job_error(
                    event_details.clone(),
                    format!("Cannot find active pod having label {}", &job_pod_selector),
                ))
            }
        };

        // Check that the selected running pod has not already been processed
        if set_of_pods_already_processed.contains(&active_selected_pod_name) {
            return OperationResult::Retry(EngineError::new_job_error(
                event_details.clone(),
                format!(
                    "Selected pod has already been processed. Waiting for the next pod to be created having label {}",
                    &job_pod_selector
                ),
            ));
        }
        OperationResult::Ok(active_selected_pod_name)
    });
    match list_job_pods_result {
        Ok(active_pod_name) => Ok(active_pod_name),
        Err(Operation { error, .. }) => Err(Box::new(error)),
        Err(Error::Internal(message)) => Err(Box::new(EngineError::new_job_error(
            event_details.clone(),
            format!(
                "Internal error when listing pods having label {} through Kube API: {}",
                &job_pod_selector, message
            ),
        ))),
    }
}

fn job_pod_container_status_is_terminated(job_pod: &Option<&Pod>, job_container_name: &str) -> bool {
    if let Some(pod) = job_pod {
        if let Some(pod_status) = &pod.status {
            if let Some(pod_container_statuses) = &pod_status.container_statuses {
                let job_container_terminated = &pod_container_statuses
                    .iter()
                    .filter(|container_status| container_status.name == job_container_name)
                    .filter_map(|container_status| container_status.borrow().clone().state)
                    .any(|status| status.terminated.is_some());
                return *job_container_terminated;
            }
        }
    }
    false
}

fn is_job_pod_container_terminated(job_container_name: &str) -> impl Condition<Pod> + '_ {
    move |job_pod: Option<&Pod>| job_pod_container_status_is_terminated(&job_pod, job_container_name)
}

// Used to validate the job json output format with serde
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
struct JobOutputVariable {
    pub value: String,
    pub sensitive: bool,
}

impl Default for JobOutputVariable {
    fn default() -> Self {
        JobOutputVariable {
            value: String::new(),
            sensitive: true,
        }
    }
}
