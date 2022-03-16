use std::thread;

use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::Service;
use crate::engine::EngineConfig;
use crate::errors::{EngineError, Tag};
use crate::events::{EngineEvent, EventMessage};
use crate::logger::{LogLevel, Logger};
use crate::models::{
    Action, Environment, EnvironmentAction, EnvironmentError, ListenersHelper, ProgressInfo, ProgressLevel,
    ProgressScope,
};

pub struct Transaction<'a> {
    engine: &'a EngineConfig,
    logger: Box<dyn Logger>,
    steps: Vec<Step<'a>>,
    executed_steps: Vec<Step<'a>>,
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
    ) -> Result<Self, EngineError> {
        let _ = engine.is_valid()?;
        let _ = engine.kubernetes().is_valid()?;

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

    pub fn deploy_environment(&mut self, environment_action: &'a EnvironmentAction) -> Result<(), EnvironmentError> {
        self.deploy_environment_with_options(
            environment_action,
            DeploymentOption {
                force_build: false,
                force_push: false,
            },
        )
    }

    pub fn deploy_environment_with_options(
        &mut self,
        environment_action: &'a EnvironmentAction,
        option: DeploymentOption,
    ) -> Result<(), EnvironmentError> {
        // add build step
        self.steps.push(Step::BuildEnvironment(environment_action, option));

        // add deployment step
        self.steps.push(Step::DeployEnvironment(environment_action));

        Ok(())
    }

    pub fn pause_environment(&mut self, environment_action: &'a EnvironmentAction) -> Result<(), EnvironmentError> {
        self.steps.push(Step::PauseEnvironment(environment_action));
        Ok(())
    }

    pub fn delete_environment(&mut self, environment_action: &'a EnvironmentAction) -> Result<(), EnvironmentError> {
        self.steps.push(Step::DeleteEnvironment(environment_action));
        Ok(())
    }

    fn build_and_push_applications(
        &self,
        environment: &Environment,
        option: &DeploymentOption,
    ) -> Result<(), EngineError> {
        // do the same for applications
        let apps_to_build = environment
            .applications
            .iter()
            // build only applications that are set with Action: Create
            .filter(|app| app.action == Action::Create)
            .collect::<Vec<_>>();

        // If nothing to build, do nothing
        if apps_to_build.is_empty() {
            return Ok(());
        }

        // Do setup of registry and be sure we are login to the registry
        let cr_registry = self.engine.container_registry();
        let _ = cr_registry.create_registry()?;
        let registry = self.engine.container_registry().login()?;

        for app in apps_to_build.into_iter() {
            let app_build = app.to_build(&registry);

            // If image already exist in the registry, skip the build
            if !option.force_build && cr_registry.does_image_exists(&app_build.image) {
                continue;
            }

            // Be sure that our repository exist before trying to pull/push images from it
            let _ = self.engine.container_registry().create_repository(&app.name)?;

            // Ok now everything is setup, we can try to build the app
            let _ = self
                .engine
                .build_platform()
                .build(app_build, &self.is_transaction_aborted)?;
        }

        Ok(())
    }

    pub fn rollback(&self) -> Result<(), RollbackError> {
        for step in self.executed_steps.iter() {
            match step {
                Step::CreateKubernetes => {
                    // revert kubernetes creation
                    if let Err(err) = self.engine.kubernetes().on_create_error() {
                        return Err(RollbackError::CommitError(err));
                    };
                }
                Step::DeleteKubernetes => {
                    // revert kubernetes deletion
                    if let Err(err) = self.engine.kubernetes().on_delete_error() {
                        return Err(RollbackError::CommitError(err));
                    };
                }
                Step::PauseKubernetes => {
                    // revert pause
                    if let Err(err) = self.engine.kubernetes().on_pause_error() {
                        return Err(RollbackError::CommitError(err));
                    };
                }
                Step::BuildEnvironment(_environment_action, _option) => {
                    // revert build applications
                }
                Step::DeployEnvironment(environment_action) => {
                    // revert environment deployment
                    self.rollback_environment(*environment_action)?;
                }
                Step::PauseEnvironment(environment_action) => {
                    self.rollback_environment(*environment_action)?;
                }
                Step::DeleteEnvironment(environment_action) => {
                    self.rollback_environment(*environment_action)?;
                }
            }
        }

        Ok(())
    }

    /// This function is a wrapper to correctly revert all changes of an attempted deployment AND
    /// if a failover environment is provided, then rollback.
    fn rollback_environment(&self, _environment_action: &EnvironmentAction) -> Result<(), RollbackError> {
        Ok(())
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
                Step::BuildEnvironment(environment_action, option) => {
                    if (self.is_transaction_aborted)() {
                        return TransactionResult::Canceled;
                    }

                    // build applications
                    let target_environment = match environment_action {
                        EnvironmentAction::Environment(te) => te,
                    };

                    match self.build_and_push_applications(target_environment, &option) {
                        Ok(apps) => apps,
                        Err(engine_err) => {
                            self.logger.log(
                                LogLevel::Error,
                                EngineEvent::Error(
                                    engine_err.clone(),
                                    Some(EventMessage::new_from_safe(
                                        "ROLLBACK STARTED! an error occurred".to_string(),
                                    )),
                                ),
                            );

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
                    match self.commit_environment(environment_action, |qe_env| {
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
                    match self.commit_environment(environment_action, |qe_env| {
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
                    match self.commit_environment(environment_action, |qe_env| {
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

        // 100 ms sleep to avoid race condition on last service status update
        // Otherwise, the last status sent to the CORE is (sometimes) not the right one.
        // Even by storing data at the micro seconds precision
        thread::sleep(std::time::Duration::from_millis(100));

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

    fn commit_environment<F>(&self, environment_action: &EnvironmentAction, action_fn: F) -> TransactionResult
    where
        F: Fn(&crate::cloud_provider::environment::Environment) -> Result<(), EngineError>,
    {
        let target_environment = match environment_action {
            EnvironmentAction::Environment(te) => te,
        };

        let registry_info = self.engine.container_registry().login().unwrap();
        let qe_environment = target_environment.to_qe_environment(
            self.engine.context(),
            self.engine.cloud_provider(),
            &registry_info,
            self.logger.clone(),
        );

        let execution_id = self.engine.context().execution_id();

        // send back the right progress status
        fn send_progress<T>(
            kubernetes: &dyn Kubernetes,
            action: &Action,
            service: &Box<T>,
            execution_id: &str,
            is_error: bool,
        ) where
            T: Service + ?Sized,
        {
            let lh = ListenersHelper::new(kubernetes.listeners());
            let progress_info = ProgressInfo::new(
                service.progress_scope(),
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

        // 100 ms sleep to avoid race condition on last service status update
        // Otherwise, the last status sent to the CORE is (sometimes) not the right one.
        // Even by storing data at the micro seconds precision
        thread::sleep(std::time::Duration::from_millis(100));

        let _ = match action_fn(&qe_environment) {
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
                for service in &qe_environment.stateful_services {
                    send_progress(
                        self.engine.kubernetes(),
                        &target_environment.action,
                        service,
                        execution_id,
                        true,
                    );
                }

                for service in &qe_environment.stateless_services {
                    send_progress(
                        self.engine.kubernetes(),
                        &target_environment.action,
                        service,
                        execution_id,
                        true,
                    );
                }

                return rollback_result;
            }
            _ => {
                // terminal update
                for service in &qe_environment.stateful_services {
                    send_progress(
                        self.engine.kubernetes(),
                        &target_environment.action,
                        service,
                        execution_id,
                        false,
                    );
                }

                for service in &qe_environment.stateless_services {
                    send_progress(
                        self.engine.kubernetes(),
                        &target_environment.action,
                        service,
                        execution_id,
                        false,
                    );
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

pub enum Step<'a> {
    // init and create all the necessary resources (Network, Kubernetes)
    CreateKubernetes,
    DeleteKubernetes,
    PauseKubernetes,
    BuildEnvironment(&'a EnvironmentAction, DeploymentOption),
    DeployEnvironment(&'a EnvironmentAction),
    PauseEnvironment(&'a EnvironmentAction),
    DeleteEnvironment(&'a EnvironmentAction),
}

impl<'a> Step<'a> {
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

impl<'a> Clone for Step<'a> {
    fn clone(&self) -> Self {
        match self {
            Step::CreateKubernetes => Step::CreateKubernetes,
            Step::DeleteKubernetes => Step::DeleteKubernetes,
            Step::PauseKubernetes => Step::PauseKubernetes,
            Step::BuildEnvironment(e, option) => Step::BuildEnvironment(*e, option.clone()),
            Step::DeployEnvironment(e) => Step::DeployEnvironment(*e),
            Step::PauseEnvironment(e) => Step::PauseEnvironment(*e),
            Step::DeleteEnvironment(e) => Step::DeleteEnvironment(*e),
        }
    }
}

#[derive(Debug)]
pub enum RollbackError {
    CommitError(EngineError),
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
