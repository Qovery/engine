use crate::cmd::docker::Docker;
use crate::engine_task;
use crate::engine_task::qovery_api::QoveryApi;
use crate::engine_task::Task;
use crate::environment::models::abort::{Abort, AbortStatus};
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::engine_request::InfrastructureEngineRequest;
use crate::io_models::{Action, QoveryIdentifier};
use crate::log_file_writer::LogFileWriter;
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use std::sync::{Arc, RwLock};
use std::{env, fs};
use tokio::sync::broadcast;

pub struct InfrastructureTask {
    workspace_root_dir: String,
    lib_root_dir: String,
    docker: Arc<Docker>,
    request: InfrastructureEngineRequest,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    qovery_api: Arc<dyn QoveryApi>,
    span: tracing::Span,
    is_terminated: (RwLock<Option<broadcast::Sender<()>>>, broadcast::Receiver<()>),
    log_file_writer: Option<LogFileWriter>,
}

impl InfrastructureTask {
    pub fn new(
        request: InfrastructureEngineRequest,
        workspace_root_dir: String,
        lib_root_dir: String,
        docker: Arc<Docker>,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        qovery_api: Box<dyn QoveryApi>,
        log_file_writer: Option<LogFileWriter>,
    ) -> Self {
        let span = info_span!(
            "infrastructure_task",
            organization_id = request.organization_long_id.to_string(),
            cluster_id = request.kubernetes.long_id.to_string(),
            // used by grafana dashboard to filter by action and compute diff of change
            action = match request.action {
                Action::Create
                    if request
                        .metadata
                        .as_ref()
                        .and_then(|x| x.is_first_cluster_deployment)
                        .unwrap_or_default() =>
                    "install".to_string(),
                Action::Create => "update".to_string(),
                _ => request.action.to_string(),
            },
        );

        InfrastructureTask {
            workspace_root_dir,
            lib_root_dir,
            docker,
            request,
            logger,
            metrics_registry,
            qovery_api: Arc::from(qovery_api),
            span,
            is_terminated: {
                let (tx, rx) = broadcast::channel(1);
                (RwLock::new(Some(tx)), rx)
            },
            log_file_writer,
        }
    }

    fn info_context(&self) -> Context {
        Context::new(
            self.request.organization_long_id,
            self.request.kubernetes.long_id,
            self.request.id.to_string(),
            self.workspace_root_dir.to_string(),
            self.lib_root_dir.to_string(),
            self.request.test_cluster,
            self.request.features.clone(),
            self.request.metadata.clone(),
            self.docker.clone(),
            self.qovery_api.clone(),
            self.request.event_details(),
        )
    }

    fn get_event_details(&self, step: InfrastructureStep) -> EventDetails {
        EventDetails::clone_changing_stage(self.request.event_details(), Infrastructure(step))
    }

    fn handle_transaction_result(&self, logger: Box<dyn Logger>, transaction_result: Result<(), Box<EngineError>>) {
        match transaction_result {
            Ok(()) => self.send_infrastructure_progress(logger.clone(), None),
            Err(err) => self.send_infrastructure_progress(logger.clone(), Some(err)),
        }
    }

    fn send_infrastructure_progress(&self, logger: Box<dyn Logger>, option_engine_error: Option<Box<EngineError>>) {
        let kubernetes = &self.request.kubernetes;
        if let Some(engine_error) = option_engine_error {
            let infrastructure_step = match self.request.action {
                Action::Create => InfrastructureStep::CreateError,
                Action::Pause => InfrastructureStep::PauseError,
                Action::Delete => InfrastructureStep::DeleteError,
                Action::Restart => InfrastructureStep::RestartedError,
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
                Action::Restart => InfrastructureStep::RestartedError,
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
    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn run(&self) {
        if self.request.is_self_managed() {
            engine_task::enable_log_file_writer(&self.info_context(), &self.log_file_writer);
        }

        let _span = self.span.enter();
        info!(
            "infrastructure task {} started with infrastructure id {}",
            self.id(),
            self.request.cloud_provider.id.as_str(),
        );

        self.logger.log(EngineEvent::Info(
            self.get_event_details(InfrastructureStep::Start),
            EventMessage::new("Qovery Engine has started the infrastructure deployment".to_string(), None),
        ));
        let guard = scopeguard::guard((), |_| {
            self.logger.log(EngineEvent::Info(
                self.get_event_details(InfrastructureStep::Terminated),
                EventMessage::new("Qovery Engine has terminated the infrastructure deployment".to_string(), None),
            ));
            let Some(is_terminated_tx) = self.is_terminated.0.write().unwrap().take() else {
                return;
            };
            let _ = is_terminated_tx.send(());
        });

        let infra_ctx = match self.request.to_infrastructure_context(
            &self.info_context(),
            self.request.event_details(),
            self.logger.clone(),
            self.metrics_registry.clone(),
            true,
        ) {
            Ok(engine) => engine,
            Err(err) => {
                self.send_infrastructure_progress(self.logger.clone(), Some(err));
                return;
            }
        };

        let ret = infra_ctx
            .kubernetes()
            .as_infra_actions()
            .run(&infra_ctx, self.request.action.to_service_action());
        self.handle_transaction_result(self.logger.clone(), ret);

        // Uploading to S3 can take a lot of time, and might hit the core timeout
        // So we early drop the guard to notify core that the task is done
        drop(guard);
        engine_task::disable_log_file_writer(&self.log_file_writer);

        // only store if not running on a workstation
        if env::var("DEPLOY_FROM_FILE_KIND").is_err() {
            match crate::fs::create_workspace_archive(
                infra_ctx.context().workspace_root_dir(),
                infra_ctx.context().execution_id(),
            ) {
                Ok(file) => match engine_task::upload_s3_file(self.request.archive.as_ref(), &file) {
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

    fn cancel(&self, _force_requested: bool) -> bool {
        false
    }

    fn cancel_checker(&self) -> Box<dyn Abort> {
        Box::new(move || AbortStatus::None)
    }

    fn is_terminated(&self) -> bool {
        self.is_terminated.0.read().map(|tx| tx.is_none()).unwrap_or(true)
    }

    fn await_terminated(&self) -> broadcast::Receiver<()> {
        self.is_terminated.1.resubscribe()
    }
}
