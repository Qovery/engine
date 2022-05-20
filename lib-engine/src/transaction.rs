use crate::build_platform::BuildError;
use crate::cloud_provider::environment::Environment;
use std::cell::RefCell;
use std::rc::Rc;

use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{Action, Service};
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::to_engine_error;
use crate::engine::{EngineConfig, EngineConfigError};
use crate::errors::{EngineError, Tag};
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage, Transmitter};
use crate::io_models::{
    EnvironmentError, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope, QoveryIdentifier,
};
use crate::logger::Logger;
use crate::models::application::ApplicationService;

pub struct Transaction<'a> {
    engine: &'a EngineConfig,
    logger: Box<dyn Logger>,
    steps: Vec<Step>,
    executed_steps: Vec<Step>,
    current_step: StepName,
    is_transaction_aborted: Box<dyn Fn() -> bool>,
    on_step_change: Box<dyn Fn(&StepName)>,
}

impl<'a> Transaction<'a> {
    pub fn new(
        engine: &'a EngineConfig,
        logger: Box<dyn Logger>,
        is_transaction_aborted: Box<dyn Fn() -> bool>,
        on_step_change: Box<dyn Fn(&StepName)>,
    ) -> Result<Self, EngineConfigError> {
        let _ = engine.is_valid()?;
        if let Err(e) = engine.kubernetes().is_valid() {
            return Err(EngineConfigError::KubernetesNotValid(e));
        }

        let mut tx = Transaction::<'a> {
            engine,
            logger,
            steps: vec![],
            executed_steps: vec![],
            current_step: StepName::Waiting,
            is_transaction_aborted,
            on_step_change,
        };
        tx.set_current_step(StepName::Waiting);

        Ok(tx)
    }

    fn get_event_details(&self, stage: Stage, transmitter: Transmitter) -> EventDetails {
        let context = self.engine.context();
        EventDetails::new(
            None,
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            None,
            stage,
            transmitter,
        )
    }

    pub fn set_current_step(&mut self, step: StepName) {
        (self.on_step_change)(&step);
        self.current_step = step;
    }

    pub fn create_kubernetes(&mut self) -> Result<(), EngineError> {
        self.steps.push(Step::CreateKubernetes);
        Ok(())
    }

    pub fn pause_kubernetes(&mut self) -> Result<(), EngineError> {
        self.steps.push(Step::PauseKubernetes);
        Ok(())
    }

    pub fn delete_kubernetes(&mut self) -> Result<(), EngineError> {
        self.steps.push(Step::DeleteKubernetes);
        Ok(())
    }

    pub fn deploy_environment(&mut self, environment: &Rc<RefCell<Environment>>) -> Result<(), EnvironmentError> {
        self.deploy_environment_with_options(
            environment,
            DeploymentOption {
                force_build: false,
                force_push: false,
            },
        )
    }

    pub fn build_environment(
        &mut self,
        environment: &Rc<RefCell<Environment>>,
        option: DeploymentOption,
    ) -> Result<(), EnvironmentError> {
        self.steps.push(Step::BuildEnvironment(environment.clone(), option));

        Ok(())
    }

    pub fn deploy_environment_with_options(
        &mut self,
        environment: &Rc<RefCell<Environment>>,
        option: DeploymentOption,
    ) -> Result<(), EnvironmentError> {
        // add build step
        self.build_environment(environment, option)?;

        // add deployment step
        self.steps.push(Step::DeployEnvironment(environment.clone()));

        Ok(())
    }

    pub fn pause_environment(&mut self, environment: &Rc<RefCell<Environment>>) -> Result<(), EnvironmentError> {
        self.steps.push(Step::PauseEnvironment(environment.clone()));
        Ok(())
    }

    pub fn delete_environment(&mut self, environment: &Rc<RefCell<Environment>>) -> Result<(), EnvironmentError> {
        self.steps.push(Step::DeleteEnvironment(environment.clone()));
        Ok(())
    }

    fn build_and_push_applications(
        &self,
        applications: &mut [Box<dyn ApplicationService>],
        option: &DeploymentOption,
    ) -> Result<(), EngineError> {
        // do the same for applications
        let mut apps_to_build = applications
            .iter_mut()
            // build only applications that are set with Action: Create
            .filter(|app| *app.action() == Action::Create)
            .collect::<Vec<_>>();

        // If nothing to build, do nothing
        if apps_to_build.is_empty() {
            return Ok(());
        }

        // To convert ContainerError to EngineError
        let cr_to_engine_error = |err: ContainerRegistryError| -> EngineError {
            let event_details = self.get_event_details(
                Stage::Environment(EnvironmentStep::Build),
                Transmitter::ContainerRegistry(
                    self.engine.container_registry().id().to_string(),
                    self.engine.container_registry().name().to_string(),
                ),
            );
            to_engine_error(event_details, err)
        };

        let build_event_details = || -> EventDetails {
            self.get_event_details(
                Stage::Environment(EnvironmentStep::Build),
                Transmitter::BuildPlatform(
                    self.engine.build_platform().id().to_string(),
                    self.engine.build_platform().name().to_string(),
                ),
            )
        };

        // Do setup of registry and be sure we are login to the registry
        let cr_registry = self.engine.container_registry();
        let _ = cr_registry.create_registry().map_err(cr_to_engine_error)?;

        for app in apps_to_build.iter_mut() {
            // If image already exist in the registry, skip the build
            if !option.force_build && cr_registry.does_image_exists(&app.get_build().image) {
                continue;
            }

            // Be sure that our repository exist before trying to pull/push images from it
            let _ = self
                .engine
                .container_registry()
                .create_repository(app.get_build().image.repository_name())
                .map_err(cr_to_engine_error)?;

            // Ok now everything is setup, we can try to build the app
            let build_result = self
                .engine
                .build_platform()
                .build(app.get_build_mut(), &self.is_transaction_aborted);

            // logging
            let image_name = app.get_build().image.full_image_name_with_tag();
            let msg = match &build_result {
                Ok(_) => format!("✅ Container image {} is built and ready to use", &image_name),
                Err(BuildError::Aborted { .. }) => {
                    format!("🚫 Container image {} build has been canceled", &image_name)
                }
                Err(err) => format!("❌ Container image {} failed to be build: {}", &image_name, err),
            };

            let progress_info = ProgressInfo::new(
                ProgressScope::Application {
                    id: app.id().to_string(),
                },
                match build_result.is_ok() {
                    true => ProgressLevel::Info,
                    false => ProgressLevel::Error,
                },
                Some(msg.to_string()),
                self.engine.context().execution_id(),
            );
            ListenersHelper::new(self.engine.build_platform().listeners()).deployment_in_progress(progress_info);

            let event_details = build_event_details();
            self.logger
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));

            // Abort if it was an error
            let _ = build_result.map_err(|err| crate::build_platform::to_engine_error(event_details, err))?;
        }

        Ok(())
    }

    pub fn rollback(&self) -> Result<(), RollbackError> {
        for step in self.executed_steps.iter() {
            match step {
                Step::CreateKubernetes => {
                    // revert kubernetes creation
                    if let Err(err) = self.engine.kubernetes().on_create_error() {
                        return Err(RollbackError::CommitError(Box::new(err)));
                    };
                }
                Step::DeleteKubernetes => {
                    // revert kubernetes deletion
                    if let Err(err) = self.engine.kubernetes().on_delete_error() {
                        return Err(RollbackError::CommitError(Box::new(err)));
                    };
                }
                Step::PauseKubernetes => {
                    // revert pause
                    if let Err(err) = self.engine.kubernetes().on_pause_error() {
                        return Err(RollbackError::CommitError(Box::new(err)));
                    };
                }
                Step::BuildEnvironment(_environment_action, _option) => {
                    // revert build applications
                }
                Step::DeployEnvironment(environment_action) => {
                    // revert environment deployment
                    self.rollback_environment(&(environment_action.as_ref().borrow()))?;
                }
                Step::PauseEnvironment(environment_action) => {
                    self.rollback_environment(&(environment_action.as_ref().borrow()))?;
                }
                Step::DeleteEnvironment(environment_action) => {
                    self.rollback_environment(&(environment_action.as_ref().borrow()))?;
                }
            }
        }

        Ok(())
    }

    // Warning: This function function does not revert anything, it just there to grab info from kube and services if it fails
    // FIXME: Cleanup this, qe_environment should not be rebuilt at this step
    fn rollback_environment(&self, environment: &Environment) -> Result<(), RollbackError> {
        let action = match environment.action {
            Action::Create => self.engine.kubernetes().deploy_environment_error(environment),
            Action::Pause => self.engine.kubernetes().pause_environment_error(environment),
            Action::Delete => self.engine.kubernetes().delete_environment_error(environment),
            Action::Nothing => Ok(()),
        };

        let _ = match action {
            Ok(_) => {}
            Err(err) => return Err(RollbackError::CommitError(Box::new(err))),
        };

        Err(RollbackError::NoFailoverEnvironment)
    }

    pub fn commit(mut self) -> TransactionResult {
        for step in self.steps.clone().into_iter() {
            // execution loop
            self.executed_steps.push(step.clone());
            self.set_current_step(step.step_name());

            match step {
                Step::CreateKubernetes => {
                    // create kubernetes
                    match self.commit_infrastructure(Action::Create, self.engine.kubernetes().on_create()) {
                        TransactionResult::Ok => {}
                        err => {
                            error!("Error while creating infrastructure: {:?}", err);
                            return err;
                        }
                    };
                }
                Step::DeleteKubernetes => {
                    // delete kubernetes
                    match self.commit_infrastructure(Action::Delete, self.engine.kubernetes().on_delete()) {
                        TransactionResult::Ok => {}
                        err => {
                            error!("Error while deleting infrastructure: {:?}", err);
                            return err;
                        }
                    };
                }
                Step::PauseKubernetes => {
                    // pause kubernetes
                    match self.commit_infrastructure(Action::Pause, self.engine.kubernetes().on_pause()) {
                        TransactionResult::Ok => {}
                        err => {
                            error!("Error while pausing infrastructure: {:?}", err);
                            return err;
                        }
                    };
                }
                Step::BuildEnvironment(environment, option) => {
                    if (self.is_transaction_aborted)() {
                        return TransactionResult::Canceled;
                    }

                    // build applications
                    let applications = &mut (environment.as_ref().borrow_mut()).applications;
                    match self.build_and_push_applications(applications, &option) {
                        Ok(apps) => apps,
                        Err(engine_err) => {
                            self.logger.log(EngineEvent::Error(
                                engine_err.clone(),
                                Some(EventMessage::new_from_safe("ROLLBACK STARTED! an error occurred".to_string())),
                            ));

                            return if engine_err.tag() == &Tag::TaskCancellationRequested {
                                TransactionResult::Canceled
                            } else {
                                TransactionResult::Rollback(engine_err)
                            };
                        }
                    };
                }
                Step::DeployEnvironment(environment_action) => {
                    if (self.is_transaction_aborted)() {
                        return TransactionResult::Canceled;
                    }

                    // deploy complete environment
                    match self.commit_environment(&(environment_action.as_ref().borrow()), |qe_env| {
                        self.engine.kubernetes().deploy_environment(qe_env)
                    }) {
                        TransactionResult::Ok => {}
                        err => {
                            error!("Error while deploying environment: {:?}", err);
                            return err;
                        }
                    };
                }
                Step::PauseEnvironment(environment_action) => {
                    if (self.is_transaction_aborted)() {
                        return TransactionResult::Canceled;
                    }

                    // pause complete environment
                    match self.commit_environment(&(environment_action.as_ref().borrow()), |qe_env| {
                        self.engine.kubernetes().pause_environment(qe_env)
                    }) {
                        TransactionResult::Ok => {}
                        err => {
                            error!("Error while pausing environment: {:?}", err);
                            return err;
                        }
                    };
                }
                Step::DeleteEnvironment(environment_action) => {
                    if (self.is_transaction_aborted)() {
                        return TransactionResult::Canceled;
                    }

                    // delete complete environment
                    match self.commit_environment(&(environment_action.as_ref().borrow()), |qe_env| {
                        self.engine.kubernetes().delete_environment(qe_env)
                    }) {
                        TransactionResult::Ok => {}
                        err => {
                            error!("Error while deleting environment: {:?}", err);
                            return err;
                        }
                    };
                }
            };
        }

        TransactionResult::Ok
    }

    fn commit_infrastructure(&self, action: Action, result: Result<(), EngineError>) -> TransactionResult {
        // send back the right progress status
        fn send_progress(lh: &ListenersHelper, action: Action, execution_id: &str, is_error: bool) {
            let progress_info = ProgressInfo::new(
                ProgressScope::Infrastructure {
                    execution_id: execution_id.to_string(),
                },
                ProgressLevel::Info,
                None::<&str>,
                execution_id,
            );

            if !is_error {
                match action {
                    Action::Create => lh.deployed(progress_info),
                    Action::Pause => lh.paused(progress_info),
                    Action::Delete => lh.deleted(progress_info),
                    Action::Nothing => {} // nothing to do here?
                };
                return;
            }

            match action {
                Action::Create => lh.deployment_error(progress_info),
                Action::Pause => lh.pause_error(progress_info),
                Action::Delete => lh.delete_error(progress_info),
                Action::Nothing => {} // nothing to do here?
            };
        }

        let execution_id = self.engine.context().execution_id();
        let lh = ListenersHelper::new(self.engine.kubernetes().listeners());

        match result {
            Err(err) => {
                warn!("infrastructure ROLLBACK STARTED! an error occurred {:?}", err);
                match self.rollback() {
                    Ok(_) => {
                        // an error occurred on infrastructure deployment BUT rolledback is OK
                        send_progress(&lh, action, execution_id, true);
                        TransactionResult::Rollback(err)
                    }
                    Err(e) => {
                        // an error occurred on infrastructure deployment AND rolledback is KO
                        error!("infrastructure ROLLBACK FAILED! fatal error: {:?}", e);
                        send_progress(&lh, action, execution_id, true);
                        TransactionResult::UnrecoverableError(err, e)
                    }
                }
            }
            _ => {
                // infrastructure deployment OK
                send_progress(&lh, action, execution_id, false);
                TransactionResult::Ok
            }
        }
    }

    fn commit_environment<F>(&self, environment: &Environment, action_fn: F) -> TransactionResult
    where
        F: Fn(&Environment) -> Result<(), EngineError>,
    {
        let execution_id = self.engine.context().execution_id();

        // send back the right progress status
        fn send_progress<T>(
            kubernetes: &dyn Kubernetes,
            action: &Action,
            service: &T,
            execution_id: &str,
            is_error: bool,
        ) where
            T: Service + ?Sized,
        {
            let lh = ListenersHelper::new(kubernetes.listeners());
            let progress_info =
                ProgressInfo::new(service.progress_scope(), ProgressLevel::Info, None::<&str>, execution_id);

            if !is_error {
                match action {
                    Action::Create => lh.deployed(progress_info),
                    Action::Pause => lh.paused(progress_info),
                    Action::Delete => lh.deleted(progress_info),
                    Action::Nothing => {} // nothing to do here?
                };
                return;
            }

            match action {
                Action::Create => lh.deployment_error(progress_info),
                Action::Pause => lh.pause_error(progress_info),
                Action::Delete => lh.delete_error(progress_info),
                Action::Nothing => {} // nothing to do here?
            };
        }

        let _ = match action_fn(environment) {
            Err(err) => {
                let rollback_result = match self.rollback() {
                    Ok(_) => TransactionResult::Rollback(err),
                    Err(rollback_err) => {
                        error!("ROLLBACK FAILED! fatal error: {:?}", rollback_err);
                        TransactionResult::UnrecoverableError(err, rollback_err)
                    }
                };

                // !!! don't change the order
                // terminal update
                for service in environment.stateful_services() {
                    send_progress(self.engine.kubernetes(), &environment.action, service, execution_id, true);
                }

                for service in environment.stateless_services() {
                    send_progress(self.engine.kubernetes(), &environment.action, service, execution_id, true);
                }

                return rollback_result;
            }
            _ => {
                // terminal update
                for service in environment.stateful_services() {
                    send_progress(self.engine.kubernetes(), &environment.action, service, execution_id, false);
                }

                for service in environment.stateless_services() {
                    send_progress(self.engine.kubernetes(), &environment.action, service, execution_id, false);
                }
            }
        };

        TransactionResult::Ok
    }
}

#[derive(Clone)]
pub struct DeploymentOption {
    pub force_build: bool,
    pub force_push: bool,
}

#[derive(Clone)]
pub enum StepName {
    CreateKubernetes,
    DeleteKubernetes,
    PauseKubernetes,
    BuildEnvironment,
    DeployEnvironment,
    PauseEnvironment,
    DeleteEnvironment,
    Waiting,
}

impl StepName {
    pub fn can_be_canceled(&self) -> bool {
        match self {
            StepName::CreateKubernetes => false,
            StepName::DeleteKubernetes => false,
            StepName::PauseKubernetes => false,
            StepName::DeployEnvironment => false,
            StepName::PauseEnvironment => false,
            StepName::DeleteEnvironment => false,
            StepName::BuildEnvironment => true,
            StepName::Waiting => true,
        }
    }
}

pub enum Step {
    // init and create all the necessary resources (Network, Kubernetes)
    CreateKubernetes,
    DeleteKubernetes,
    PauseKubernetes,
    BuildEnvironment(Rc<RefCell<Environment>>, DeploymentOption),
    DeployEnvironment(Rc<RefCell<Environment>>),
    PauseEnvironment(Rc<RefCell<Environment>>),
    DeleteEnvironment(Rc<RefCell<Environment>>),
}

impl Step {
    fn step_name(&self) -> StepName {
        match self {
            Step::CreateKubernetes => StepName::CreateKubernetes,
            Step::DeleteKubernetes => StepName::DeleteKubernetes,
            Step::PauseKubernetes => StepName::PauseKubernetes,
            Step::BuildEnvironment(_, _) => StepName::BuildEnvironment,
            Step::DeployEnvironment(_) => StepName::DeployEnvironment,
            Step::PauseEnvironment(_) => StepName::PauseEnvironment,
            Step::DeleteEnvironment(_) => StepName::DeleteEnvironment,
        }
    }
}

impl Clone for Step {
    fn clone(&self) -> Self {
        match self {
            Step::CreateKubernetes => Step::CreateKubernetes,
            Step::DeleteKubernetes => Step::DeleteKubernetes,
            Step::PauseKubernetes => Step::PauseKubernetes,
            Step::BuildEnvironment(e, option) => Step::BuildEnvironment(e.clone(), option.clone()),
            Step::DeployEnvironment(e) => Step::DeployEnvironment(e.clone()),
            Step::PauseEnvironment(e) => Step::PauseEnvironment(e.clone()),
            Step::DeleteEnvironment(e) => Step::DeleteEnvironment(e.clone()),
        }
    }
}

#[derive(Debug)]
pub enum RollbackError {
    CommitError(Box<EngineError>),
    NoFailoverEnvironment,
    Nothing,
}

#[derive(Debug)]
pub enum TransactionResult {
    Ok,
    Canceled,
    Rollback(EngineError),
    UnrecoverableError(EngineError, RollbackError),
}
