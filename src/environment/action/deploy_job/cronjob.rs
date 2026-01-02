use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::action::{JobPostRun, JobPreRun, JobRun, TaskContext};
use crate::environment::action::deploy_job::common::{mk_deploy_post_run, mk_deploy_pre_run};
use crate::environment::action::deploy_job::job::ConditionStatus;
use crate::environment::models::job::{Job, JobService};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::logger::EnvProgressLogger;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Service;
use crate::runtime::block_on;
use itertools::Itertools;
use k8s_openapi::api::batch::v1::{CronJob, Job as K8sJob};
use kube::Api;
use kube::api::PostParams;
use std::path::PathBuf;
use std::time::Duration;

const DEPLOYMENT_ID_LABEL: &str = "qovery.com/deployment-id";

pub(super) fn run_cronjob<'a, T: CloudProvider>(
    job: &'a Job<T>,
    target: &'a DeploymentTarget,
    event_details: &'a EventDetails,
) -> (JobPreRun<'a>, JobRun<'a>, JobPostRun<'a>)
where
    Job<T>: JobService,
{
    let task = move |_logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
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

        // simple case when the job is not force-trigger
        // Only install the helm chart
        if !job.is_force_trigger() {
            helm.on_create(target)?;
            return Ok(state);
        }

        // Cronjob requested to be force triggered

        // check if cronjob is installed to know if we need to uninstall it at the end
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
            .ttl_seconds_after_finished = Some(30);
        // add a suffix in name to avoid conflict with jobs created by cronjob
        // truncate original name to 63 chars minus "-force-trigger" length to avoid k8s name length limit
        let job_name = format!("{}-force-trigger", job.kube_name().chars().take(63 - 14).join("").as_str(),);
        let mut metadata = job_template.metadata.expect("job_template should have metadata");
        metadata.name = Some(job_name.clone());
        metadata
            .labels
            .get_or_insert_default()
            .insert(DEPLOYMENT_ID_LABEL.to_string(), job.deployment_id.clone());
        job_template
            .spec
            .as_mut()
            .and_then(|s| s.template.metadata.as_mut())
            .and_then(|m| m.labels.as_mut())
            .and_then(|labels| labels.insert(DEPLOYMENT_ID_LABEL.to_string(), job.deployment_id.clone()));
        let job_to_start = K8sJob {
            metadata,
            spec: job_template.spec,
            status: None,
        };
        block_on(k8s_job_api.create(&PostParams::default(), &job_to_start)).map_err(|err| {
            EngineError::new_job_error(event_details.clone(), format!("Cannot create job from cronjob: {err}"))
        })?;

        let max_execution_duration = Duration::from_secs(60) + job.max_duration * (job.max_nb_restart + 1);
        let job = block_on(super::job::await_job_to_complete(
            &job_name,
            max_execution_duration,
            target.environment.namespace(),
            target.kube.client(),
            target.abort,
        ));
        let job = match job {
            Ok(job) => job,
            Err(super::job::JobRunError::Aborted) => {
                return Err(Box::new(EngineError::new_task_cancellation_requested(event_details.clone())));
            }
            Err(err) => return Err(Box::new(EngineError::new_job_error(event_details.clone(), err.to_string()))),
        };

        let cronjob_result = match super::job::job_is_failed(&job) {
            None => Ok(()),
            Some(ConditionStatus { reason, message }) => {
                let msg = format!("Job failed to correctly run due to {reason} {message}");
                Err(EngineError::new_job_error(event_details.clone(), msg))
            }
        };

        // uninstall cronjob if it was not present is the first place
        if !cronjob_is_already_installed {
            helm.on_delete(target)?;
        }

        // propagate if result is an error
        cronjob_result?;
        Ok(state)
    };

    let pre_run = mk_deploy_pre_run(job, target, event_details);
    let post_run = mk_deploy_post_run(job, target);
    (pre_run, Box::new(task), post_run)
}
