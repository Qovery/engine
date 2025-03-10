use crate::cmd::kubectl::kubectl_get_job_pod_output;
use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::job_status;
use crate::environment::models::terraform_service::{TerraformAction, TerraformService, TerraformServiceTrait};
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
use kube::api::{AttachParams, DeleteParams, ListParams};
use kube::runtime::wait::await_condition;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for TerraformService<T>
where
    TerraformService<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let pre_run = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) };

        let run = |logger: &EnvProgressLogger, state: ()| -> Result<(), Box<EngineError>> {
            self.deploy_job_and_execute_cmd(target, &event_details, logger, state, false)
        };

        let post_run = |_logger: &EnvSuccessLogger, _state: ()| {};

        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };

        execute_long_deployment(TerraformServiceDeploymentReporter::new(self, target, Action::Create), task)
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Restart));

        let command_error = CommandError::new_from_safe_message("Cannot pause a Terraform service".to_string());
        Err(Box::new(EngineError::new_cannot_restart_service(
            EventDetails::clone_changing_stage(event_details, Stage::Environment(EnvironmentStep::Restart)),
            target.environment.namespace(),
            "",
            command_error,
        )))
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let pre_run = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) };
        let post_run = |_logger: &EnvSuccessLogger, _state: ()| {};
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let run = |logger: &EnvProgressLogger, state: ()| -> Result<(), Box<EngineError>> {
            self.deploy_job_and_execute_cmd(target, &event_details, logger, state, true)
        };

        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };

        execute_long_deployment(TerraformServiceDeploymentReporter::new(self, target, Action::Delete), task)
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
        state: (),
        uninstall_helm: bool,
    ) -> Result<(), Box<EngineError>> {
        // We first need to delete the old job, because job spec cannot be updated (due to be an immutable resources)
        // But we can't uninstall the helm chart as we need to keep the persistent volume.
        delete_old_job_if_exist(self.kube_name(), event_details, target)?;

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

        let _backend_config_secret_cleanup = scopeguard::guard(&self.backend.kube_secret_name, |secret_name| {
            info!("Removing secret: {:?}", secret_name);
            let _ = delete_backend_config_secret(secret_name, event_details, target);
        });

        let job_pod_selector = format!("job-name={}", self.kube_name());
        let kube_pod_api: Api<Pod> = Api::namespaced(target.kube.client(), target.environment.namespace());

        let mut set_of_pods_already_processed: HashSet<String> = HashSet::new();

        // Wait for the pod to be started to get its name
        let pod_name = crate::environment::action::deploy_job::get_active_job_pod_by_selector(
            kube_pod_api.clone(),
            &job_pod_selector,
            event_details,
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

        // read json output
        match self.terraform_action {
            TerraformAction::TerraformPlanOnly { execution_id: _ } | TerraformAction::TerraformDestroy => {}
            TerraformAction::TerraformApplyFromPlan { execution_id: _ } | TerraformAction::TerraformPlanAndApply => {
                retrieve_terraform_output(target, logger, event_details, &pod_name)?;
            }
        }

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
        let jobs: Api<K8sJob> = Api::namespaced(target.kube.client(), target.environment.namespace());

        // await_condition WILL NOT return an error if the job is not found, hence checking the job existence before
        info!("Get Jobs");
        block_on(jobs.get(self.kube_name())).map_err(|err| {
            EngineError::new_job_error(event_details.clone(), format!("Cannot get job {}: {}", self.kube_name(), err))
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

        match job_status(&ret.as_ref()) {
            crate::environment::action::deploy_job::JobStatus::Success => return Ok(state),
            crate::environment::action::deploy_job::JobStatus::NotRunning
            | crate::environment::action::deploy_job::JobStatus::Running => unreachable!(),
            crate::environment::action::deploy_job::JobStatus::Failure { reason, message } => {
                let msg = format!("Job failed to correctly run due to {reason} {message}");
                Err(EngineError::new_job_error(event_details.clone(), msg))
            }
        }?;
        // TODO TF check if the pod should start only once

        // delete helm
        if uninstall_helm {
            helm.on_delete(target)?;
        }

        Ok(())
    }
}

fn delete_old_job_if_exist(
    job_name: &str,
    event_details: &EventDetails,
    target: &DeploymentTarget,
) -> Result<(), Box<EngineError>> {
    let kube_job_api: Api<K8sJob> = Api::namespaced(target.kube.client(), target.environment.namespace());

    let field_selector = format!("metadata.name={}", job_name);
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

    let field_selector = format!("metadata.name={}", secret_name);
    let secrets = block_on(kube_secret_api.list(&ListParams::default().fields(&field_selector)))
        .map_err(|_err| EngineError::new_job_error(event_details.clone(), "Error when listing secrets".to_string()))?;

    if !secrets.items.is_empty() {
        block_on(kube_secret_api.delete(secret_name, &DeleteParams::background())).map_err(|_err| {
            EngineError::new_job_error(event_details.clone(), "Error when deleting secret".to_string())
        })?;
    }

    Ok(())
}

fn retrieve_terraform_output(
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
    event_details: &EventDetails,
    pod_name: &str,
) -> Result<(), Box<EngineError>> {
    info!("Get JSON output from shared volume");
    let result_json_output = kubectl_get_job_pod_output(
        target.kubernetes.kubeconfig_local_file_path(),
        target.cloud_provider.credentials_environment_variables(),
        target.environment.namespace(),
        pod_name,
    );
    match result_json_output {
        Ok(json) => {
            let result_serde_json: Result<
                HashMap<String, crate::environment::action::deploy_job::JobOutputVariable>,
                serde_json::Error,
            > = crate::environment::action::deploy_job::serialize_job_output(&json);
            match result_serde_json {
                Ok(deserialized_json_hashmap) => {
                    let deserialized_json_hashmap_with_uppercase_keys: HashMap<
                        String,
                        crate::environment::action::deploy_job::JobOutputVariable,
                    > = deserialized_json_hashmap
                        .iter()
                        .map(|(key, value)| (key.to_uppercase(), value.clone()))
                        .collect();
                    logger.core_configuration_for_terraform_service(
                        "TerraformService output succeeded. Environment variables will be synchronized.".to_string(),
                        serde_json::to_string(&deserialized_json_hashmap_with_uppercase_keys)
                            .unwrap_or_else(|_| "{}".to_string()),
                    )
                }
                Err(err) => {
                    logger.log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::from(EngineError::new_invalid_job_output_cannot_be_serialized(
                            event_details.clone(),
                            err,
                            &json,
                        )),
                    ));
                }
            }
        }
        Err(err) => {
            info!(
                "Cannot get JSON output: {}",
                err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)
            );
        }
    };
    Ok(())
}
