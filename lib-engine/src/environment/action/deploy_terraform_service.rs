use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::job::JobRunError;
use crate::environment::action::{DeploymentAction, log_job_output_error};
use crate::environment::models::terraform_service::{TerraformAction, TerraformService, TerraformServiceTrait};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::environment::report::terraform_service::reporter::TerraformServiceDeploymentReporter;
use crate::environment::report::{DeploymentTaskMut, execute_long_deployment};
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::runtime::block_on;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::api::core::v1::Secret;
use kube::Api;
use kube::api::{DeleteParams, ListParams};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug)]
pub struct TaskContext {}
pub(super) type TerraformPreRun<'a> = Box<dyn FnMut(&EnvProgressLogger) -> Result<TaskContext, Box<EngineError>> + 'a>;
pub(super) type TerraformPostRun<'a> = Box<dyn FnMut(&EnvSuccessLogger, TaskContext) + 'a>;
pub(super) type TerraformRun<'a> =
    Box<dyn FnMut(&EnvProgressLogger, TaskContext) -> Result<TaskContext, Box<EngineError>> + 'a>;

impl<T: CloudProvider> DeploymentAction for TerraformService<T>
where
    TerraformService<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let mut pre_run: TerraformPreRun = mk_deploy_pre_run(self, target, event_details.clone());
        let mut post_run: TerraformPostRun = mk_deploy_post_run(self, target);

        let (mut pod_tx, rx) = {
            let (tx, rx) = mpsc::sync_channel(1);
            // we put it into an optional to be able to take the value out of the closure
            // Effectively, it simulates a FnOnce(). This allow for the receiver to be notified when pod_tx is dropped.
            // So the reporter can be unstuck
            (Some(tx), rx)
        };

        let mut run: TerraformRun = Box::new(
            move |logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
                let task_ctx = self
                    .deploy_job_and_execute_cmd(target, &event_details, logger, state, pod_tx.take())?
                    .0;
                Ok(task_ctx)
            },
        );

        let task = DeploymentTaskMut {
            pre_run: &mut pre_run,
            run: &mut run,
            post_run_success: &mut post_run,
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
        // If the TerraformAction is Noop, we should delete terraform service related resources (job, helm, etc) without triggering terraform destroy
        let skip_terraform_destroy = matches!(self.terraform_action, TerraformAction::TerraformNoop);

        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));

        let mut pre_run: TerraformPreRun =
            Box::new(|_logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> { Ok(TaskContext {}) });
        let mut post_run: TerraformPostRun = Box::new(|_logger: &EnvSuccessLogger, _state: TaskContext| {});

        let (mut pod_tx, rx) = {
            let (tx, rx) = mpsc::sync_channel(1);
            // we put it into an optional to be able to take the value out of the closure
            // Effectively, it simulates a FnOnce(). This allow for the receiver to be notified when pod_tx is dropped.
            // So the reporter can be unstuck
            (Some(tx), rx)
        };

        let mut run: TerraformRun = if skip_terraform_destroy {
            // For skip_terraform_destroy case: only cleanup job and helm without terraform execution
            Box::new(
                move |_logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
                    let helm = self.delete_job_and_cleanup(target, &event_details)?;
                    // Uninstall helm release
                    helm.on_delete(target)?;
                    // Drop the sender to signal no pod will be sent
                    drop(pod_tx.take());
                    Ok(state)
                },
            )
        } else {
            // For normal case: deploy job and execute terraform destroy
            Box::new(
                move |logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
                    let (task, helm) =
                        self.deploy_job_and_execute_cmd(target, &event_details, logger, state, pod_tx.take())?;
                    helm.on_delete(target)?;
                    Ok(task)
                },
            )
        };

        let task = DeploymentTaskMut {
            pre_run: &mut pre_run,
            run: &mut run,
            post_run_success: &mut post_run,
        };

        execute_long_deployment(
            TerraformServiceDeploymentReporter::new_with_skip_terraform_job_execution(
                self,
                target,
                Action::Delete,
                rx,
                skip_terraform_destroy,
            ),
            task,
        )
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
    fn delete_job_and_cleanup(
        &self,
        target: &DeploymentTarget,
        event_details: &EventDetails,
    ) -> Result<HelmDeployment, Box<EngineError>> {
        // Ensure old job is deleted if exists
        delete_old_job_if_exist(self.kube_name(), event_details, target)?;

        // Prepare HelmDeployment to uninstall
        let helm = self.helm_deployment(target, event_details)?;

        // Cleanup backend config secret
        let _backend_config_secret_cleanup = scopeguard::guard(&self.backend.kube_secret_name, |secret_name| {
            info!("Removing secret: {:?}", secret_name);
            let _ = delete_backend_config_secret(secret_name, event_details, target);
        });

        Ok(helm)
    }

    fn deploy_job_and_execute_cmd(
        &self,
        target: &DeploymentTarget,
        event_details: &EventDetails,
        logger: &EnvProgressLogger,
        state: TaskContext,
        pod_tx: Option<mpsc::SyncSender<Pod>>,
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

        let helm = self.helm_deployment(target, event_details)?;

        // create job
        helm.on_create(target)?;

        let _backend_config_secret_cleanup = scopeguard::guard(&self.backend.kube_secret_name, |secret_name| {
            info!("Removing secret: {:?}", secret_name);
            let _ = delete_backend_config_secret(secret_name, event_details, target);
        });

        let max_execution_duration = Duration::from_secs(60) + self.timeout;
        let pod = block_on(super::deploy_job::job::await_job_pod_to_start(
            self.kube_name(),
            max_execution_duration,
            target.environment.namespace(),
            target.kube.client(),
            target.abort,
        ))
        .map_err(handle_error)?;
        // pod is available: as of today, we consider the status can be set to Executing
        logger.switch_to_executing_step();

        let _ = pod_tx.map(|tx| tx.send(pod));
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

        // STEP 1: Retrieve terraform resources first (before terminating the container)
        let should_retrieve_terraform_resources_and_output = matches!(
            self.terraform_action,
            TerraformAction::TerraformApplyFromPlan { execution_id: _ } | TerraformAction::TerraformPlanAndApply
        );

        if should_retrieve_terraform_resources_and_output {
            match block_on(super::deploy_job::job::retrieve_terraform_resources(
                target.kube.client(),
                target.environment.namespace(),
                &pod_name,
            )) {
                Ok(Some(resources_json)) => {
                    // Parse and filter resources using TerraformResourceParser
                    use crate::engine_task::qovery_api::TerraformResourcesRequest;
                    use crate::environment::resource_extraction::parser::TerraformResourceParser;

                    let parser = TerraformResourceParser::new();
                    match parser.parse_from_value(resources_json) {
                        Ok(resources) => {
                            let resource_count = resources.len();
                            info!(
                                "Successfully extracted {} terraform resource(s) from deployment",
                                resource_count
                            );

                            // Send resources to core via gRPC instead of logging
                            let request = TerraformResourcesRequest {
                                terraform_id: *self.long_id(),
                                execution_id: event_details.execution_id().to_string(),
                                resources,
                            };

                            match target.qovery_api.send_terraform_resources(&request) {
                                Ok(()) => {
                                    info!("Successfully sent {} terraform resources to core via gRPC", resource_count);
                                }
                                Err(e) => {
                                    info!("Warning: Failed to send terraform resources via gRPC (non-fatal): {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            info!("Warning: Failed to parse terraform resources (non-fatal): {}", e);
                        }
                    }
                }
                Ok(None) => {
                    debug!("No terraform resources available");
                }
                Err(e) => {
                    info!("Warning: Failed to retrieve terraform resources (non-fatal): {}", e);
                }
            }
        }

        // STEP 2: Retrieve terraform outputs and terminate the waiting container
        if should_retrieve_terraform_resources_and_output {
            match block_on(super::deploy_job::job::retrieve_output_and_terminate_pod(
                target.kube.client(),
                target.environment.namespace(),
                &pod_name,
                "^[a-zA-Z_][a-zA-Z0-9_]*$",
            )) {
                Ok(Some(output)) => {
                    logger.core_configuration_for_terraform_service(
                        "Terraform output succeeded. Environment variables will be synchronized.".to_string(),
                        serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string()),
                    );
                }
                Ok(None) => {}
                Err(err) => {
                    log_job_output_error(logger, event_details, err);
                }
            }
        }

        Ok((state, helm))
    }

    fn helm_deployment(
        &self,
        target: &DeploymentTarget,
        event_details: &EventDetails,
    ) -> Result<HelmDeployment, Box<EngineError>> {
        let chart = self.build_chart(target);
        Ok(HelmDeployment::new(
            event_details.clone(),
            self.to_tera_context(target)?,
            PathBuf::from(self.helm_chart_dir()),
            None,
            chart,
        ))
    }

    fn build_chart(&self, target: &DeploymentTarget) -> ChartInfo {
        ChartInfo {
            name: self.helm_release_name(),
            path: self.workspace_directory().to_string(),
            namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
            timeout_in_seconds: self.startup_timeout().as_secs() as i64,
            k8s_selector: Some(self.kube_label_selector()),
            ..Default::default()
        }
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
    _terraform: &'a TerraformService<T>,
    _target: &'a DeploymentTarget,
    _event_details: EventDetails,
) -> TerraformPreRun<'a> {
    Box::new(move |_logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> { Ok(TaskContext {}) })
}

pub(super) fn mk_deploy_post_run<'a, T: CloudProvider>(
    _terraform: &'a TerraformService<T>,
    _target: &'a DeploymentTarget,
) -> TerraformPostRun<'a>
where
    TerraformService<T>: TerraformServiceTrait,
{
    Box::new(move |_logger: &EnvSuccessLogger, _state: TaskContext| {})
}
