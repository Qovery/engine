use crate::constants::AWS_DEFAULT_REGION;
use crate::environment::action::deploy_database::{
    DB_READY_STATE, DB_STOPPED_STATE, get_managed_database_status, start_stop_managed_database,
};
use crate::environment::action::deploy_external_secrets::{
    clean_unused_secrets_generated_by_eso, uninstall_service_external_secret,
};
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::job::JobRunError;
use crate::environment::action::{DeploymentAction, log_job_output_error};
use crate::environment::models::abort::AbortStatus;
use crate::environment::models::terraform_service::{
    TerraformAction, TerraformService, TerraformServiceTrait, is_terraform_noop,
};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::environment::report::terraform_service::reporter::TerraformServiceDeploymentReporter;
use crate::environment::report::{DeploymentTaskMut, execute_long_deployment};
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::helm::{ChartInfo, ChartSetValue, HelmAction, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::service::{self, Action, Service};
use crate::infrastructure::models::cloud_provider::{self, DeploymentTarget};
use crate::io_models::terraform::{AdoptedDatabaseKind, ManagedDbConnectivity};
use crate::runtime::block_on;
use k8s_openapi::api::batch::v1::Job as K8sJob;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::api::core::v1::Secret;
use kube::Api;
use kube::api::{DeleteParams, ListParams};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

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
                deploy_managed_db_external_name(self, target, &event_details)?;
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

    // Nothing to stop in a run-to-completion job, unless it is an adopted a database that keeps billing.
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        report_action(self, target, Action::Pause, &mut |logger| {
            pause_adopted_managed_db(self, target, logger)
        })
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        // If the TerraformAction is Noop, we should delete terraform service related resources (job, helm, etc) without triggering terraform destroy
        let skip_terraform_destroy = is_terraform_noop(&self.terraform_action);

        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));

        let pre_run_event_details = event_details.clone();
        let mut pre_run: TerraformPreRun =
            Box::new(move |logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> {
                // A stopped instance rejects the modifications a destroy needs (final snapshot).
                ensure_adopted_managed_db_available(self, target, &pre_run_event_details, logger)?;
                Ok(TaskContext {})
            });
        let mut post_run: TerraformPostRun = Box::new(|_logger: &EnvSuccessLogger, _state: TaskContext| {
            // Delete external secrets helm release if exists
            uninstall_service_external_secret(&self.kube_name, self.long_id(), target);
        });

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
                    delete_managed_db_external_name(self, target, &event_details)?;
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
                    delete_managed_db_external_name(self, target, &event_details)?;
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

    // Restarting cannot re-run a completed job, but it must not leave an adopted database stopped.
    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Restart));
        report_action(self, target, Action::Restart, &mut |logger| {
            ensure_adopted_managed_db_available(self, target, &event_details, logger)
        })
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

        let _ = pod_tx.map(|tx| tx.send(pod).map_err(|e| e.to_string()));
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
        // CRITICAL: Must be called BEFORE await_job_to_complete() to avoid deadlock
        // (Job won't complete until sidecar terminates, and sidecar can't terminate until this is called)
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

        // STEP 3: Wait for job to complete (after sidecar has been terminated)
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
    terraform: &'a TerraformService<T>,
    target: &'a DeploymentTarget,
    event_details: EventDetails,
) -> TerraformPreRun<'a> {
    Box::new(move |logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> {
        // Before the apply: terraform cannot modify a stopped instance, so an apply that touches one
        // fails before any start is issued.
        ensure_adopted_managed_db_available(terraform, target, &event_details, logger)?;
        Ok(TaskContext {})
    })
}

pub(super) fn mk_deploy_post_run<'a, T: CloudProvider>(
    terraform: &'a TerraformService<T>,
    target: &'a DeploymentTarget,
) -> TerraformPostRun<'a>
where
    TerraformService<T>: TerraformServiceTrait,
{
    Box::new(move |_logger: &EnvSuccessLogger, _state: TaskContext| {
        // Clean unused secret generated by ESO
        let service_eso_secret_names: Vec<String> = terraform
            .external_secrets
            .iter()
            .map(|group| group.secret_name.to_string())
            .collect();
        clean_unused_secrets_generated_by_eso(
            target.kube.client(),
            target.environment.namespace(),
            &terraform.long_id,
            service_eso_secret_names,
        );
    })
}

/// Publishes the ExternalName a blueprint-adopted managed database used to own, so consumers keep
/// resolving the legacy in-cluster name once the `database` service is gone. No-op for every other
/// terraform service. The release name matches the one the database deployment used, so this updates
/// that release instead of racing a second one.
/// Wraps a short adopted-database action in the reporter, so pause and restart emit the per-service
/// events every other service does.
fn report_action<T: CloudProvider>(
    terraform: &TerraformService<T>,
    target: &DeploymentTarget,
    action: Action,
    body: &mut dyn FnMut(&EnvProgressLogger) -> Result<(), Box<EngineError>>,
) -> Result<(), Box<EngineError>>
where
    TerraformService<T>: ToTeraContext,
{
    let mut pre_run: TerraformPreRun = Box::new(|_| Ok(TaskContext {}));
    let mut run: TerraformRun = Box::new(|logger, state| {
        body(logger)?;
        Ok(state)
    });
    let mut post_run: TerraformPostRun = Box::new(|_, _| {});

    execute_long_deployment(
        TerraformServiceDeploymentReporter::without_terraform_job(terraform, target, action),
        DeploymentTaskMut {
            pre_run: &mut pre_run,
            run: &mut run,
            post_run_success: &mut post_run,
        },
    )
}

/// An adopted instance this engine may stop and start, with the region it actually lives in.
struct AdoptedInstance {
    id: String,
    db_type: service::DatabaseType,
    region: String,
}

/// The instance this service adopted, if it can be driven at all.
fn get_adopted_managed_db<T: CloudProvider>(
    terraform: &TerraformService<T>,
    target: &DeploymentTarget,
) -> Option<AdoptedInstance> {
    if target.cloud_provider.kind() != cloud_provider::Kind::Aws {
        return None;
    }

    stoppable_adopted_instance(terraform.managed_db_connectivity.as_ref()?)
}

/// The cloud-agnostic half of [adopted_instance]: what the payload itself says can be driven.
fn stoppable_adopted_instance(connectivity: &ManagedDbConnectivity) -> Option<AdoptedInstance> {
    let id = connectivity.instance_identifier.as_ref()?.trim().to_string();
    let db_type = match connectivity.database_kind.as_ref()? {
        AdoptedDatabaseKind::Postgresql => service::DatabaseType::PostgreSQL,
        AdoptedDatabaseKind::Mysql => service::DatabaseType::MySQL,
        AdoptedDatabaseKind::Mongodb => service::DatabaseType::MongoDB,
        // Elasticache has no stopped state, and an unknown kind is one this engine cannot drive.
        AdoptedDatabaseKind::Redis | AdoptedDatabaseKind::Unsupported => return None,
    };

    // `<id>.<hash>.<region>.rds.amazonaws.com` both corroborates the catalog-supplied identifier and
    // carries the region: the adopted instance need not live in the cluster's.
    let (endpoint_id, rest) = connectivity.target_hostname.split_once('.')?;
    if id.is_empty() || endpoint_id != id {
        return None;
    }
    let mut rest = rest.splitn(3, '.');
    let _hash = rest.next()?;
    let region = rest.next()?.to_string();
    if rest.next()? != "rds.amazonaws.com" {
        return None;
    }

    Some(AdoptedInstance { id, db_type, region })
}

fn aws_credentials<'a>(target: &'a DeploymentTarget, instance: &'a AdoptedInstance) -> Vec<(&'a str, &'a str)> {
    let mut credentials = target.cloud_provider.credentials_environment_variables();
    credentials.retain(|(key, _)| *key != AWS_DEFAULT_REGION);
    credentials.push((AWS_DEFAULT_REGION, instance.region.as_str()));
    credentials
}

/// Why a wait ended without the instance reaching the state.
enum WaitFailure {
    Aborted,
    TimedOut,
}

impl WaitFailure {
    fn describe(&self, instance_id: &str, state: &str) -> String {
        match self {
            WaitFailure::Aborted => format!("Aborted while waiting for database `{instance_id}` to be {state}"),
            WaitFailure::TimedOut => format!("Timed out waiting for database `{instance_id}` to be {state}"),
        }
    }
}

/// Polls until the instance reaches `state`, honouring the service's timeout and abort.
fn await_adopted_instance_state(
    instance: &AdoptedInstance,
    credentials: &[(&str, &str)],
    state: &str,
    timeout: Duration,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
) -> Result<(), WaitFailure> {
    let started = Instant::now();
    loop {
        // Distinct, so the message names the real cause.
        if target.abort.status() != AbortStatus::None {
            return Err(WaitFailure::Aborted);
        }
        if started.elapsed() >= timeout {
            return Err(WaitFailure::TimedOut);
        }

        match get_managed_database_status(instance.db_type, &instance.id, credentials) {
            Ok(current) if current == state => return Ok(()),
            Ok(current) => {
                logger.info(format!("Database `{}` is {current}, waiting for {state}", instance.id));
                thread::sleep(Duration::from_secs(30));
            }
            // A flaky describe must not fail a deployment.
            Err(_) => thread::sleep(Duration::from_secs(30)),
        }
    }
}

fn pause_adopted_managed_db<T: CloudProvider>(
    terraform: &TerraformService<T>,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>> {
    let Some(instance) = get_adopted_managed_db(terraform, target) else {
        return Ok(());
    };

    let credentials = aws_credentials(target, &instance);
    match get_managed_database_status(instance.db_type, &instance.id, &credentials) {
        Ok(status) if status == DB_READY_STATE => {}
        Ok(status) => {
            logger.info(format!("Database `{}` is {status}, nothing to stop", instance.id));
            return Ok(());
        }
        Err((_, msg)) => {
            logger.warning(format!(
                "Cannot read the state of database `{}`, leaving it alone: {msg}",
                instance.id
            ));
            return Ok(());
        }
    }

    logger.info(format!("Stopping the adopted database instance `{}`", instance.id));
    if let Err((_, msg)) = start_stop_managed_database(instance.db_type, &instance.id, &credentials, true) {
        logger.warning(format!("Could not stop database `{}`: {msg}", instance.id));
        return Ok(());
    }

    // Awaited: a deploy landing mid-stop finds an instance it can neither start nor use.
    if await_adopted_instance_state(&instance, &credentials, DB_STOPPED_STATE, terraform.timeout, target, logger)
        .is_err()
    {
        logger.warning(format!("Database `{}` did not reach {DB_STOPPED_STATE} in time", instance.id));
    }
    Ok(())
}

/// Starts the instance without waiting, so the apply that follows is not blocked behind it.
/// Waits for an instance started in pre-run: an apply reports done while the database is still coming up.
fn ensure_adopted_managed_db_available<T: CloudProvider>(
    terraform: &TerraformService<T>,
    target: &DeploymentTarget,
    event_details: &EventDetails,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>> {
    if !mutates_infrastructure(&terraform.terraform_action) || target.is_dry_run_deploy {
        return Ok(());
    }

    let Some(instance) = get_adopted_managed_db(terraform, target) else {
        return Ok(());
    };

    let credentials = aws_credentials(target, &instance);
    match get_managed_database_status(instance.db_type, &instance.id, &credentials) {
        Ok(status) if status == DB_READY_STATE => return Ok(()),
        Ok(_) | Err(_) => {
            logger.info(format!("Starting the adopted database instance `{}`", instance.id));
            let _ = start_stop_managed_database(instance.db_type, &instance.id, &credentials, false);
        }
    }

    await_adopted_instance_state(&instance, &credentials, DB_READY_STATE, terraform.timeout, target, logger).map_err(
        |failure| {
            Box::new(EngineError::new_database_failed_to_start_after_several_retries(
                event_details.clone(),
                instance.id.clone(),
                instance.db_type.to_string(),
                Some(CommandError::new_from_safe_message(
                    failure.describe(&instance.id, DB_READY_STATE),
                )),
            ))
        },
    )
}

/// Whether this run changes infrastructure. Plan-only, init, unlock and noop are reads and must not
/// stop or start anything; a destroy does change it, and needs the instance up to do so.
fn mutates_infrastructure(action: &TerraformAction) -> bool {
    match action {
        TerraformAction::TerraformPlanAndApply
        | TerraformAction::TerraformApplyFromPlan { .. }
        | TerraformAction::TerraformDestroy => true,
        TerraformAction::TerraformPlanOnly { .. }
        | TerraformAction::TerraformUnlockState
        | TerraformAction::TerraformInit
        | TerraformAction::TerraformNoop => false,
    }
}

fn deploy_managed_db_external_name<T: CloudProvider>(
    terraform: &TerraformService<T>,
    target: &DeploymentTarget,
    event_details: &EventDetails,
) -> Result<(), Box<EngineError>>
where
    TerraformService<T>: ToTeraContext,
{
    let Some(connectivity) = &terraform.managed_db_connectivity else {
        return Ok(());
    };

    // A plan-only run is a read: publishing here would write to the cluster the user asked us to
    // inspect. A noop has nothing to publish either.
    if !matches!(
        terraform.terraform_action,
        TerraformAction::TerraformPlanAndApply | TerraformAction::TerraformApplyFromPlan { .. }
    ) {
        return Ok(());
    }

    let mut tera_context = tera::Context::new();
    tera_context.insert("publicly_accessible", &connectivity.publicly_accessible);

    let values = vec![
        ChartSetValue {
            key: "target_hostname".to_string(),
            value: connectivity.target_hostname.to_string(),
        },
        ChartSetValue {
            key: "source_fqdn".to_string(),
            value: connectivity.source_fqdn.to_string(),
        },
        ChartSetValue {
            key: "service_name".to_string(),
            value: connectivity.service_name.to_string(),
        },
        ChartSetValue {
            key: "database_id".to_string(),
            value: connectivity.database_id.to_string(),
        },
        ChartSetValue {
            key: "database_long_id".to_string(),
            value: connectivity.database_long_id.to_string(),
        },
        ChartSetValue {
            key: "environment_id".to_string(),
            value: target.environment.id.to_string(),
        },
        ChartSetValue {
            key: "environment_long_id".to_string(),
            value: target.environment.long_id.to_string(),
        },
        ChartSetValue {
            key: "project_long_id".to_string(),
            value: target.environment.project_long_id.to_string(),
        },
        ChartSetValue {
            key: "publicly_accessible".to_string(),
            value: connectivity.publicly_accessible.to_string(),
        },
    ];

    let chart = ChartInfo {
        name: format!("{}-externalname", connectivity.service_name),
        path: format!("{}/external-name-svc", terraform.workspace_directory()),
        namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
        values,
        ..Default::default()
    };

    HelmDeployment::new(
        event_details.clone(),
        tera_context,
        PathBuf::from(terraform.helm_chart_external_name_service_dir()),
        None,
        chart,
    )
    .on_create(target)
}

/// Removes the ExternalName this service published for an adopted managed database. Without it the
/// Service outlives the RDS its terraform just destroyed, and a public DNS record keeps pointing at
/// a host that no longer exists.
fn delete_managed_db_external_name<T: CloudProvider>(
    terraform: &TerraformService<T>,
    target: &DeploymentTarget,
    event_details: &EventDetails,
) -> Result<(), Box<EngineError>>
where
    TerraformService<T>: ToTeraContext,
{
    let Some(connectivity) = &terraform.managed_db_connectivity else {
        return Ok(());
    };

    let chart = ChartInfo {
        name: format!("{}-externalname", connectivity.service_name),
        path: format!("{}/external-name-svc", terraform.workspace_directory()),
        namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
        action: HelmAction::Destroy,
        ..Default::default()
    };

    HelmDeployment::new(
        event_details.clone(),
        tera::Context::default(),
        PathBuf::from(terraform.helm_chart_external_name_service_dir()),
        None,
        chart,
    )
    .on_delete(target)
}

#[cfg(test)]
mod tests {
    use super::stoppable_adopted_instance;
    use crate::infrastructure::models::cloud_provider::service;
    use crate::io_models::terraform::{AdoptedDatabaseKind, ManagedDbConnectivity};
    use uuid::Uuid;

    fn connectivity(
        identifier: Option<&str>,
        kind: Option<AdoptedDatabaseKind>,
        hostname: &str,
    ) -> ManagedDbConnectivity {
        ManagedDbConnectivity {
            service_name: "z94067f00-postgresql".to_string(),
            target_hostname: hostname.to_string(),
            source_fqdn: "z94067f00-postgresql.example.com".to_string(),
            publicly_accessible: false,
            database_id: "z94067f00".to_string(),
            database_long_id: Uuid::new_v4(),
            instance_identifier: identifier.map(str::to_string),
            database_kind: kind,
        }
    }

    const ENDPOINT: &str = "prod-billing-db.cfx9tmpl.eu-west-3.rds.amazonaws.com";

    #[test]
    fn drives_an_adopted_instance_the_endpoint_corroborates() {
        // The real RDS id from the blueprint's db_identifier output, not the Qovery service name.
        let resolved = stoppable_adopted_instance(&connectivity(
            Some("prod-billing-db"),
            Some(AdoptedDatabaseKind::Postgresql),
            ENDPOINT,
        ))
        .expect("should resolve");

        assert_eq!(resolved.id, "prod-billing-db");
        assert_eq!(resolved.db_type, service::DatabaseType::PostgreSQL);
        // Taken from the endpoint, not the cluster: an adopted instance need not share the region.
        assert_eq!(resolved.region, "eu-west-3");
    }

    #[test]
    fn refuses_an_identifier_the_endpoint_does_not_corroborate() {
        // The identifier selects what gets stopped and comes from a customer-authored catalog output.
        assert!(
            stoppable_adopted_instance(&connectivity(
                Some("someone-elses-database"),
                Some(AdoptedDatabaseKind::Postgresql),
                ENDPOINT,
            ))
            .is_none()
        );
    }

    #[test]
    fn refuses_a_kind_with_no_stopped_state_or_none_this_engine_knows() {
        assert!(
            stoppable_adopted_instance(&connectivity(
                Some("prod-billing-db"),
                Some(AdoptedDatabaseKind::Redis),
                ENDPOINT
            ))
            .is_none()
        );
        // A kind a newer core invented: skip this service rather than guess at an AWS command.
        assert!(
            stoppable_adopted_instance(&connectivity(
                Some("prod-billing-db"),
                Some(AdoptedDatabaseKind::Unsupported),
                ENDPOINT,
            ))
            .is_none()
        );
    }

    #[test]
    fn refuses_a_payload_without_the_pause_fields() {
        assert!(
            stoppable_adopted_instance(&connectivity(None, Some(AdoptedDatabaseKind::Postgresql), ENDPOINT)).is_none()
        );
        assert!(stoppable_adopted_instance(&connectivity(Some("prod-billing-db"), None, ENDPOINT)).is_none());
        assert!(
            stoppable_adopted_instance(&connectivity(Some("   "), Some(AdoptedDatabaseKind::Postgresql), ENDPOINT))
                .is_none()
        );
    }

    #[test]
    fn a_payload_from_a_core_that_predates_these_fields_still_parses() {
        // Engines outlive the core: a block written before these fields must still deploy.
        let json = r#"{
            "service_name": "z94067f00-postgresql",
            "target_hostname": "prod-billing-db.cfx9tmpl.eu-west-3.rds.amazonaws.com",
            "source_fqdn": "z94067f00-postgresql.example.com",
            "publicly_accessible": false,
            "database_id": "z94067f00",
            "database_long_id": "94067f00-6524-4677-9353-d70d16d4957c"
        }"#;

        let connectivity: ManagedDbConnectivity = serde_json::from_str(json).expect("must parse");

        assert_eq!(connectivity.instance_identifier, None);
        assert_eq!(connectivity.database_kind, None);
        assert!(stoppable_adopted_instance(&connectivity).is_none());
    }

    #[test]
    fn an_unknown_kind_does_not_fail_the_whole_payload() {
        let kind: AdoptedDatabaseKind = serde_json::from_str("\"CLICKHOUSE\"").expect("must deserialize");
        assert_eq!(kind, AdoptedDatabaseKind::Unsupported);
    }
}
