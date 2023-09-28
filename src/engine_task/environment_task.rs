use super::Task;
use crate::build_platform;
use crate::build_platform::{to_build_error, BuildError, BuildPlatform};
use crate::cloud_provider::aws::regions::AwsRegion;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service;
use crate::cloud_provider::service::Service;
use crate::cmd::command::CommandKiller;
use crate::cmd::docker;
use crate::cmd::docker::{BuilderHandle, Docker};
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::{to_engine_error, ContainerRegistry};
use crate::deployment_action::deploy_environment::EnvironmentDeployment;
use crate::deployment_report::logger::EnvLogger;
use crate::engine::InfrastructureContext;
use crate::engine_task::qovery_api::QoveryApi;
use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::io_models::context::Context;
use crate::io_models::engine_request::EnvironmentEngineRequest;
use crate::io_models::Action;
use crate::logger::Logger;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::transaction::DeploymentOption;
use itertools::Itertools;
use std::cmp::{max, min};
use std::collections::{HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
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
    cancel_requested: Arc<AtomicBool>,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    qovery_api: Arc<Box<dyn QoveryApi>>,
    span: tracing::Span,
    is_terminated: (RwLock<Option<broadcast::Sender<()>>>, broadcast::Receiver<()>),
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
    ) -> Self {
        let span = info_span!(
            "environment_task",
            //organization_id = request.organization_long_id.to_string(),
            //cluster_id = request.kubernetes.long_id.to_string(),
            execution_id = request.id,
        );

        EnvironmentTask {
            workspace_root_dir,
            lib_root_dir,
            docker,
            request,
            logger,
            metrics_registry,
            cancel_requested: Arc::new(AtomicBool::new(false)),
            qovery_api: Arc::new(qovery_api),
            span,
            is_terminated: {
                let (tx, rx) = broadcast::channel(1);
                (RwLock::new(Some(tx)), rx)
            },
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

    // FIXME: Remove EngineConfig type, there is no use for it
    // merge it with DeploymentTarget type
    fn infrastructure_context(&self) -> Result<InfrastructureContext, Box<EngineError>> {
        self.request.engine(
            &self.info_context(),
            self.request.event_details(),
            self.logger.clone(),
            self.metrics_registry.clone(),
        )
    }

    fn _is_canceled(&self) -> bool {
        self.cancel_requested.load(Ordering::Relaxed)
    }

    fn get_event_details(&self, step: EnvironmentStep) -> EventDetails {
        EventDetails::clone_changing_stage(self.request.event_details(), Stage::Environment(step))
    }

    pub fn build_and_push_services(
        environment_id: Uuid,
        services: Vec<&mut dyn Service>,
        option: &DeploymentOption,
        infra_ctx: &InfrastructureContext,
        max_build_in_parallel: usize,
        env_logger: impl Fn(String),
        mk_logger: impl Fn(&dyn Service) -> EnvLogger + Send + Sync,
        should_abort: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), Box<EngineError>> {
        // Only keep services that have something to build
        let mut build_needs_builpacks = false;
        let metrics_registry = Arc::new(infra_ctx.kubernetes().metrics_registry().clone_dyn());
        let services = services
            .into_iter()
            .filter(|srv| {
                if let Some(build) = srv.build() {
                    build_needs_builpacks = build_needs_builpacks || build.use_buildpacks();
                    true
                } else {
                    false
                }
            })
            .collect::<Vec<_>>();

        // If nothing to build, do nothing
        let first_service = match services.first() {
            None => return Ok(()),
            Some(srv) => srv,
        };

        let provision_builder =
            metrics_registry.start_record(environment_id, StepLabel::Environment, StepName::ProvisionBuilder);
        let builder_handle = match Self::provision_builder(
            infra_ctx,
            max_build_in_parallel,
            env_logger,
            &should_abort,
            build_needs_builpacks,
            &services,
            first_service,
        ) {
            Ok(handle) => {
                provision_builder.stop(StepStatus::Success);
                handle
            }
            Err(engine_error) => {
                provision_builder.stop(StepStatus::Error);
                return Err(engine_error);
            }
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
        let should_abort_flag = AtomicBool::new(false);
        let should_abort = || should_abort_flag.load(Ordering::Relaxed) || should_abort();

        // Prepare our tasks
        let img_retention_time_sec = infra_ctx
            .kubernetes()
            .advanced_settings()
            .registry_image_retention_time_sec;
        let cr_registry = infra_ctx.container_registry();
        let build_platform = infra_ctx.build_platform();

        let _ = services.iter().map(|service| {
            metrics_registry.start_record(*service.long_id(), StepLabel::Service, StepName::BuildQueueing)
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
                        cr_to_engine_error,
                        &mk_logger,
                        metrics_registry.clone(),
                        &should_abort,
                    )
                }
            })
            .collect_vec();

        let builder_threadpool = BuilderThreadPool::new();
        builder_threadpool.run(build_tasks, builder_handle.nb_builder, &should_abort_flag, should_abort)
    }

    fn provision_builder(
        infra_ctx: &InfrastructureContext,
        max_build_in_parallel: usize,
        env_logger: impl Fn(String),
        should_abort: &(dyn Fn() -> bool + Send + Sync),
        build_needs_builpacks: bool,
        services: &Vec<&mut dyn Service>,
        first_service: &&mut dyn Service,
    ) -> Result<BuilderHandle, Box<EngineError>> {
        let builder_handle = {
            let nb_builder = if build_needs_builpacks {
                env_logger("⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️️️".to_string());
                env_logger("⚠️ By using buildpacks you cannot build in parallel. Please migrate to Docker to benefit of parallel builds ⚠️".to_string());
                env_logger("⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️️️".to_string());
                NonZeroUsize::new(1).unwrap()
            } else {
                NonZeroUsize::new(max(min(max_build_in_parallel, services.len()), 1)).unwrap()
            };

            // Compute max resources needed for the builders
            let (max_cpu, max_ram) = services.iter().fold((2000u32, 2u32), |(cpu, ram), s| {
                s.build()
                    .map(|b| (max(cpu, b.max_cpu_in_milli), max(ram, b.max_ram_in_gib)))
                    .unwrap_or((cpu, ram))
            });

            env_logger(format!(
                "🧑‍🏭 Provisioning {nb_builder} docker builder with {max_cpu}m CPU and {max_ram}gib RAM for parallel build. This can take some time"
            ));
            match infra_ctx.context().docker.spawn_builder(
                nb_builder,
                infra_ctx
                    .kubernetes()
                    .cpu_architectures()
                    .iter()
                    .map(docker::Architecture::from)
                    .collect_vec()
                    .as_slice(),
                (max_cpu - 1000, max_cpu),
                (max_ram - 1, max_ram),
                &CommandKiller::from_cancelable(should_abort),
            ) {
                Ok(build_handle) => build_handle,
                Err(err) => {
                    let build_error = to_build_error(first_service.long_id().to_string(), err);
                    let engine_error = build_platform::to_engine_error(
                        first_service.get_event_details(Stage::Environment(EnvironmentStep::BuiltError)),
                        build_error,
                        "Cannot provision docker builder. Please retry later.".to_string(),
                    );
                    return Err(Box::new(engine_error));
                }
            }
        };
        Ok(builder_handle)
    }

    fn build_and_push_service(
        service: &mut dyn Service,
        option: &DeploymentOption,
        cr_registry: &dyn ContainerRegistry,
        build_platform: &dyn BuildPlatform,
        image_retention_time_sec: u32,
        cr_to_engine_error: impl Fn(ContainerRegistryError) -> EngineError,
        mk_logger: impl Fn(&dyn Service) -> EnvLogger,
        metrics_registry: Arc<Box<dyn MetricsRegistry>>,
        should_abort: &dyn Fn() -> bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(service);
        let build = match service.build_mut() {
            Some(build) => build,
            None => return Ok(()), // this case should not happen as we filter on buildable services
        };
        let image_name = build.image.full_image_name_with_tag();

        // If image already exist in the registry, skip the build
        if !option.force_build && cr_registry.does_image_exists(&build.image) {
            let msg = format!("✅ Container image {image_name} already exists and ready to use");
            logger.send_success(msg);
            return Ok(());
        }

        // Be sure that our repository exist before trying to pull/push images from it
        logger.send_progress(format!("🗂️ Provisioning container repository {}", build.image.repository_name()));
        let provision_registry_record = metrics_registry.start_record(
            build.image.service_long_id,
            StepLabel::Service,
            StepName::RegistryCreateRepository,
        );
        match cr_registry.create_repository(build.image.repository_name(), image_retention_time_sec) {
            Err(err) => {
                provision_registry_record.stop(StepStatus::Error);
                return Err(Box::new(cr_to_engine_error(err)));
            }
            Ok(repository_info) => provision_registry_record.stop(if repository_info.created {
                StepStatus::Success
            } else {
                StepStatus::Skip
            }),
        }

        // Ok now everything is setup, we can try to build the app
        let build_result = build_platform.build(build, &logger, metrics_registry.clone(), should_abort);
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
        should_abort: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), Box<EngineError>> {
        let metrics_registry = Arc::new(infra_ctx.kubernetes().metrics_registry().clone_dyn());
        let services = std::iter::empty()
            .chain(environment.applications.iter().map(|x| x.as_service()))
            .chain(environment.containers.iter().map(|x| x.as_service()))
            .chain(environment.routers.iter().map(|x| x.as_service()))
            .chain(environment.databases.iter().map(|x| x.as_service()))
            .chain(environment.jobs.iter().map(|x| x.as_service()))
            .chain(environment.helm_charts.iter().map(|x| x.as_service()));

        for service in services.clone() {
            metrics_registry.start_record(*service.long_id(), StepLabel::Service, StepName::Total);
        }
        let record = metrics_registry.start_record(environment.long_id, StepLabel::Environment, StepName::Total);
        let mut deployed_services: HashSet<Uuid> = HashSet::new();
        let event_details = environment.event_details().clone();
        let run_deploy = || -> Result<(), Box<EngineError>> {
            // Build apps
            if should_abort() {
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
                services_to_build,
                &DeploymentOption {
                    force_build: false,
                    force_push: false,
                },
                infra_ctx,
                environment.max_parallel_build as usize,
                env_logger,
                |srv: &dyn Service| EnvLogger::new(srv, EnvironmentStep::Build, logger.clone()),
                should_abort,
            )?;

            if should_abort() {
                return Err(Box::new(EngineError::new_task_cancellation_requested(event_details)));
            }

            let mut env_deployment = EnvironmentDeployment::new(infra_ctx, &environment, should_abort, logger.clone())?;
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
                record.stop(StepStatus::Success);
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

        record.stop(StepStatus::Error);
        Err(deployment_err)
    }
}

impl Task for EnvironmentTask {
    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn run(&self) {
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

        let deployment_ret =
            EnvironmentTask::deploy_environment(environment, &infra_context, env_logger, &self.cancel_checker());
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
                        r#"
💣 Deployment aborted following a failure to deploy a service. 
This is a general/global message. Look at your services deployment status to know which one made the deployment fail
                        "#
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

        // only store if not running on a workstation
        if env::var("DEPLOY_FROM_FILE_KIND").is_err() {
            match crate::fs::create_workspace_archive(
                infra_context.context().workspace_root_dir(),
                infra_context.context().execution_id(),
            ) {
                Ok(file) => match super::upload_s3_file(
                    infra_context.context(),
                    self.request.archive.as_ref(),
                    file.as_str(),
                    AwsRegion::EuWest3, // TODO(benjaminch): make it customizable
                    self.request.kubernetes.advanced_settings.pleco_resources_ttl,
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
        self.cancel_requested.store(true, Ordering::Relaxed);
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

    fn cancel_checker(&self) -> Box<dyn Fn() -> bool + Send + Sync> {
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
        should_abort_flag: &AtomicBool,
        should_abort: impl Fn() -> bool + Send + Sync,
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
                            should_abort_flag.store(true, Ordering::Relaxed);
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
                        // So it can happens that we got unparked but the thread is not marked as finished yet
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

                if should_abort() {
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
        pool.run(tasks, NonZeroUsize::new(3).unwrap(), &AtomicBool::new(false), || false)
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
        pool.run(tasks, NonZeroUsize::new(3).unwrap(), &AtomicBool::new(false), || false)
            .unwrap();

        assert_eq!(active_tasks.load(Ordering::Relaxed), 0);
        assert_eq!(max_active_task.load(Ordering::Relaxed), 3);

        // Test we get our error, and that we try to stop all tasks on first error
        let mut tasks = Vec::new();
        let active_taks = Arc::new(AtomicUsize::new(0));
        let should_abort_flag = AtomicBool::new(false);
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
        let ret = pool.run(tasks, NonZeroUsize::new(2).unwrap(), &should_abort_flag, || {
            should_abort_flag.load(Ordering::Relaxed)
        });

        assert!(ret.is_err());
        assert_ne!(active_taks.load(Ordering::Relaxed), 10);
    }
}
