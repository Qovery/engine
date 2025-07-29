#![allow(unused_imports, unused_variables, dead_code)]

use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::job::JobRunError;
use crate::environment::action::deploy_job::job_output::JobOutputSerializationError;
use crate::environment::action::utils::{
    KubeObjectKind, delete_cached_image, get_last_deployed_image, mirror_image_if_necessary,
};
use crate::environment::models::job::{ImageSource, Job, JobService};
use crate::environment::models::terraform_service::{
    TerraformAction, TerraformFilesSource, TerraformService, TerraformServiceTrait,
};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::environment::report::terraform_service::reporter::TerraformServiceDeploymentReporter;
use crate::environment::report::{DeploymentTaskImpl, execute_long_deployment};
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::runtime::block_on;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::api::core::v1::Secret;
use kube::Api;
use kube::api::{DeleteParams, ListParams};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug)]
pub struct TaskContext {}
pub(super) type TerraformPreRun<'a> = Box<dyn Fn(&EnvProgressLogger) -> Result<TaskContext, Box<EngineError>> + 'a>;
pub(super) type TerraformPostRun<'a> = Box<dyn Fn(&EnvSuccessLogger, TaskContext) + 'a>;
pub(super) type TerraformRun<'a> =
    Box<dyn Fn(&EnvProgressLogger, TaskContext) -> Result<TaskContext, Box<EngineError>> + 'a>;

impl<T: CloudProvider> DeploymentAction for TerraformService<T>
where
    TerraformService<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let pre_run: TerraformPreRun = mk_deploy_pre_run(self, target, event_details.clone());
        let post_run: TerraformPostRun = mk_deploy_post_run(self, target);

        let (pod_tx, rx) = mpsc::sync_channel(1);
        let run: TerraformRun = Box::new(
            move |logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
                let task_ctx = self
                    .deploy_job_and_execute_cmd(target, &event_details, logger, state, pod_tx.clone())?
                    .0;
                Ok(task_ctx)
            },
        );

        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };

        execute_long_deployment(TerraformServiceDeploymentReporter::new(self, target, Action::Create, rx), task)
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));

        let command_error = CommandError::new_from_safe_message("Cannot pause a Terraform service".to_string());
        Err(Box::new(EngineError::new_cannot_restart_service(
            EventDetails::clone_changing_stage(event_details, Stage::Environment(EnvironmentStep::Pause)),
            target.environment.namespace(),
            "",
            command_error,
        )))
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        let pre_run: TerraformPreRun =
            Box::new(|_logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> { Ok(TaskContext {}) });
        let post_run: TerraformPostRun = Box::new(|_logger: &EnvSuccessLogger, _state: TaskContext| {});

        let (pod_tx, rx) = mpsc::sync_channel(1);
        let run: TerraformRun = Box::new(
            move |logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
                let (task, helm) =
                    self.deploy_job_and_execute_cmd(target, &event_details, logger, state, pod_tx.clone())?;
                helm.on_delete(target)?;
                Ok(task)
            },
        );

        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };

        execute_long_deployment(TerraformServiceDeploymentReporter::new(self, target, Action::Delete, rx), task)
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Restart));

        let command_error = CommandError::new_from_safe_message("Cannot restart a Terraform service".to_string());
        Err(Box::new(EngineError::new_cannot_restart_service(
            EventDetails::clone_changing_stage(event_details, Stage::Environment(EnvironmentStep::Restart)),
            target.environment.namespace(),
            "",
            command_error,
        )))
    }
}

impl<T: CloudProvider> TerraformService<T>
where
    TerraformService<T>: ToTeraContext,
{
    fn deploy_job_and_execute_cmd(
        &self,
        target: &DeploymentTarget,
        event_details: &EventDetails,
        logger: &EnvProgressLogger,
        state: TaskContext,
        pod_tx: mpsc::SyncSender<Pod>,
    ) -> Result<(TaskContext, HelmDeployment), Box<EngineError>> {
        let handle_error = |err: JobRunError| -> Box<EngineError> {
            match err {
                JobRunError::Aborted => {
                    // if cancel/abort has been requested, we want to kill/send a sigterm to the job
                    // To notify it to terminate
                    let _ = block_on(super::deploy_job::job::kill_job(
                        target.kube.client(),
                        target.environment.namespace(),
                        self.kube_name(),
                    ));
                    Box::new(EngineError::new_task_cancellation_requested(event_details.clone()))
                }
                _ => Box::new(EngineError::new_job_error(event_details.clone(), err.to_string())),
            }
        };

        // We first need to delete the old job, because job spec cannot be updated (due to be an immutable resources)
        // But we can't uninstall the helm chart as we need to keep the persistent volume.
        delete_old_job_if_exist(self.kube_name(), event_details, target)?;

        let chart = ChartInfo {
            name: self.helm_release_name(),
            path: self.workspace_directory().to_string(),
            namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
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

        let _backend_config_secret_cleanup = scopeguard::guard(&self.backend.kube_secret_name, |secret_name| {
            // to be sure we unstuck the reporter
            let _ = pod_tx.send(Pod::default());

            info!("Removing secret: {:?}", secret_name);
            let _ = delete_backend_config_secret(secret_name, event_details, target);
        });

        let job_pod_selector = format!("job-name={}", self.kube_name());
        let max_execution_duration = Duration::from_secs(60) + self.timeout;
        let pod = block_on(super::deploy_job::job::await_job_pod_to_start(
            self.kube_name(),
            max_execution_duration,
            target.environment.namespace(),
            target.kube.client(),
            target.abort,
        ))
        .map_err(handle_error)?;
        let _ = pod_tx.send(pod);
        let pod = block_on(super::deploy_job::job::await_job_pod_to_terminate(
            self.kube_name(),
            max_execution_duration,
            target.environment.namespace(),
            target.kube.client(),
            target.abort,
        ))
        .map_err(handle_error)?;

        let pod_name = pod.metadata.name.unwrap_or_default();
        info!("Targeting job pod name: {}", pod_name);

        match self.terraform_action {
            TerraformAction::TerraformPlanOnly { execution_id: _ } | TerraformAction::TerraformDestroy => {}
            TerraformAction::TerraformApplyFromPlan { execution_id: _ } | TerraformAction::TerraformPlanAndApply => {
                match block_on(super::deploy_job::job::retrieve_output_and_terminate_pod(
                    target.kube.client(),
                    target.environment.namespace(),
                    &pod_name,
                    "^[a-zA-Z_][a-zA-Z0-9_]*$",
                )) {
                    Ok(None) => {}
                    Ok(Some(output)) => logger.core_configuration_for_terraform_service(
                        "Terraform output succeeded. Environment variables will be synchronized.".to_string(),
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
                                EngineError::new_invalid_job_output_cannot_be_serialized(
                                    event_details.clone(),
                                    err.to_string(),
                                )
                            }),
                        ));
                    }
                }
            }
        }

        let job = block_on(crate::environment::action::deploy_job::job::await_job_to_complete(
            self.kube_name(),
            max_execution_duration,
            target.environment.namespace(),
            target.kube.client(),
            target.abort,
        ))
        .map_err(|err| Box::new(EngineError::new_job_error(event_details.clone(), err.to_string())))?;

        if let Some(crate::environment::action::deploy_job::job::ConditionStatus { reason, message }) =
            crate::environment::action::deploy_job::job::job_is_failed(&job)
        {
            let msg = format!("Job failed to correctly run due to {reason} {message}");
            debug!(msg);
            debug!("Job pod: {:?}", job);
            return Err(Box::new(EngineError::new_job_error(event_details.clone(), msg)));
        }

        Ok((state, helm))
    }
}

fn delete_old_job_if_exist(
    job_name: &str,
    event_details: &EventDetails,
    target: &DeploymentTarget,
) -> Result<(), Box<EngineError>> {
    let kube_job_api: Api<K8sJob> = Api::namespaced(target.kube.client(), target.environment.namespace());

    let field_selector = format!("metadata.name={job_name}");
    let jobs = block_on(kube_job_api.list(&ListParams::default().fields(&field_selector)))
        .map_err(|_err| EngineError::new_job_error(event_details.clone(), "Error when listing jobs".to_string()))?;

    if !jobs.items.is_empty() {
        block_on(kube_job_api.delete(job_name, &DeleteParams::background()))
            .map_err(|_err| EngineError::new_job_error(event_details.clone(), "Error when deleting job".to_string()))?;
    }

    Ok(())
}

fn delete_backend_config_secret(
    secret_name: &str,
    event_details: &EventDetails,
    target: &DeploymentTarget,
) -> Result<(), Box<EngineError>> {
    let kube_secret_api: Api<Secret> = Api::namespaced(target.kube.client(), target.environment.namespace());

    let field_selector = format!("metadata.name={secret_name}");
    let secrets = block_on(kube_secret_api.list(&ListParams::default().fields(&field_selector)))
        .map_err(|_err| EngineError::new_job_error(event_details.clone(), "Error when listing secrets".to_string()))?;

    if !secrets.items.is_empty() {
        block_on(kube_secret_api.delete(secret_name, &DeleteParams::background())).map_err(|_err| {
            EngineError::new_job_error(event_details.clone(), "Error when deleting secret".to_string())
        })?;
    }

    Ok(())
}

pub(super) fn mk_deploy_pre_run<'a, T: CloudProvider>(
    terraform: &'a TerraformService<T>,
    target: &'a DeploymentTarget,
    event_details: EventDetails,
) -> TerraformPreRun<'a> {
    Box::new(move |logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> { Ok(TaskContext {}) })
}

pub(super) fn mk_deploy_post_run<'a, T: CloudProvider>(
    terraform: &'a TerraformService<T>,
    target: &'a DeploymentTarget,
) -> TerraformPostRun<'a>
where
    TerraformService<T>: TerraformServiceTrait,
{
    Box::new(move |logger: &EnvSuccessLogger, state: TaskContext| {})
}
