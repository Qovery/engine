use crate::task_manager::models::{Action, EngineRequest};
use crate::task_manager::scheduler::Task;
use chrono::{DateTime, Utc};
use qovery_engine::build_platform;
use qovery_engine::build_platform::BuildError;
use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::cloud_provider::service;
use qovery_engine::cmd::docker::Docker;
use qovery_engine::container_registry::errors::ContainerRegistryError;
use qovery_engine::container_registry::to_engine_error;
use qovery_engine::deployment_action::deploy_environment::EnvironmentDeployment;
use qovery_engine::engine::EngineConfig;
use qovery_engine::errors::EngineError;
use qovery_engine::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage, Transmitter};
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::logger::Logger;
use qovery_engine::models::application::ApplicationService;
use qovery_engine::transaction::DeploymentOption;
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
    request: EngineRequest,
    cancel_requested: Arc<AtomicBool>,
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

    // FIXME: Remove EngineConfig type, there is no use for it
    // merge it with DeploymentTarget type
    fn engine_config(&self) -> EngineConfig {
        self.request
            .engine(&self.info_context(), self.logger.clone())
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
        // TODO: Add environment name inside EnvironmentRequest
        let env_id = self
            .request
            .target_environment
            .as_ref()
            .map(|target| target.long_id)
            .unwrap_or_default();
        EventDetails::new(
            Some(self.request.cloud_provider.kind.clone()),
            QoveryIdentifier::new(self.request.organization_long_id),
            QoveryIdentifier::new(self.request.cloud_provider.kubernetes.long_id),
            self.request.id.to_string(),
            Stage::Environment(step),
            Transmitter::Environment(env_id, "environment".to_string()),
        )
    }

    fn build_and_push_applications(
        &self,
        applications: &mut [Box<dyn ApplicationService>],
        option: &DeploymentOption,
        engine_config: &EngineConfig,
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
            let event_details = self.get_event_details(EnvironmentStep::Build);
            to_engine_error(event_details, err)
        };

        // Do setup of registry and be sure we are login to the registry
        let cr_registry = engine_config.container_registry();
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
                    engine_config
                        .kubernetes()
                        .advanced_settings()
                        .registry_image_retention_time_sec,
                )
                .map_err(cr_to_engine_error)?;

            // Ok now everything is setup, we can try to build the app
            let build_result = engine_config
                .build_platform()
                .build(app.get_build_mut(), &self.cancel_checker());

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
            self.logger
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));

            // Abort if it was an error
            let _ = build_result.map_err(|err| build_platform::to_engine_error(event_details, err))?;
        }

        Ok(())
    }

    fn build_and_deploy_environment(&self, engine: &EngineConfig) -> Result<(), EngineError> {
        let env_step = match self
            .request
            .target_environment
            .as_ref()
            .map(|x| &x.action)
            .unwrap_or(&qovery_engine::io_models::Action::Create)
        {
            qovery_engine::io_models::Action::Create => EnvironmentStep::Deploy,
            qovery_engine::io_models::Action::Pause => EnvironmentStep::Pause,
            qovery_engine::io_models::Action::Delete => EnvironmentStep::Delete,
            qovery_engine::io_models::Action::Nothing => EnvironmentStep::Deploy,
        };
        let event_details = self.get_event_details(env_step);

        let environment_action = match &self.request.target_environment {
            None => Err(EngineError::new_invalid_engine_payload(
                event_details.clone(),
                "failed to get environment action, self.request.environment_action() returned None variant",
            )),
            Some(ea) => Ok(ea),
        }?;

        let mut environment = environment_action
            .to_environment_domain(
                engine.context(),
                engine.cloud_provider(),
                engine.container_registry(),
                self.logger.clone(),
            )
            .map_err(|err| EngineError::new_invalid_engine_payload(event_details.clone(), err.to_string().as_str()))?;

        let mut deployed_services: HashSet<Uuid> = HashSet::new();
        let should_abort = self.cancel_checker();
        let run_deploy = || -> Result<(), EngineError> {
            // Build applications
            self.build_and_push_applications(
                &mut environment.applications,
                &DeploymentOption {
                    force_build: false,
                    force_push: false,
                },
                engine,
            )?;

            // Deploy environment now that everything is built
            let mut env_deployment = EnvironmentDeployment::new(&engine, &environment, event_details, &should_abort)?;
            let deployment_ret = match environment.action {
                service::Action::Create => env_deployment.on_create(),
                service::Action::Pause => env_deployment.on_pause(),
                service::Action::Delete => env_deployment.on_delete(),
                service::Action::Nothing => Ok(()),
            };
            deployed_services.extend(env_deployment.deployed_services);

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
            service.logger().log(EngineEvent::Info(
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

        // run the actions
        let engine_config = self.engine_config();
        let deployment_ret = self.build_and_deploy_environment(&engine_config);
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
            (Action::Create, Err(err)) => {
                self.logger.log(EngineEvent::Error(err, None));
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::DeployedError),
                    EventMessage::new("💣 Deployment failed".to_string(), None),
                ));
            }
            (Action::Pause, Err(err)) => {
                self.logger.log(EngineEvent::Error(err, None));
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(EnvironmentStep::PausedError),
                    EventMessage::new("💣 Environment failed to be paused".to_string(), None),
                ));
            }
            (Action::Delete, Err(err)) => {
                self.logger.log(EngineEvent::Error(err, None));
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
                engine_config.context().workspace_root_dir(),
                engine_config.context().execution_id(),
            ) {
                Ok(file) => match super::infrastructure_task::upload_s3_file(
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
        self.cancel_requested.store(true, Ordering::Release);
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
        Box::new(move || cancel_requested.load(Ordering::Acquire))
    }
}
