use super::Task;
use crate::build_platform;
use crate::build_platform::{BuildError, BuildPlatform};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service;
use crate::cloud_provider::service::Service;
use crate::cmd::docker::Docker;
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::{to_engine_error, ContainerRegistry, RegistryTags};
use crate::deployment_action::deploy_environment::EnvironmentDeployment;
use crate::deployment_report::logger::EnvLogger;
use crate::engine::InfrastructureContext;
use crate::engine_task::qovery_api::QoveryApi;
use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::io_models::context::Context;
use crate::io_models::engine_request::EnvironmentEngineRequest;
use crate::io_models::Action;
use crate::log_file_writer::LogFileWriter;
use crate::logger::Logger;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepRecordHandle, StepStatus};
use crate::models::abort::{Abort, AbortStatus, AtomicAbortStatus};
use crate::transaction::DeploymentOption;
use base64::Engine;
use itertools::Itertools;
use std::cmp::{max, min};
use std::collections::{HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::thread::ScopedJoinHandle;
use std::time::Duration;
use std::{env, fs, thread};
use tokio::sync::broadcast;
use uuid::Uuid;

pub struct EnvironmentTask {
    workspace_root_dir: String,
    lib_root_dir: String,
    docker: Arc<Docker>,
    request: EnvironmentEngineRequest,
    cancel_requested: Arc<AtomicAbortStatus>,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    qovery_api: Arc<dyn QoveryApi>,
    span: tracing::Span,
    is_terminated: (RwLock<Option<broadcast::Sender<()>>>, broadcast::Receiver<()>),
    log_file_writer: Option<LogFileWriter>,
}

impl EnvironmentTask {
    pub fn new(
        request: EnvironmentEngineRequest,
        workspace_root_dir: String,
        lib_root_dir: String,
        docker: Arc<Docker>,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        qovery_api: Box<dyn QoveryApi>,
        log_file_writer: Option<LogFileWriter>,
    ) -> Self {
        let span = info_span!(
            "environment_task",
            //organization_id = request.organization_long_id.to_string(),
            //cluster_id = request.kubernetes.long_id.to_string(),
            execution_id = request.id,
        );

        let secrets = Self::get_secrets(&request);
        EnvironmentTask {
            workspace_root_dir,
            lib_root_dir,
            docker,
            request,
            logger: logger.with_secrets(secrets),
            metrics_registry,
            cancel_requested: Arc::new(AtomicAbortStatus::new(AbortStatus::None)),
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

    fn infrastructure_context(&self) -> Result<InfrastructureContext, Box<EngineError>> {
        self.request.engine(
            &self.info_context(),
            self.request.event_details(),
            self.logger.clone(),
            self.metrics_registry.clone(),
            false,
        )
    }

    fn _is_canceled(&self) -> bool {
        self.cancel_requested.load(Ordering::Relaxed).should_cancel()
    }

    fn get_event_details(&self, step: EnvironmentStep) -> EventDetails {
        EventDetails::clone_changing_stage(self.request.event_details(), Stage::Environment(step))
    }

    pub fn build_and_push_services(
        environment_id: Uuid,
        project_id: Uuid,
        services: Vec<&mut dyn Service>,
        option: &DeploymentOption,
        infra_ctx: &InfrastructureContext,
        max_build_in_parallel: usize,
        env_logger: impl Fn(String),
        mk_logger: impl Fn(&dyn Service) -> EnvLogger + Send + Sync,
        abort: &dyn Abort,
    ) -> Result<(), Box<EngineError>> {
        // Only keep services that have something to build
        let mut build_needs_buildpacks = false;
        let metrics_registry: Arc<dyn MetricsRegistry> = Arc::from(infra_ctx.metrics_registry().clone_dyn());
        let services = services
            .into_iter()
            .filter(|srv| {
                if let Some(build) = srv.build() {
                    build_needs_buildpacks = build_needs_buildpacks || build.use_buildpacks();
                    true
                } else {
                    false
                }
            })
            .collect::<Vec<_>>();

        // If nothing to build, do nothing
        if services.first().is_none() {
            return Ok(());
        };

        let max_build_in_parallel = if build_needs_buildpacks {
            env_logger("⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️️️".to_string());
            env_logger("⚠️ By using buildpacks you cannot build in parallel. Please migrate to Docker to benefit of parallel builds ⚠️".to_string());
            env_logger("⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️️️".to_string());
            1
        } else {
            max(min(max_build_in_parallel, services.len()), 1)
        };

        // To convert ContainerError to EngineError
        let cr_registry = infra_ctx.container_registry();
        let cr_to_engine_error = |err: ContainerRegistryError| -> EngineError {
            let event_details = cr_registry.get_event_details(Stage::Environment(EnvironmentStep::BuiltError));
            to_engine_error(event_details, err)
        };

        // Do setup of registry and be sure we are login to the registry
        cr_registry.create_registry().map_err(cr_to_engine_error)?;

        // We wrap should_abort, to allow to notify parallel build threads to abort when one of them fails
        let abort_flag = AtomicAbortStatus::new(AbortStatus::None);
        let abort_status = || AbortStatus::merge(abort_flag.load(Ordering::Relaxed), abort.status());

        // Prepare our tasks
        let img_retention_time_sec = infra_ctx
            .kubernetes()
            .advanced_settings()
            .registry_image_retention_time_sec;
        let resource_ttl = infra_ctx.kubernetes().advanced_settings().resource_ttl();
        let cr_registry = infra_ctx.container_registry();
        let build_platform = infra_ctx.build_platform();

        services.iter().for_each(|service| {
            metrics_registry.start_record(*service.long_id(), StepLabel::Service, StepName::BuildQueueing);
        });

        let build_tasks = services
            .into_iter()
            .map(|service| {
                || {
                    metrics_registry.stop_record(*service.long_id(), StepName::BuildQueueing, StepStatus::Success);
                    Self::build_and_push_service(
                        service,
                        option,
                        cr_registry,
                        build_platform,
                        img_retention_time_sec,
                        RegistryTags {
                            environment_id: environment_id.to_string(),
                            project_id: project_id.to_string(),
                            resource_ttl,
                        },
                        cr_to_engine_error,
                        &mk_logger,
                        metrics_registry.clone(),
                        &abort_status,
                    )
                }
            })
            .collect_vec();

        let builder_threadpool = BuilderThreadPool::new();
        builder_threadpool.run(
            build_tasks,
            NonZeroUsize::new(max_build_in_parallel).unwrap(),
            &abort_flag,
            &abort_status,
        )
    }

    fn build_and_push_service(
        service: &mut dyn Service,
        option: &DeploymentOption,
        cr_registry: &dyn ContainerRegistry,
        build_platform: &dyn BuildPlatform,
        image_retention_time_sec: u32,
        registry_tags: RegistryTags,
        cr_to_engine_error: impl Fn(ContainerRegistryError) -> EngineError,
        mk_logger: impl Fn(&dyn Service) -> EnvLogger,
        metrics_registry: Arc<dyn MetricsRegistry>,
        abort: &dyn Abort,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(service);
        let build = match service.build_mut() {
            Some(build) => build,
            None => return Ok(()), // this case should not happen as we filter on buildable services
        };
        let image_name = build.image.full_image_name_with_tag();

        // If image already exists in the registry, skip the build
        if !option.force_build && cr_registry.image_exists(&build.image) {
            let msg = format!("✅ Container image {image_name} already exists and ready to use");
            logger.send_success(msg);
            return Ok(());
        }

        // Be sure that our repository exist before trying to pull/push images from it
        logger.send_progress(format!(
            "🗂️ Provisioning container repository {}",
            build.image.repository_name()
        ));
        let provision_registry_record = metrics_registry.start_record(
            build.image.service_long_id,
            StepLabel::Service,
            StepName::RegistryCreateRepository,
        );

        match cr_registry.create_repository(build.image.repository_name(), image_retention_time_sec, registry_tags) {
            Err(err) => {
                provision_registry_record.stop(StepStatus::Error);
                return Err(Box::new(cr_to_engine_error(err)));
            }
            Ok((_repository, repository_info)) => provision_registry_record.stop(if repository_info.created {
                StepStatus::Success
            } else {
                StepStatus::Skip
            }),
        }

        // Ok now everything is setup, we can try to build the app
        let build_result = build_platform.build(build, &logger, metrics_registry.clone(), abort);
        match build_result {
            Ok(_) => {
                let msg = format!("✅ Container image {} is built and ready to use", &image_name);
                logger.send_success(msg);
                Ok(())
            }
            Err(err @ BuildError::Aborted { .. }) => {
                let msg = format!(
                    "🚫 Container image {} build has been canceled. Either due to timeout or at user request",
                    &image_name
                );
                info!("{}", err);
                let event_details = service.get_event_details(Stage::Environment(EnvironmentStep::Cancelled));
                let build_result = build_platform::to_engine_error(event_details, err, msg);
                logger.send_error(build_result.clone());
                Err(Box::new(build_result))
            }
            Err(err @ BuildError::DockerError { .. }) => {
                let msg = format!(
                    "❌ Container image {} failed to be build: Look at the build logs to understand the error",
                    &image_name
                );
                info!("{}", err);
                let event_details = service.get_event_details(Stage::Environment(EnvironmentStep::BuiltError));
                let build_result = build_platform::to_engine_error(event_details, err, msg);
                logger.send_error(build_result.clone());
                Err(Box::new(build_result))
            }
            Err(err @ BuildError::GitError { .. }) => {
                let msg = format!("❌ Application {} failed to be cloned: {}", &service.name(), err);
                info!("{}", err);
                let event_details = service.get_event_details(Stage::Environment(EnvironmentStep::BuiltError));
                let build_result = build_platform::to_engine_error(event_details, err, msg);
                logger.send_error(build_result.clone());
                Err(Box::new(build_result))
            }
            Err(err) => {
                let msg = format!("❌ Container image {} failed to be build: {}", &image_name, err);
                let event_details = service.get_event_details(Stage::Environment(EnvironmentStep::BuiltError));
                let build_result = build_platform::to_engine_error(event_details, err, msg);
                logger.send_error(build_result.clone());
                Err(Box::new(build_result))
            }
        }
    }

    pub fn deploy_environment(
        mut environment: Environment,
        infra_ctx: &InfrastructureContext,
        env_logger: impl Fn(String),
        abort: &dyn Abort,
    ) -> Result<(), Box<EngineError>> {
        let mut deployed_services: HashSet<Uuid> = HashSet::new();
        let event_details = environment.event_details().clone();
        let run_deploy = || -> Result<(), Box<EngineError>> {
            // Build apps
            if abort.status().should_cancel() {
                return Err(Box::new(EngineError::new_task_cancellation_requested(event_details)));
            }

            let logger = Arc::new(infra_ctx.kubernetes().logger().clone_dyn());
            let services_to_build: Vec<&mut dyn Service> = environment
                .applications
                .iter_mut()
                .map(|app| app.as_service_mut())
                .chain(environment.jobs.iter_mut().map(|job| job.as_service_mut()))
                .collect();
            Self::build_and_push_services(
                environment.long_id,
                environment.project_long_id,
                services_to_build,
                &DeploymentOption {
                    force_build: false,
                    force_push: false,
                },
                infra_ctx,
                environment.max_parallel_build as usize,
                env_logger,
                |srv: &dyn Service| EnvLogger::new(srv, EnvironmentStep::Build, logger.clone()),
                abort,
            )?;

            if abort.status().should_cancel() {
                return Err(Box::new(EngineError::new_task_cancellation_requested(event_details)));
            }

            let mut env_deployment = EnvironmentDeployment::new(infra_ctx, &environment, abort, logger.clone())?;
            let deployment_ret = match environment.action {
                service::Action::Create => env_deployment.on_create(),
                service::Action::Pause => env_deployment.on_pause(),
                service::Action::Delete => env_deployment.on_delete(),
                service::Action::Restart => env_deployment.on_restart(),
            };
            deployed_services = env_deployment.deployed_services.lock().map(|v| v.clone()).unwrap();

            deployment_ret
        };

        let deployment_err = match run_deploy() {
            Ok(_) => {
                return Ok(());
            } // return early if no error
            Err(err) => err,
        };

        // Handle deployment error, send back Cancelled event for all services that were not deployed
        let services = std::iter::empty()
            .chain(environment.applications.iter().map(|x| x.as_service()))
            .chain(environment.containers.iter().map(|x| x.as_service()))
            .chain(environment.routers.iter().map(|x| x.as_service()))
            .chain(environment.databases.iter().map(|x| x.as_service()))
            .chain(environment.jobs.iter().map(|x| x.as_service()))
            .chain(environment.helm_charts.iter().map(|x| x.as_service()));

        for service in services {
            if deployed_services.contains(service.long_id()) {
                continue;
            }
            infra_ctx.kubernetes().logger().log(EngineEvent::Info(
                service.get_event_details(Stage::Environment(EnvironmentStep::Cancelled)),
                EventMessage::new_from_safe("".to_string()),
            ));
        }

        Err(deployment_err)
    }

    fn get_secrets(request: &EnvironmentEngineRequest) -> Vec<String> {
        let mut secrets = vec![];
        let services_secrets = request
            .target_environment
            .applications
            .iter()
            .flat_map(|x| x.environment_vars_with_infos.values())
            .chain(
                request
                    .target_environment
                    .containers
                    .iter()
                    .flat_map(|x| x.environment_vars_with_infos.values()),
            )
            .chain(
                request
                    .target_environment
                    .jobs
                    .iter()
                    .flat_map(|x| x.environment_vars_with_infos.values()),
            )
            .chain(
                request
                    .target_environment
                    .helms
                    .iter()
                    .flat_map(|x| x.environment_vars_with_infos.values()),
            );

        let service_secrets = services_secrets.filter_map(|v| {
            if !v.is_secret {
                return None;
            }
            let decoded_secret = base64::engine::general_purpose::STANDARD
                .decode(&v.value)
                .unwrap_or_default();
            Some(String::from_utf8(decoded_secret).unwrap_or_default())
        });
        secrets.extend(service_secrets);

        let cloud_provider_secrets = request
            .cloud_provider
            .options
            .gcp_credentials
            .as_ref()
            .map(|x| &x.private_key)
            .into_iter()
            .chain(request.cloud_provider.options.secret_access_key.iter())
            .chain(request.cloud_provider.options.password.iter())
            .chain(request.cloud_provider.options.scaleway_secret_key.iter())
            .chain(request.cloud_provider.options.spaces_secret_key.iter())
            .cloned();

        secrets.extend(cloud_provider_secrets);
        secrets
    }

    fn stop_total_steps_records(
        deployment_ret: &Result<(), Box<EngineError>>,
        record: StepRecordHandle,
        service_records: Vec<StepRecordHandle>,
    ) {
        let step_status = match deployment_ret {
            Ok(()) => StepStatus::Success,
            Err(err) if err.tag().is_cancel() => StepStatus::Cancel,
            Err(_) => StepStatus::Error,
        };

        for record in service_records {
            record.stop(step_status.clone());
        }
        record.stop(step_status);
    }
}

impl Task for EnvironmentTask {
    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn run(&self) {
        if self.request.is_self_managed() {
            super::enable_log_file_writer(&self.info_context(), &self.log_file_writer);
        }

        let _span = self.span.enter();
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
            let Some(is_terminated_tx) = self.is_terminated.0.write().unwrap().take() else {
                return;
            };
            let _ = is_terminated_tx.send(());
        });

        let infra_context = match self.infrastructure_context() {
            Ok(infra_ctx) => infra_ctx,
            Err(err) => {
                self.logger.log(EngineEvent::Error(*err, None));
                return;
            }
        };
        let env_step = self
            .request
            .target_environment
            .action
            .to_service_action()
            .to_environment_step();
        let event_details = self.get_event_details(env_step);
        let environment = match self.request.target_environment.to_environment_domain(
            infra_context.context(),
            infra_context.cloud_provider(),
            infra_context.container_registry(),
            infra_context.kubernetes(),
        ) {
            Ok(env) => env,
            Err(err) => {
                self.logger.log(EngineEvent::Error(
                    EngineError::new_invalid_engine_payload(event_details, err.to_string().as_str(), None),
                    None,
                ));
                return;
            }
        };

        // run the actions
        let env_logger = |msg: String| {
            self.logger
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new(msg, None)));
        };

        let metrics_registry = Arc::new(infra_context.metrics_registry().clone_dyn());
        let service_ids = std::iter::empty()
            .chain(environment.applications.iter().map(|x| x.as_service().long_id()))
            .chain(environment.containers.iter().map(|x| x.as_service().long_id()))
            .chain(environment.routers.iter().map(|x| x.as_service().long_id()))
            .chain(environment.databases.iter().map(|x| x.as_service().long_id()))
            .chain(environment.jobs.iter().map(|x| x.as_service().long_id()))
            .chain(environment.helm_charts.iter().map(|x| x.as_service().long_id()));

        let record = metrics_registry.start_record(environment.long_id, StepLabel::Environment, StepName::Total);
        let service_records: Vec<StepRecordHandle> = service_ids
            .into_iter()
            .map(|service_id| metrics_registry.start_record(*service_id, StepLabel::Service, StepName::Total))
            .collect();

        let deployment_ret = EnvironmentTask::deploy_environment(
            environment,
            &infra_context,
            env_logger,
            self.cancel_checker().as_ref(),
        );

        Self::stop_total_steps_records(&deployment_ret, record, service_records);

        match (&self.request.action, deployment_ret) {
            (Action::Create, Ok(())) => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Deployed),
                EventMessage::new("❤️ Deployment succeeded ❤️".to_string(), None),
            )),
            (Action::Pause, Ok(())) => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Paused),
                EventMessage::new("⏸️ Environment is paused".to_string(), None),
            )),
            (Action::Delete, Ok(())) => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Deleted),
                EventMessage::new("🗑️ Environment is deleted".to_string(), None),
            )),
            (Action::Restart, Ok(_)) => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Restarted),
                EventMessage::new("⟳️ Environment is restarted".to_string(), None),
            )),
            (_, Err(err)) if err.tag().is_cancel() => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Cancelled),
                EventMessage::new("🚫 Deployment has been canceled at user request 🚫".to_string(), None),
            )),
            (Action::Create, Err(err)) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::DeployedError),
                    EventMessage::new(
                        "💣 Deployment aborted following a failure to deploy a service. This is a general/global message. Look at your services deployment status to know which one made the deployment fail"
                            .trim()
                            .to_string(),
                        Some(err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }
            (Action::Pause, Err(err)) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::PausedError),
                    EventMessage::new(
                        "💣 Environment failed to be paused".to_string(),
                        Some(err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }
            (Action::Delete, Err(err)) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::DeletedError),
                    EventMessage::new(
                        "💣 Environment failed to be deleted".to_string(),
                        Some(err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }
            (Action::Restart, Err(err)) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::RestartedError),
                    EventMessage::new(
                        "💣 Environment failed to be restarted".to_string(),
                        Some(err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }
        };

        // Uploading to S3 can take a lot of time, and might hit the core timeout
        // So we early drop the guard to notify core that the task is done
        drop(guard);
        super::disable_log_file_writer(&self.log_file_writer);

        // only store if not running on a workstation
        if env::var("DEPLOY_FROM_FILE_KIND").is_err() {
            match crate::fs::create_workspace_archive(
                infra_context.context().workspace_root_dir(),
                infra_context.context().execution_id(),
            ) {
                Ok(file) => match super::upload_s3_file(self.request.archive.as_ref(), &file) {
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

    fn cancel(&self, force_requested: bool) -> bool {
        if self.is_terminated() {
            info!("Skipping cancel action as the task is already terminated.");
            return false;
        }

        self.cancel_requested.store(
            match force_requested {
                true => AbortStatus::UserForceRequested,
                false => AbortStatus::Requested,
            },
            Ordering::Relaxed,
        );
        self.logger.log(EngineEvent::Info(
            self.get_event_details(EnvironmentStep::Cancel),
            EventMessage::new(r#"
                    🚫 Cancel received, deployment is going to stop.
                    This may take a while, as a safe point need to be reached.
                    Some operation cannot be stopped (i.e: terraform actions) and need to be completed before stopping the deployment
                    "#.trim().to_string()
                              , None),
        ));
        true
    }

    fn cancel_checker(&self) -> Box<dyn Abort> {
        let cancel_requested = self.cancel_requested.clone();
        Box::new(move || cancel_requested.load(Ordering::Relaxed))
    }

    fn is_terminated(&self) -> bool {
        self.is_terminated.0.read().map(|tx| tx.is_none()).unwrap_or(true)
    }

    fn await_terminated(&self) -> broadcast::Receiver<()> {
        self.is_terminated.1.resubscribe()
    }
}

struct BuilderThreadPool {}

impl BuilderThreadPool {
    pub fn new() -> Self {
        Self {}
    }

    pub fn run<Err, Task>(
        &self,
        tasks: Vec<Task>,
        max_parallelism: NonZeroUsize,
        should_abort_flag: &AtomicAbortStatus,
        abort: &dyn Abort,
    ) -> Result<(), Err>
    where
        Err: Send,
        Task: FnMut() -> Result<(), Err> + Send,
    {
        // Launch our thread-pool
        let current_thread = thread::current();
        thread::scope(|scope| {
            let mut ret = Ok(());
            let mut active_threads: VecDeque<ScopedJoinHandle<Result<(), Err>>> =
                VecDeque::with_capacity(max_parallelism.get());

            let mut handle_thread_result = |th_result: thread::Result<Result<(), Err>>| {
                match th_result {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        // We want to store only the first error
                        if ret.is_ok() {
                            should_abort_flag.store(AbortStatus::Requested, Ordering::Relaxed);
                            ret = Err(err);
                        }
                    }
                    Err(err) => panic!("Building thread panicked: {err:?}"),
                }
            };

            let mut await_build_slot = |active_threads: &mut VecDeque<ScopedJoinHandle<_>>| {
                if active_threads.len() < max_parallelism.get() {
                    return;
                }

                // There is no available build slot, so we wait for a thread to terminate
                let terminated_thread_ix = loop {
                    match active_threads.iter().position(|th| th.is_finished()) {
                        // timeout is needed because we call unpark within the thread
                        // So it can happen that we got unparked but the thread is not marked as finished yet
                        None => thread::park_timeout(Duration::from_secs(10)),
                        Some(position) => break position,
                    }
                };

                handle_thread_result(active_threads.swap_remove_back(terminated_thread_ix).unwrap().join());
            };

            // Launch our build in parallel for each service
            for (ix, mut task) in tasks.into_iter().enumerate() {
                // Ensure we have a slot available to run a new thread
                await_build_slot(&mut active_threads);

                if abort.status().should_cancel() {
                    break;
                }

                // We have a slot to run a new thread, so start a new build
                let th = thread::Builder::new()
                    .name(format!("builder-{}", ix))
                    .spawn_scoped(scope, {
                        let current_thread = &current_thread;
                        let current_span = tracing::Span::current();

                        move || {
                            let _span = current_span.enter();
                            let _guard = scopeguard::guard((), |_| current_thread.unpark());
                            task()
                        }
                    });
                active_threads.push_back(th.unwrap());
            }

            // Wait for all threads to terminate
            for th in active_threads {
                handle_thread_result(th.join());
            }

            ret
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[test]
    fn test_builder_thread_pool() {
        let pool = BuilderThreadPool::new();

        // Test we can run 10 tasks in parallel
        let active_tasks = AtomicUsize::new(0);
        let mut tasks = Vec::new();
        for _i in 0..10 {
            tasks.push(|| {
                active_tasks.fetch_add(1, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(100));
                Result::<(), ()>::Ok(())
            });
        }
        pool.run(
            tasks,
            NonZeroUsize::new(3).unwrap(),
            &AtomicAbortStatus::new(AbortStatus::None),
            &|| AbortStatus::None,
        )
        .unwrap();

        assert_eq!(active_tasks.load(Ordering::Relaxed), 10);

        // Test max parallelism
        let active_tasks = AtomicUsize::new(0);
        let max_active_task = AtomicUsize::new(0);
        let mut tasks = Vec::new();
        for _i in 0..10 {
            tasks.push(|| {
                let nb_tasks = active_tasks.fetch_add(1, Ordering::Relaxed);
                max_active_task.fetch_max(nb_tasks + 1, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(1000));
                active_tasks.fetch_sub(1, Ordering::Relaxed);
                Result::<(), ()>::Ok(())
            });
        }
        pool.run(
            tasks,
            NonZeroUsize::new(3).unwrap(),
            &AtomicAbortStatus::new(AbortStatus::None),
            &|| AbortStatus::None,
        )
        .unwrap();

        assert_eq!(active_tasks.load(Ordering::Relaxed), 0);
        assert_eq!(max_active_task.load(Ordering::Relaxed), 3);

        // Test we get our error, and that we try to stop all tasks on first error
        let mut tasks = Vec::new();
        let active_taks = Arc::new(AtomicUsize::new(0));
        let should_abort_flag = AtomicAbortStatus::new(AbortStatus::None);
        for i in 0..10 {
            tasks.push({
                let active_tasks = active_taks.clone();
                move || {
                    active_tasks.fetch_add(1, Ordering::Relaxed);
                    thread::sleep(Duration::from_millis(100));
                    if i == 5 {
                        Result::<(), ()>::Err(())
                    } else {
                        Result::<(), ()>::Ok(())
                    }
                }
            });
        }
        let ret = pool.run(tasks, NonZeroUsize::new(2).unwrap(), &should_abort_flag, &|| {
            should_abort_flag.load(Ordering::Relaxed)
        });

        assert!(ret.is_err());
        assert_ne!(active_taks.load(Ordering::Relaxed), 10);
    }
}
