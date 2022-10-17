use crate::build_platform;
use crate::build_platform::BuildError;
use crate::cloud_provider::aws::regions::AwsRegion;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service;
use crate::cmd::docker::Docker;
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::to_engine_error;
use crate::deployment_action::deploy_environment::EnvironmentDeployment;
use crate::deployment_task::Task;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage};
use crate::io_models::context::Context;
use crate::io_models::engine_request::EnvironmentEngineRequest;
use crate::io_models::Action;
use crate::logger::Logger;
use crate::models::application::ApplicationService;
use crate::transaction::DeploymentOption;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{env, fs};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct EnvironmentTask {
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<Url>,
    docker: Docker,
    request: EnvironmentEngineRequest,
    cancel_requested: Arc<AtomicBool>,
    logger: Box<dyn Logger>,
}

impl EnvironmentTask {
    pub fn new(
        request: EnvironmentEngineRequest,
        workspace_root_dir: String,
        lib_root_dir: String,
        docker_host: Option<Url>,
        logger: Box<dyn Logger>,
    ) -> Self {
        // FIXME: Remove unwrap/expect
        let docker = Docker::new(docker_host.clone()).expect("Can't init docker builder");

        EnvironmentTask {
            workspace_root_dir,
            lib_root_dir,
            docker_host,
            docker,
            request,
            logger,
            cancel_requested: Arc::new(AtomicBool::from(false)),
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
            self.docker_host.clone(),
            self.request.features.clone(),
            self.request.metadata.clone(),
            self.docker.clone(),
            self.request.event_details(),
        )
    }

    // FIXME: Remove EngineConfig type, there is no use for it
    // merge it with DeploymentTarget type
    fn infrastructure_context(&self) -> InfrastructureContext {
        self.request
            .engine(&self.info_context(), self.request.event_details(), self.logger.clone())
            .map_err(|err| {
                self.logger.log(EngineEvent::Error(err.clone(), None));
                err
            })
            .expect("Can't init engine")
    }

    fn _is_canceled(&self) -> bool {
        self.cancel_requested.load(Ordering::Acquire)
    }

    fn get_event_details(&self, step: EnvironmentStep) -> EventDetails {
        EventDetails::clone_changing_stage(self.request.event_details(), Stage::Environment(step))
    }

    pub fn build_and_push_applications(
        applications: &mut [Box<dyn ApplicationService>],
        option: &DeploymentOption,
        infra_ctx: &InfrastructureContext,
        event_details: EventDetails,
        should_abort: &dyn Fn() -> bool,
    ) -> Result<(), EngineError> {
        // do the same for applications
        let mut apps_to_build = applications
            .iter_mut()
            // build only applications that are set with Action: Create
            .filter(|app| *app.action() == service::Action::Create)
            .collect::<Vec<_>>();

        // If nothing to build, do nothing
        if apps_to_build.is_empty() {
            return Ok(());
        }

        // To convert ContainerError to EngineError
        let cr_to_engine_error = |err: ContainerRegistryError| -> EngineError {
            let event_details =
                EventDetails::clone_changing_stage(event_details.clone(), Stage::Environment(EnvironmentStep::Build));
            to_engine_error(event_details, err)
        };

        // Do setup of registry and be sure we are login to the registry
        let cr_registry = infra_ctx.container_registry();
        cr_registry.create_registry().map_err(cr_to_engine_error)?;

        for app in apps_to_build.iter_mut() {
            // If image already exist in the registry, skip the build
            if !option.force_build && cr_registry.does_image_exists(&app.get_build().image) {
                continue;
            }

            // Be sure that our repository exist before trying to pull/push images from it
            cr_registry
                .create_repository(
                    app.get_build().image.repository_name(),
                    infra_ctx
                        .kubernetes()
                        .advanced_settings()
                        .registry_image_retention_time_sec,
                )
                .map_err(cr_to_engine_error)?;

            // Ok now everything is setup, we can try to build the app
            let build_result = infra_ctx.build_platform().build(app.get_build_mut(), should_abort);

            // logging
            let image_name = app.get_build().image.full_image_name_with_tag();
            let (msg, step) = match &build_result {
                Ok(_) => (
                    format!("✅ Container image {} is built and ready to use", &image_name),
                    EnvironmentStep::Built,
                ),
                Err(BuildError::Aborted { .. }) => (
                    format!("🚫 Container image {} build has been canceled", &image_name),
                    EnvironmentStep::Cancelled,
                ),
                Err(err) => (
                    format!("❌ Container image {} failed to be build: {}", &image_name, err),
                    EnvironmentStep::BuiltError,
                ),
            };

            let event_details = app.get_event_details(Stage::Environment(step));
            infra_ctx
                .build_platform()
                .logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));

            // Abort if it was an error
            let _ = build_result.map_err(|err| build_platform::to_engine_error(event_details, err))?;
        }

        Ok(())
    }

    pub fn deploy_environment(
        mut environment: Environment,
        infra_ctx: &InfrastructureContext,
        should_abort: &dyn Fn() -> bool,
    ) -> Result<(), EngineError> {
        let mut deployed_services: HashSet<Uuid> = HashSet::new();
        let event_details = environment.event_details().clone();
        let run_deploy = || -> Result<(), EngineError> {
            // Build applications if needed
            if environment.action == service::Action::Create {
                if should_abort() {
                    return Err(EngineError::new_task_cancellation_requested(event_details));
                }

                Self::build_and_push_applications(
                    &mut environment.applications,
                    &DeploymentOption {
                        force_build: false,
                        force_push: false,
                    },
                    infra_ctx,
                    event_details.clone(),
                    should_abort,
                )?;
            }

            if should_abort() {
                return Err(EngineError::new_task_cancellation_requested(event_details));
            }

            let mut env_deployment = EnvironmentDeployment::new(infra_ctx, &environment, should_abort)?;
            let deployment_ret = match environment.action {
                service::Action::Create => env_deployment.on_create(),
                service::Action::Pause => env_deployment.on_pause(),
                service::Action::Delete => env_deployment.on_delete(),
                service::Action::Nothing => Ok(()),
            };
            deployed_services = env_deployment.deployed_services;

            deployment_ret
        };

        let deployment_err = match run_deploy() {
            Ok(_) => return Ok(()), // return early if no error
            Err(err) => err,
        };

        // Handle deployment error, send back all correct status
        let to_stage = |action: &service::Action| -> Stage {
            if deployment_err.tag().is_cancel() {
                return Stage::Environment(EnvironmentStep::Cancelled);
            }

            match action {
                service::Action::Create => Stage::Environment(EnvironmentStep::DeployedError),
                service::Action::Pause => Stage::Environment(EnvironmentStep::PausedError),
                service::Action::Delete => Stage::Environment(EnvironmentStep::DeletedError),
                service::Action::Nothing => Stage::Environment(EnvironmentStep::DeployedError),
            }
        };

        let services = std::iter::empty()
            .chain(environment.applications.iter().map(|x| x.as_service()))
            .chain(environment.containers.iter().map(|x| x.as_service()))
            .chain(environment.routers.iter().map(|x| x.as_service()))
            .chain(environment.databases.iter().map(|x| x.as_service()));

        for service in services {
            if deployed_services.contains(service.long_id()) {
                continue;
            }
            infra_ctx.build_platform().logger().log(EngineEvent::Info(
                service.get_event_details(to_stage(service.action())),
                EventMessage::new_from_safe("".to_string()),
            ));
        }

        Err(deployment_err)
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

        let infra_context = self.infrastructure_context();
        let env_step = match self.request.target_environment.action {
            Action::Create => EnvironmentStep::Deploy,
            Action::Pause => EnvironmentStep::Pause,
            Action::Delete => EnvironmentStep::Delete,
            Action::Nothing => EnvironmentStep::Deploy,
        };
        let event_details = self.get_event_details(env_step);
        let environment = match self.request.target_environment.to_environment_domain(
            infra_context.context(),
            infra_context.cloud_provider(),
            infra_context.container_registry(),
            self.logger.clone(),
        ) {
            Ok(env) => env,
            Err(err) => {
                self.logger.log(EngineEvent::Error(
                    EngineError::new_invalid_engine_payload(event_details, err.to_string().as_str()),
                    None,
                ));
                return;
            }
        };

        // run the actions
        let deployment_ret = EnvironmentTask::deploy_environment(environment, &infra_context, &self.cancel_checker());
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
            (_, Err(err)) if err.tag().is_cancel() => self.logger.log(EngineEvent::Info(
                self.get_event_details(EnvironmentStep::Cancelled),
                EventMessage::new("🚫 Deployment has been canceled at user request 🚫".to_string(), None),
            )),
            (Action::Create, Err(_err)) => {
                //self.logger.log(EngineEvent::Error(err, None));
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::DeployedError),
                    EventMessage::new("💣 Deployment failed".to_string(), None),
                ));
            }
            (Action::Pause, Err(_err)) => {
                //self.logger.log(EngineEvent::Error(err, None));
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::PausedError),
                    EventMessage::new("💣 Environment failed to be paused".to_string(), None),
                ));
            }
            (Action::Delete, Err(_err)) => {
                //self.logger.log(EngineEvent::Error(err, None));
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::DeletedError),
                    EventMessage::new("💣 Environment failed to be deleted".to_string(), None),
                ));
            }
            (Action::Nothing, _) => {}
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

    fn cancel_checker(&self) -> Box<dyn Fn() -> bool> {
        let cancel_requested = self.cancel_requested.clone();
        Box::new(move || cancel_requested.load(Ordering::Relaxed))
    }
}
