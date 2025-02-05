use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::job_status;
use crate::environment::action::DeploymentAction;
use crate::environment::models::terraform_service::{TerraformService, TerraformServiceTrait};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::environment::report::terraform_service::reporter::TerraformServiceDeploymentReporter;
use crate::environment::report::{execute_long_deployment, DeploymentTaskImpl};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::runtime::block_on;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{AttachParams, DeleteParams, ListParams};
use kube::runtime::wait::await_condition;
use kube::Api;
use std::collections::HashSet;
use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for TerraformService<T>
where
    TerraformService<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let pre_run = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) };

        let run = |logger: &EnvProgressLogger, state: ()| -> Result<(), Box<EngineError>> {
            // We first need to delete the old job, because job spec cannot be updated (due to be an immutable resources)
            // But we can't uninstall the helm chart as we need to keep the persistent volume.
            delete_old_job_if_exist(self.kube_name(), &event_details, target)?;

            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom,
                custom_namespace: Some(target.environment.namespace().to_string()),
                timeout_in_seconds: self.startup_timeout().as_secs() as i64,
                k8s_selector: Some(self.kube_label_selector()),
                ..Default::default()
            };

            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

            // create job
            helm.on_create(target)?;

            // Get kube config file
            let job_pod_selector = format!("job-name={}", self.kube_name());
            let kube_pod_api: Api<Pod> = Api::namespaced(target.kube.clone(), target.environment.namespace());

            let mut set_of_pods_already_processed: HashSet<String> = HashSet::new();
            // loop {

            // Wait for the pod to be started to get its name
            let pod_name = crate::environment::action::deploy_job::get_active_job_pod_by_selector(
                kube_pod_api.clone(),
                &job_pod_selector,
                &event_details,
                &set_of_pods_already_processed,
                self.job_max_duration(),
            )?;
            set_of_pods_already_processed.insert(pod_name.clone());

            // Wait for the job container to be terminated
            logger.info(format!("Waiting for the job container {} to be processed...", self.kube_name()));
            block_on(async {
                tokio::select! {
                    biased;
                    _ = await_condition(
                        kube_pod_api.clone(),
                        &pod_name,
                        crate::environment::action::deploy_job::is_job_pod_container_terminated(self.kube_name()),
                    ) => {},
                }
            });

            info!("Write file in shared volume to let the waiting container terminate");
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

            // await_condition WILL NOT return an error if the job is not found, hence checking the job existence before
            info!("Get Jobs");
            block_on(jobs.get(self.kube_name())).map_err(|err| {
                EngineError::new_job_error(
                    event_details.clone(),
                    format!("Cannot get job {}: {}", self.kube_name(), err),
                )
            })?;
            info!("Wait for job to finish");
            let ret = block_on(await_condition(
                jobs,
                self.kube_name(),
                crate::environment::action::deploy_job::is_job_terminated(),
            ))
            .map_err(|_err| {
                EngineError::new_job_error(
                    event_details.clone(),
                    format!("Cannot find job for terminated pod {}", &pod_name),
                )
            })?;

            let job_status_result = match job_status(&ret.as_ref()) {
                crate::environment::action::deploy_job::JobStatus::Success => return Ok(state),
                crate::environment::action::deploy_job::JobStatus::NotRunning
                | crate::environment::action::deploy_job::JobStatus::Running => unreachable!(),
                crate::environment::action::deploy_job::JobStatus::Failure { reason, message } => {
                    let msg = format!("Job failed to correctly run due to {reason} {message}");
                    Err(EngineError::new_job_error(event_details.clone(), msg))
                }
            };

            // break; // TODO TF check if the pod should start only once
            // }

            Ok(job_status_result?)
        };

        let post_run = |_logger: &EnvSuccessLogger, _state: ()| {};

        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };

        execute_long_deployment(TerraformServiceDeploymentReporter::new(self, target, Action::Create), task)
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_pause().");
        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_delete().");
        Ok(())
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_restart().");
        Ok(())
    }
}

fn delete_old_job_if_exist(
    job_name: &str,
    event_details: &EventDetails,
    target: &DeploymentTarget,
) -> Result<(), Box<EngineError>> {
    let kube_job_api: Api<K8sJob> = Api::namespaced(target.kube.clone(), target.environment.namespace());

    let field_selector = format!("metadata.name={}", job_name);
    let jobs = block_on(kube_job_api.list(&ListParams::default().fields(&field_selector)))
        .map_err(|_err| EngineError::new_job_error(event_details.clone(), "Error when listing jobs".to_string()))?;

    if !jobs.items.is_empty() {
        block_on(kube_job_api.delete(job_name, &DeleteParams::background()))
            .map_err(|_err| EngineError::new_job_error(event_details.clone(), "Error when deleting job".to_string()))?;
    }

    Ok(())
}
