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
use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EventDetails, EventMessage, Stage};
use crate::io_models::job::JobSchedule;
use crate::models::job::{Job, JobService};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{AttachParams, ListParams, ObjectList};
use kube::runtime::wait::{await_condition, Condition};
use kube::Api;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::{Error, OperationResult};
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for Job<T>
where
    Job<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
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
                execute_long_deployment(job_reporter, |_logger: &EnvProgressLogger| -> Result<(), EngineError> {
                    Ok(())
                })
            }
        }
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
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
                execute_long_deployment(job_reporter, |_logger: &EnvProgressLogger| -> Result<(), EngineError> {
                    Ok(())
                })
            }
        }
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
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
}

struct TaskContext {
    last_deployed_image: Option<String>,
}

fn run_job<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
) -> (
    impl Fn(&EnvProgressLogger) -> Result<TaskContext, EngineError> + 'a,
    impl Fn(&EnvProgressLogger, TaskContext) -> Result<TaskContext, EngineError> + 'a,
    impl Fn(&EnvSuccessLogger, TaskContext) + 'a,
)
where
    Job<T>: JobService,
{
    let pre_run = move |logger: &EnvProgressLogger| -> Result<TaskContext, EngineError> {
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

        Ok(TaskContext {
            last_deployed_image: last_image,
        })
    };

    let task = move |_logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, EngineError> {
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
            // Get kube config file
            let kubernetes_config_file_path = match target.kubernetes.get_kubeconfig_file_path() {
                Ok(file_path) => file_path,
                Err(e) => {
                    let safe_message = "Error when retrieving the kubeconfig file";
                    target.kubernetes.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(
                            safe_message.to_string(),
                            Some(e.message(ErrorMessageVerbosity::FullDetails)),
                        ),
                    ));
                    "".to_string()
                }
            };
            let job_pod_selector = format!("job-name={}", job.kube_service_name());
            let kube_pod_api: Api<Pod> = Api::namespaced(target.kube.clone(), target.environment.namespace());

            // Wait for the pod to be started to get its name
            let list_job_pods = list_pods_by_selector(kube_pod_api.clone(), &job_pod_selector, event_details)?;
            let job_pod = list_job_pods.items.get(0).unwrap();
            let pod_name = job_pod.metadata.name.as_ref().unwrap();

            // Wait for the job container to be terminated
            _logger.info(format!(
                "Waiting for the job container {} to terminate...",
                job.kube_service_name()
            ));
            let _ = block_on(async {
                await_condition(
                    kube_pod_api.clone(),
                    pod_name,
                    is_job_pod_container_terminated(job.kube_service_name().as_str()),
                )
                .await
            });

            // Get JSON output from shared volume
            let result_json_output = kubectl_get_job_pod_output(
                kubernetes_config_file_path,
                target.kubernetes.cloud_provider().credentials_environment_variables(),
                target.environment.namespace(),
                pod_name,
            );
            match result_json_output {
                Ok(json) => {
                    _logger.info(format!("JSON output has been received: {}", json));
                    let result_serde_json: serde_json::Result<HashMap<String, JobOutputVariable>> =
                        serde_json::from_str(&json);
                    match result_serde_json {
                        Ok(deserialized_json) => _logger.core_configuration(
                            "Sending job output to create environment variables".to_string(),
                            serde_json::to_string(&deserialized_json).unwrap(),
                        ),
                        Err(_) => _logger.warning("Cannot parse JSON output".to_string()),
                    }
                }
                Err(err) => {
                    _logger.warning(format!(
                        "Cannot get JSON job output: {}",
                        err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)
                    ));
                }
            };

            // Write file in shared volume to let the waiting container terminate
            let exec_result = block_on(async {
                kube_pod_api
                    .clone()
                    .exec(
                        pod_name,
                        vec!["touch", "/output/terminate"],
                        &AttachParams::default().container("qovery-wait-container-output"),
                    )
                    .await
            });
            match exec_result {
                Ok(_) => {}
                Err(_) => {
                    return Err(EngineError::new_job_error(
                        event_details.clone(),
                        format!("Cannot create terminate file inside waiting container for pod {}", pod_name),
                    ))
                }
            }

            // wait for job to finish
            let jobs: Api<K8sJob> = Api::namespaced(target.kube.clone(), target.environment.namespace());
            let ret = block_on(async { await_condition(jobs, &job.kube_service_name(), is_job_terminated()).await });
            let ret = ret.unwrap();
            match job_status(&ret.as_ref()) {
                JobStatus::Success => Ok(()),
                JobStatus::NotRunning | JobStatus::Running => unreachable!(),
                JobStatus::Failure { reason, message } => {
                    let msg = format!("Job failed to correctly run due to {} {}", reason, message);
                    Err(EngineError::new_job_error(event_details.clone(), msg))
                }
            }?;
        }

        Ok(state)
    };

    let post_run = move |logger: &EnvSuccessLogger, state: TaskContext| {
        // Delete previous image from cache to cleanup resources
        if let Err(err) = delete_cached_image(job.tag_for_mirror(), state.last_deployed_image, false, target, logger) {
            error!("Failed to delete previous image from cache: {}", err);
        }
    };

    (pre_run, task, post_run)
}

fn delete_job<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
) -> (
    impl Fn(&EnvProgressLogger) -> Result<TaskContext, EngineError> + 'a,
    impl Fn(&EnvProgressLogger, TaskContext) -> Result<TaskContext, EngineError> + 'a,
    impl Fn(&EnvSuccessLogger, TaskContext) + 'a,
)
where
    Job<T>: JobService,
{
    let pre_run = move |_logger: &EnvProgressLogger| -> Result<TaskContext, EngineError> {
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

    let task = move |_logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, EngineError> {
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
        // Delete previous image from cache to cleanup resources
        if let Err(err) = delete_cached_image(job.tag_for_mirror(), state.last_deployed_image, false, target, logger) {
            error!("Failed to delete previous image from cache: {}", err);
        }
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

fn list_pods_by_selector(
    kube_pod_api: Api<Pod>,
    job_pod_selector: &str,
    event_details: &EventDetails,
) -> Result<ObjectList<Pod>, EngineError> {
    let list_job_pods_result = retry::retry(Fixed::from_millis(1000).take(5), || {
        match block_on(async { kube_pod_api.list(&ListParams::default().labels(job_pod_selector)).await }) {
            Ok(pods_list) => {
                if pods_list.items.is_empty() {
                    OperationResult::Retry(EngineError::new_job_error(
                        event_details.clone(),
                        format!("No pod found when listing pods having label {}", &job_pod_selector),
                    ))
                } else {
                    OperationResult::Ok(pods_list)
                }
            }
            Err(_) => OperationResult::Retry(EngineError::new_job_error(
                event_details.clone(),
                format!("Error when listing pods having label {} through Kube API", &job_pod_selector),
            )),
        }
    });
    match list_job_pods_result {
        Ok(pods) => Ok(pods),
        Err(Operation { error, .. }) => Err(error),
        Err(Error::Internal(message)) => Err(EngineError::new_job_error(
            event_details.clone(),
            format!(
                "Internal error when listing pods having label {} through Kube API: {}",
                &job_pod_selector, message
            ),
        )),
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
