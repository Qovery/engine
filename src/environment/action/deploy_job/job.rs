use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::action::{JobRun, TaskContext};
use crate::environment::action::deploy_job::job_output::{
    JobOutputSerializationError, JobOutputVariable, serialize_job_output,
};
use crate::environment::models::abort::Abort;
use crate::environment::models::job::{Job, JobService};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::logger::EnvProgressLogger;
use crate::errors::EngineError;
use crate::events::EngineEvent;
use crate::events::{EventDetails, EventMessage};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Service;
use crate::runtime::block_on;
use anyhow::{Context, anyhow};
use futures::pin_mut;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::{AttachParams, DeleteParams, ListParams};
use kube::runtime::wait::await_condition;
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;
use tokio::io::AsyncReadExt;

#[derive(thiserror::Error, Debug)]
pub enum JobRunError {
    #[error("Job aborted due to user cancel request")]
    Aborted,

    #[error("Job terminated due to timeout: {raw_error_message:?}")]
    Timeout { raw_error_message: String },

    #[error("Job terminated due to an error: {0}")]
    Unknown(#[from] anyhow::Error),
}

pub(super) fn mk_deploy_job_run<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
) -> JobRun<'a>
where
    Job<T>: JobService,
{
    Box::new(
        move |logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
            run_job_task(job, target, event_details, logger, state)
        },
    )
}

fn run_job_task<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
    logger: &EnvProgressLogger,
    state: TaskContext,
) -> Result<TaskContext, Box<EngineError>>
where
    Job<T>: JobService,
{
    let chart = ChartInfo {
        name: job.helm_release_name(),
        path: job.workspace_directory().to_string(),
        namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
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

    // Wait for the job to terminate in order to have its status
    // For cronjob we dont care as we don't control when it is executed
    // We first need to delete the old job, because job spec cannot be updated (due to be an immutable resources)
    helm.on_delete(target)?;

    // create job
    helm.on_create(target)?;

    let job_name = job.kube_name();
    let max_execution_duration = Duration::from_secs(60) + job.max_duration * (job.max_nb_restart + 1);
    let pod = block_on(await_user_job_to_terminate(
        job.kube_name(),
        max_execution_duration,
        target.environment.namespace(),
        target.kube.client(),
        target.abort,
    ));
    let pod_name = match pod {
        Ok(pod) => pod.metadata.name.unwrap_or_default(),
        Err(JobRunError::Aborted) => {
            let _ = block_on(kill_job(target.kube.client(), target.environment.namespace(), job_name));
            return Err(Box::new(EngineError::new_task_cancellation_requested(event_details.clone())));
        }
        Err(err) => return Err(Box::new(EngineError::new_job_error(event_details.clone(), err.to_string()))),
    };
    info!("Targeting job pod name: {}", pod_name);

    // Fetch Qovery Json output if any, and transmit it to the core for next deployment stage
    match block_on(retrieve_output_and_terminate_pod(
        target.kube.client(),
        target.environment.namespace(),
        &pod_name,
        job.output_variable_validation_pattern.as_str(),
    )) {
        Ok(None) => {}
        Ok(Some(output)) => logger.core_configuration_for_job(
            "Job output succeeded. Environment variables will be synchronized.".to_string(),
            serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string()),
        ),
        Err(err) => {
            logger.log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::from(if err.to_string().contains("Validation error") {
                    EngineError::new_invalid_job_output_variable_validation_failed(
                        event_details.clone(),
                        err.to_string(),
                    )
                } else {
                    EngineError::new_invalid_job_output_cannot_be_serialized(event_details.clone(), err.to_string())
                }),
            ));
        }
    }

    let job = block_on(await_job_to_complete(
        job.kube_name(),
        max_execution_duration,
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
    Ok(state)
}

pub async fn retrieve_output_and_terminate_pod(
    kube_client: kube::Client,
    namespace: &str,
    pod_name: &str,
    output_variable_validation_pattern: &str,
) -> Result<Option<HashMap<String, JobOutputVariable>>, JobRunError> {
    let pod_api: Api<Pod> = Api::namespaced(kube_client.clone(), namespace);

    // Write file in shared volume to let the waiting container terminate
    let mut process = pod_api
        .exec(
            pod_name,
            vec!["/qovery-job-output-waiter", "--display-output-file", "--terminate"],
            &AttachParams::default().container("qovery-wait-container-output"),
        )
        .await
        .with_context(|| format!("Cannot retrieve qovery-output.json {}", &pod_name))?;

    let mut stdout = process
        .stdout()
        .with_context(|| format!("Cannot get stdout from waiting container for pod {}", &pod_name))?;

    // write stdout into buffer
    let mut json_str = Vec::with_capacity(4096);
    stdout
        .read_to_end(&mut json_str)
        .await
        .with_context(|| "cannot read stdout from qovery-job-output-waiter container")?;

    let Ok(_) = process.join().await else {
        debug!("No qovery JSON job output available");
        return Ok(None);
    };

    if json_str.is_empty() {
        debug!("No qovery JSON job output available");
        return Ok(None);
    }

    let json = serialize_job_output(&json_str, output_variable_validation_pattern)
        .map_err(|err| match err {
            JobOutputSerializationError::SerializationError { serde_err } => {
                anyhow::anyhow!(format!(
                    "qovery output json cannot be deserialized: {} {}",
                    serde_err,
                    String::from_utf8_lossy(&json_str)
                ))
            }
            JobOutputSerializationError::OutputVariableValidationError { err } => {
                anyhow::anyhow!("Validation error on job output json: {}", err)
            }
        })?
        .into_iter()
        .map(|(k, v)| (k.to_uppercase(), v))
        .collect::<HashMap<String, JobOutputVariable>>();

    Ok(Some(json))
}

pub async fn kill_job(kube_client: kube::Client, namespace: &str, job_name: &str) -> anyhow::Result<()> {
    info!("Killing job {} from namespace {}", job_name, namespace);
    let pod_api: Api<K8sJob> = Api::namespaced(kube_client.clone(), namespace);
    let _ = pod_api.delete(job_name, &DeleteParams::foreground()).await?;

    Ok(())
}

pub async fn await_job_to_complete(
    job_name: &str,
    max_execution_duration: Duration,
    namespace: &str,
    kube_client: kube::Client,
    abort_handle: impl Abort,
) -> Result<K8sJob, JobRunError> {
    let job_api: Api<K8sJob> = Api::namespaced(kube_client.clone(), namespace);
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
        await_condition(job_api.clone(), job_name, should_process_job),
        max_execution_duration,
    )
    .await?
    .ok_or_else(|| JobRunError::Unknown(anyhow!("Job not found")))
}

pub async fn await_user_job_to_terminate(
    job_name: &str,
    max_execution_duration: Duration,
    namespace: &str,
    kube_client: kube::Client,
    abort_handle: impl Abort,
) -> Result<Pod, JobRunError> {
    let job_api: Api<K8sJob> = Api::namespaced(kube_client.clone(), namespace);
    let pod_api: Api<Pod> = Api::namespaced(kube_client.clone(), namespace);
    let pod_selector = format!("job-name={job_name}");
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
            await_condition(job_api.clone(), job_name, should_process_job),
            max_execution_duration,
        )
        .await?
        .ok_or_else(|| anyhow!("Job not found"))?;

        if let Some(ConditionStatus { reason, message }) = job_is_failed(&job) {
            return Err(JobRunError::Unknown(anyhow::anyhow!(
                "Job {} failed to run due to {} {}",
                job_name,
                reason,
                message
            )));
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
) -> Result<Option<T>, JobRunError> {
    tokio::select! {
        biased;
        _ = await_force_cancel() => {
            Err(JobRunError::Aborted)
        },
        _ = execution_deadline => {
            Err(JobRunError::Timeout {
                    raw_error_message: format!( "Job execution has exceeded the maximum duration of {max_duration:?} seconds" )
                }
            )
        },
        kube_obj = await_condition => match kube_obj {
            Ok(obj) => Ok(obj),
            Err(err) => Err(JobRunError::Unknown(anyhow!("Cannot get {}: {:?}", std::any::type_name::<T>(), err)))
        }
    }
}

fn job_is_active(job: &K8sJob) -> bool {
    job.status.as_ref().and_then(|status| status.active).unwrap_or(0) as u32 > 0
}

pub struct ConditionStatus {
    pub reason: String,
    pub message: String,
}
pub fn job_is_failed(job: &K8sJob) -> Option<ConditionStatus> {
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
