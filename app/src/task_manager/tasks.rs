use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::{env, fs};

use chrono::{DateTime, Utc};
use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::cmd::docker::Docker;
use qovery_engine::engine::EngineConfigError;
use url::Url;
use uuid::Uuid;

use qovery_engine::errors;
use qovery_engine::events::Stage::Infrastructure;
use qovery_engine::events::{
    EngineEvent, EnvironmentStep, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter,
};
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::logger::Logger;
use qovery_engine::object_storage::errors::ObjectStorageError;
use qovery_engine::transaction::{StepName, Transaction, TransactionResult};

use crate::task_manager::models::{Action, Archive, EngineRequest};
use crate::task_manager::scheduler::Task;
use qovery_engine::object_storage::ObjectStorage;
use qovery_engine::transaction::StepName::Waiting;

#[derive(Clone)]
pub struct InfrastructureTask {
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<Url>,
    docker: Docker,
    request: EngineRequest,
    logger: Box<dyn Logger>,
}

impl InfrastructureTask {
    pub fn new(
        request: EngineRequest,
        workspace_root_dir: String,
        lib_root_dir: String,
        docker_host: Option<Url>,
        logger: Box<dyn Logger>,
    ) -> Self {
        let docker = Docker::new(docker_host.clone()).expect("Can't init docker builder");
        InfrastructureTask {
            workspace_root_dir,
            lib_root_dir,
            docker_host,
            docker,
            request,
            logger,
        }
    }

    fn info_context(&self) -> Context {
        Context::new(
            self.request.organization_long_id,
            self.request.cloud_provider.kubernetes.long_id,
            self.request.id.to_string(),
            self.workspace_root_dir.to_string(),
            self.lib_root_dir.to_string(),
            self.request.test_cluster,
            self.docker_host.clone(),
            self.request.features.clone(),
            self.request.metadata.clone(),
            self.docker.clone(),
        )
    }

    fn handle_transaction_result(&self, logger: Box<dyn Logger>, transaction_result: TransactionResult) {
        match transaction_result {
            TransactionResult::Ok => {
                self.send_infrastructure_progress(logger.clone(), None);
            }
            TransactionResult::Error(engine_error) => {
                self.send_infrastructure_progress(logger.clone(), Some(*engine_error));
            }
            TransactionResult::Canceled => {
                // should never happen by design
                error!("Infrastructure task should never been canceled");
            }
        }
    }

    fn send_infrastructure_progress(&self, logger: Box<dyn Logger>, option_engine_error: Option<errors::EngineError>) {
        let kubernetes = &self.request.cloud_provider.kubernetes;
        if let Some(engine_error) = option_engine_error {
            let infrastructure_step = match self.request.action {
                Action::Create => InfrastructureStep::CreateError,
                Action::Pause => InfrastructureStep::PauseError,
                Action::Delete => InfrastructureStep::DeleteError,
            };
            let event_message =
                EventMessage::new_from_safe(format!("Kubernetes cluster failure {}", &infrastructure_step));

            let engine_event = EngineEvent::Error(
                engine_error.clone_engine_error_with_stage(Infrastructure(infrastructure_step)),
                Some(event_message),
            );

            logger.log(engine_event);
        } else {
            let infrastructure_step = match self.request.action {
                Action::Create => InfrastructureStep::Created,
                Action::Pause => InfrastructureStep::Paused,
                Action::Delete => InfrastructureStep::Deleted,
            };
            let event_message =
                EventMessage::new_from_safe(format!("Kubernetes cluster successfully {}", &infrastructure_step));
            let engine_event = EngineEvent::Info(
                EventDetails::new(
                    Some(self.request.cloud_provider.kind.clone()),
                    QoveryIdentifier::new(self.request.organization_long_id),
                    QoveryIdentifier::new(kubernetes.long_id),
                    self.request.id.to_string(),
                    Infrastructure(infrastructure_step),
                    Transmitter::Kubernetes(kubernetes.long_id, kubernetes.name.to_string()),
                ),
                event_message,
            );

            logger.log(engine_event);
        }
    }
}

impl Task for InfrastructureTask {
    fn created_at(&self) -> &DateTime<Utc> {
        &self.request.created_at
    }

    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn run(&self) {
        info!(
            "infrastructure task {} started with infrastructure id {}-{}-{}",
            self.id(),
            self.request.cloud_provider.id.as_str(),
            self.request.container_registry.id.as_str(),
            self.request.build_platform.id.as_str()
        );

        let engine = match self.request.engine(&self.info_context(), self.logger.clone()) {
            Ok(engine) => engine,
            Err(err) => {
                self.send_infrastructure_progress(self.logger.clone(), Some(err));
                return;
            }
        };

        // check and init the connection to all services
        let mut tx = match Transaction::new(&engine, self.logger.clone(), self.cancel_checker(), Box::new(|_| {})) {
            Ok(transaction) => transaction,
            Err(err) => {
                let engine_error = match err {
                    EngineConfigError::BuildPlatformNotValid(engine_error) => engine_error,
                    EngineConfigError::CloudProviderNotValid(engine_error) => engine_error,
                    EngineConfigError::DnsProviderNotValid(engine_error) => engine_error,
                    EngineConfigError::KubernetesNotValid(engine_error) => engine_error,
                };
                self.send_infrastructure_progress(self.logger.clone(), Some(engine_error));
                return;
            }
        };

        let _ = match self.request.action {
            Action::Create => tx.create_kubernetes(),
            Action::Pause => tx.pause_kubernetes(),
            Action::Delete => tx.delete_kubernetes(),
        };

        self.handle_transaction_result(self.logger.clone(), tx.commit());

        // only store if not running on a workstation
        if env::var("DEPLOY_FROM_FILE_KIND").is_err() {
            match qovery_engine::fs::create_workspace_archive(
                engine.context().workspace_root_dir(),
                engine.context().execution_id(),
            ) {
                Ok(file) => match upload_s3_file(
                    &self.info_context(),
                    self.request.archive.as_ref(),
                    file.as_str(),
                    AwsRegion::EuWest3, // TODO(benjaminch): make it customizable
                    self.request
                        .cloud_provider
                        .kubernetes
                        .advanced_settings
                        .pleco_resources_ttl,
                ) {
                    Ok(_) => {
                        let _ = fs::remove_file(file).map_err(|err| error!("Cannot delete file {}", err));
                    }
                    Err(e) => error!("Error while uploading archive {}", e),
                },
                Err(err) => error!("{}", err),
            };
        };

        info!("infrastructure task {} finished", self.id());
    }

    fn cancel(&self) -> bool {
        false
    }

    fn cancel_checker(&self) -> Box<dyn Fn() -> bool> {
        Box::new(|| false)
    }
}

#[derive(Clone)]
pub struct EnvironmentTask {
    id: Uuid,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<Url>,
    docker: Docker,
    request: EngineRequest,
    cancel_requested: Arc<AtomicBool>,
    current_step: Arc<RwLock<StepName>>,
    logger: Box<dyn Logger>,
}

impl EnvironmentTask {
    pub fn new(
        request: EngineRequest,
        workspace_root_dir: String,
        lib_root_dir: String,
        docker_host: Option<Url>,
        logger: Box<dyn Logger>,
    ) -> Self {
        let docker = Docker::new(docker_host.clone()).expect("Can't init docker builder");
        EnvironmentTask {
            id: Uuid::new_v4(),
            workspace_root_dir,
            lib_root_dir,
            docker_host,
            docker,
            request,
            logger,
            cancel_requested: Arc::new(AtomicBool::from(false)),
            current_step: Arc::new(RwLock::new(Waiting)),
        }
    }

    fn info_context(&self) -> Context {
        Context::new(
            self.request.organization_long_id,
            self.request.cloud_provider.kubernetes.long_id,
            self.request.id.to_string(),
            self.workspace_root_dir.to_string(),
            self.lib_root_dir.to_string(),
            self.request.test_cluster,
            self.docker_host.clone(),
            self.request.features.clone(),
            self.request.metadata.clone(),
            self.docker.clone(),
        )
    }

    fn _is_canceled(&self) -> bool {
        self.cancel_requested.load(Ordering::Acquire)
    }

    fn get_event_details(&self, step: EnvironmentStep) -> EventDetails {
        EventDetails::new(
            Some(self.request.cloud_provider.kind.clone()),
            QoveryIdentifier::new(self.request.organization_long_id),
            QoveryIdentifier::new(self.request.cloud_provider.kubernetes.long_id),
            self.request.id.to_string(),
            Stage::Environment(step),
            Transmitter::TaskManager(self.id, "engine".to_string()),
        )
    }
}

impl Task for EnvironmentTask {
    fn created_at(&self) -> &DateTime<Utc> {
        &self.request.created_at
    }

    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn run(&self) {
        info!("environment task {} started", self.id());

        self.logger.log(EngineEvent::Info(
            self.get_event_details(EnvironmentStep::Start),
            EventMessage::new("🚀 Qovery Engine starts to execute the deployment".to_string(), None),
        ));
        let guard = scopeguard::guard((), |_| {
            self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Terminated),
                EventMessage::new("Qovery Engine has terminated the deployment".to_string(), None),
            ));
        });

        let engine = match self.request.engine(&self.info_context(), self.logger.clone()) {
            Ok(engine) => engine,
            Err(err) => {
                self.logger.log(EngineEvent::Error(err, None));
                return;
            }
        };

        let task_status_updater = {
            let current_step = self.current_step.clone();

            move |step: &StepName| {
                if let Ok(mut current_step) = current_step.write() {
                    *current_step = step.clone();
                }
            }
        };
        let mut tx = match Transaction::new(
            &engine,
            self.logger.clone(),
            self.cancel_checker(),
            Box::new(task_status_updater),
        ) {
            Ok(transaction) => transaction,
            Err(err) => {
                self.logger.log(EngineEvent::Error(err.engine_error().clone(), None));
                return;
            }
        };

        let environment_action = match &self.request.target_environment {
            None => {
                self.logger.log(EngineEvent::Error(
                    errors::EngineError::new_invalid_engine_payload(
                        self.request.event_details(),
                        "failed to get environment action, self.request.environment_action() returned None variant",
                    ),
                    None,
                ));
                return;
            }
            Some(ea) => ea,
        };

        let env = environment_action.to_environment_domain(
            engine.context(),
            engine.cloud_provider(),
            engine.container_registry(),
            self.logger.clone(),
        );

        let env = match env {
            Ok(env) => env,
            Err(err) => {
                self.logger.log(EngineEvent::Error(
                    errors::EngineError::new_invalid_engine_payload(
                        self.request.event_details(),
                        err.to_string().as_str(),
                    ),
                    None,
                ));
                return;
            }
        };

        let env = Rc::new(RefCell::new(env));
        let action = self.request.action.clone();
        let _ = match action {
            Action::Create => tx.deploy_environment(&env),
            Action::Pause => tx.pause_environment(&env),
            Action::Delete => tx.delete_environment(&env),
        };

        // run the actions
        let tx_result = tx.commit();
        match (action, &tx_result) {
            (_, TransactionResult::Canceled) => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Cancelled),
                EventMessage::new("🚫 Deployment has been canceled at user request 🚫".to_string(), None),
            )),
            (Action::Create, TransactionResult::Ok) => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Deployed),
                EventMessage::new("❤️ Deployment succeeded ❤️".to_string(), None),
            )),
            (Action::Pause, TransactionResult::Ok) => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Paused),
                EventMessage::new("⏸️ Environment is paused".to_string(), None),
            )),
            (Action::Delete, TransactionResult::Ok) => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Deleted),
                EventMessage::new("🗑️ Environment is deleted".to_string(), None),
            )),

            (Action::Create, TransactionResult::Error(err)) => {
                self.logger.log(EngineEvent::Error(*err.clone(), None));
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::DeployedError),
                    EventMessage::new("💣 Deployment failed".to_string(), None),
                ));
            }
            (Action::Pause, TransactionResult::Error(err)) => {
                self.logger.log(EngineEvent::Error(*err.clone(), None));
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::PausedError),
                    EventMessage::new("💣 Environment failed to be paused".to_string(), None),
                ));
            }
            (Action::Delete, TransactionResult::Error(err)) => {
                self.logger.log(EngineEvent::Error(*err.clone(), None));
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::DeletedError),
                    EventMessage::new("💣 Environment failed to be deleted".to_string(), None),
                ));
            }
        };

        // Uploading to S3 can take a lot of time, and might hit the core timeout
        // So we early drop the guard to notify core that the task is done
        drop(guard);

        // only store if not running on a workstation
        if env::var("DEPLOY_FROM_FILE_KIND").is_err() {
            match qovery_engine::fs::create_workspace_archive(
                engine.context().workspace_root_dir(),
                engine.context().execution_id(),
            ) {
                Ok(file) => match upload_s3_file(
                    &self.info_context(),
                    self.request.archive.as_ref(),
                    file.as_str(),
                    AwsRegion::EuWest3, // TODO(benjaminch): make it customizable
                    self.request
                        .cloud_provider
                        .kubernetes
                        .advanced_settings
                        .pleco_resources_ttl,
                ) {
                    Ok(_) => {
                        let _ = fs::remove_file(file).map_err(|err| error!("Cannot remove file {}", err));
                    }
                    Err(e) => error!("Error while uploading archive {}", e),
                },
                Err(err) => error!("{}", err),
            };
        };

        info!("environment task {} finished", self.id());
    }

    fn cancel(&self) -> bool {
        if let Ok(current_step) = self.current_step.read() {
            if current_step.can_be_canceled() {
                self.cancel_requested.store(true, Ordering::Release);

                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::Cancel),
                    EventMessage::new("Cancel request received, aborting the deployment".to_string(), None),
                ));
                return true;
            }
        }

        false
    }

    fn cancel_checker(&self) -> Box<dyn Fn() -> bool> {
        let cancel_requested = self.cancel_requested.clone();
        Box::new(move || cancel_requested.load(Ordering::Acquire))
    }
}

fn basename(path: &str, sep: char) -> Cow<str> {
    let pieces = path.split(sep);
    match pieces.last() {
        Some(p) => p.into(),
        None => path.into(),
    }
}

fn upload_s3_file(
    context: &Context,
    archive: Option<&Archive>,
    file_path: &str,
    region: AwsRegion,
    bucket_ttl: i32,
) -> Result<(), ObjectStorageError> {
    let archive = match archive {
        Some(archive) => archive,
        None => {
            info!("no archive upload (request.archive is None)");
            return Ok(());
        }
    };

    let object_key = format!("{}/{}", context.organization_short_id(), basename(file_path, '/'));

    info!(
        "Sending file {} to bucket {} object {} with access_key_id '{}' and secret_access_key '{}'",
        file_path,
        archive.bucket_name.as_str(),
        object_key.as_str(),
        archive.access_key_id.as_str(),
        archive.secret_access_key.as_str(),
    );

    // I am using this s3 object directly to avoid reinventing the wheel.
    let ttl = match bucket_ttl {
        0 => None,
        _ => Some(bucket_ttl),
    };
    let s3 = qovery_engine::object_storage::s3::S3::new(
        context.clone(),
        "archive-123abc".to_string(),
        "archive-s3".to_string(),
        archive.access_key_id.to_string(),
        archive.secret_access_key.to_string(),
        region,
        true,
        ttl,
    );

    match s3.put(archive.bucket_name.as_str(), object_key.as_str(), file_path) {
        Ok(_) => {
            info!("Archive successfully pushed to Qovery S3");
            Ok(())
        }
        Err(err) => {
            warn!("Error while pushing archive to s3, {}", err);
            Err(err)
        }
    }
}
