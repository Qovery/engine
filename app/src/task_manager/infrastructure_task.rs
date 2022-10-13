use crate::task_manager::models::{Action, Archive, EngineRequest};
use crate::task_manager::scheduler::Task;
use chrono::{DateTime, Utc};
use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::cmd::docker::Docker;
use qovery_engine::engine::EngineConfigError;
use qovery_engine::errors::EngineError;
use qovery_engine::events::Stage::Infrastructure;
use qovery_engine::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Transmitter};
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::logger::Logger;
use qovery_engine::object_storage::errors::ObjectStorageError;
use qovery_engine::object_storage::ObjectStorage;
use qovery_engine::transaction::{Transaction, TransactionResult};
use std::borrow::Cow;
use std::{env, fs};
use url::Url;

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

    fn send_infrastructure_progress(&self, logger: Box<dyn Logger>, option_engine_error: Option<EngineError>) {
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

fn basename(path: &str, sep: char) -> Cow<str> {
    let pieces = path.split(sep);
    match pieces.last() {
        Some(p) => p.into(),
        None => path.into(),
    }
}

pub fn upload_s3_file(
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
