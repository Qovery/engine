use std::collections::HashMap;
use std::thread;

use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{Application, Service};
use crate::container_registry::PushResult;
use crate::engine::Engine;
use crate::error::EngineError;
use crate::models::{
    Action, Environment, EnvironmentAction, EnvironmentError, ListenersHelper, ProgressInfo,
    ProgressLevel,
};

pub struct Transaction<'a> {
    engine: &'a Engine,
    steps: Vec<Step<'a>>,
    executed_steps: Vec<Step<'a>>,
}

impl<'a> Transaction<'a> {
    pub fn new(engine: &'a Engine) -> Self {
        Transaction::<'a> {
            engine,
            steps: vec![],
            executed_steps: vec![],
        }
    }

    pub fn create_kubernetes(&mut self, kubernetes: &'a dyn Kubernetes) -> Result<(), EngineError> {
        match kubernetes.is_valid() {
            Ok(_) => {
                self.steps.push(Step::CreateKubernetes(kubernetes));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn delete_kubernetes(&mut self, kubernetes: &'a dyn Kubernetes) -> Result<(), EngineError> {
        match kubernetes.is_valid() {
            Ok(_) => {
                self.steps.push(Step::DeleteKubernetes(kubernetes));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn deploy_environment(
        &mut self,
        kubernetes: &'a dyn Kubernetes,
        environment_action: &'a EnvironmentAction,
    ) -> Result<(), EnvironmentError> {
        self.deploy_environment_with_options(
            kubernetes,
            environment_action,
            DeploymentOption {
                force_build: false,
                force_push: false,
            },
        )
    }

    pub fn deploy_environment_with_options(
        &mut self,
        kubernetes: &'a dyn Kubernetes,
        environment_action: &'a EnvironmentAction,
        option: DeploymentOption,
    ) -> Result<(), EnvironmentError> {
        let _ = self.check_environment_action(environment_action)?;

        // add build step
        self.steps
            .push(Step::BuildEnvironment(environment_action, option));

        // add deployment step
        self.steps
            .push(Step::DeployEnvironment(kubernetes, environment_action));

        Ok(())
    }

    pub fn pause_environment(
        &mut self,
        kubernetes: &'a dyn Kubernetes,
        environment_action: &'a EnvironmentAction,
    ) -> Result<(), EnvironmentError> {
        let _ = self.check_environment_action(environment_action)?;

        self.steps
            .push(Step::PauseEnvironment(kubernetes, environment_action));
        Ok(())
    }

    pub fn delete_environment(
        &mut self,
        kubernetes: &'a dyn Kubernetes,
        environment_action: &'a EnvironmentAction,
    ) -> Result<(), EnvironmentError> {
        let _ = self.check_environment_action(environment_action)?;

        self.steps
            .push(Step::DeleteEnvironment(kubernetes, environment_action));
        Ok(())
    }

    fn check_environment_action(
        &self,
        environment_action: &EnvironmentAction,
    ) -> Result<(), EnvironmentError> {
        match environment_action {
            EnvironmentAction::Environment(te) => match te.is_valid() {
                Ok(_) => {}
                Err(err) => return Err(err),
            },
            EnvironmentAction::EnvironmentWithFailover(te, fe) => {
                match te.is_valid() {
                    Ok(_) => {}
                    Err(err) => return Err(err),
                };

                match fe.is_valid() {
                    Ok(_) => {}
                    Err(err) => return Err(err),
                };
            }
        };

        Ok(())
    }

    fn _build_applications(
        &self,
        environment: &Environment,
        option: &DeploymentOption,
    ) -> Result<Vec<Box<dyn Application>>, EngineError> {
        let external_services_to_build = environment
            .external_services
            .iter()
            // build only applications that are set with Action: Create
            .filter(|es| es.action == Action::Create)
            .filter(|es| {
                // get useful services only
                if option.force_build {
                    // forcing build means building all services
                    true
                } else {
                    let image = es.to_image();
                    // return service only if it does not exist on the targeted container registry
                    !self.engine.container_registry().does_image_exists(&image)
                }
            });

        let external_service_and_result_tuples = external_services_to_build
            .map(|es| {
                (
                    es,
                    self.engine
                        .build_platform()
                        .build(es.to_build(), option.force_build),
                )
            })
            .collect::<Vec<_>>();

        // do the same for applications

        let apps_to_build = environment
            .applications
            .iter()
            // build only applications that are set with Action: Create
            .filter(|app| app.action == Action::Create)
            .filter(|app| {
                // get useful services only
                if option.force_build {
                    // forcing build means building all services
                    true
                } else {
                    let image = app.to_image();
                    // return service only if it does not exist on the targeted container registry
                    !self.engine.container_registry().does_image_exists(&image)
                }
            });

        let application_and_result_tuples = apps_to_build
            .map(|app| {
                (
                    app,
                    self.engine
                        .build_platform()
                        .build(app.to_build(), option.force_build),
                )
            })
            .collect::<Vec<_>>();

        let mut applications: Vec<Box<dyn Application>> =
            Vec::with_capacity(application_and_result_tuples.len());

        for (external_service, result) in external_service_and_result_tuples {
            // catch build error, can't do it in Fn
            let build_result = match result {
                Err(err) => {
                    error!(
                        "build error for external_service {}: {:?}",
                        external_service.id.as_str(),
                        err
                    );
                    return Err(err);
                }
                Ok(build_result) => build_result,
            };

            match external_service.to_application(
                self.engine.context(),
                &build_result.build.image,
                self.engine.cloud_provider(),
            ) {
                Some(x) => applications.push(x),
                None => {}
            }
        }

        for (application, result) in application_and_result_tuples {
            // catch build error, can't do it in Fn
            let build_result = match result {
                Err(err) => {
                    error!(
                        "build error for application {}: {:?}",
                        application.id.as_str(),
                        err
                    );
                    return Err(err);
                }
                Ok(build_result) => build_result,
            };

            match application.to_application(
                self.engine.context(),
                &build_result.build.image,
                self.engine.cloud_provider(),
            ) {
                Some(x) => applications.push(x),
                None => {}
            }
        }

        Ok(applications)
    }

    fn _push_applications(
        &self,
        applications: Vec<Box<dyn Application>>,
        option: &DeploymentOption,
    ) -> Result<Vec<(Box<dyn Application>, PushResult)>, EngineError> {
        let application_and_push_results: Vec<_> = applications
            .into_iter()
            .map(|mut app| {
                match self
                    .engine
                    .container_registry()
                    .push(app.image(), option.force_push)
                {
                    Ok(push_result) => {
                        // I am not a big fan of doing that but it's the most effective way
                        app.set_image(push_result.image.clone());
                        Ok((app, push_result))
                    }
                    Err(err) => Err(err),
                }
            })
            .collect();

        let mut results: Vec<(Box<dyn Application>, PushResult)> = vec![];
        for result in application_and_push_results.into_iter() {
            match result {
                Ok(tuple) => results.push(tuple),
                Err(err) => {
                    error!("error pushing docker image {:?}", err);
                    return Err(err);
                }
            }
        }

        Ok(results)
    }

    fn check_environment(
        &self,
        environment: &crate::cloud_provider::environment::Environment,
    ) -> TransactionResult {
        match environment.is_valid() {
            Err(engine_error) => {
                warn!("ROLLBACK STARTED! an error occurred {:?}", engine_error);
                return match self.rollback() {
                    Ok(_) => TransactionResult::Rollback(engine_error),
                    Err(err) => {
                        error!("ROLLBACK FAILED! fatal error: {:?}", err);
                        TransactionResult::UnrecoverableError(engine_error, err)
                    }
                };
            }
            _ => {}
        };

        TransactionResult::Ok
    }

    pub fn rollback(&self) -> Result<(), RollbackError> {
        for step in self.executed_steps.iter() {
            match step {
                Step::CreateKubernetes(kubernetes) => {
                    // revert kubernetes creation
                    match kubernetes.on_create_error() {
                        Err(err) => return Err(RollbackError::CommitError(err)),
                        _ => {}
                    };
                }
                Step::DeleteKubernetes(kubernetes) => {
                    // revert kubernetes deletion
                    match kubernetes.on_delete_error() {
                        Err(err) => return Err(RollbackError::CommitError(err)),
                        _ => {}
                    };
                }
                Step::BuildEnvironment(_environment_action, _option) => {
                    // revert build applications
                }
                Step::DeployEnvironment(kubernetes, environment_action) => {
                    // revert environment deployment
                    self.rollback_environment(*kubernetes, *environment_action)?;
                }
                Step::PauseEnvironment(kubernetes, environment_action) => {
                    self.rollback_environment(*kubernetes, *environment_action)?;
                }
                Step::DeleteEnvironment(kubernetes, environment_action) => {
                    self.rollback_environment(*kubernetes, *environment_action)?;
                }
            }
        }

        Ok(())
    }

    /// This function is a wrapper to correctly revert all changes of an attempted deployment AND
    /// if a failover environment is provided, then rollback.
    fn rollback_environment(
        &self,
        kubernetes: &dyn Kubernetes,
        environment_action: &EnvironmentAction,
    ) -> Result<(), RollbackError> {
        let qe_environment = |environment: &Environment| {
            let mut _applications = Vec::with_capacity(
                // ExternalService impl Application (which is a StatelessService)
                environment.applications.len() + environment.external_services.len(),
            );

            for application in environment.applications.iter() {
                let build = application.to_build();

                match application.to_application(
                    self.engine.context(),
                    &build.image,
                    self.engine.cloud_provider(),
                ) {
                    Some(x) => _applications.push(x),
                    None => {}
                }
            }

            for external_service in environment.external_services.iter() {
                let build = external_service.to_build();

                match external_service.to_application(
                    self.engine.context(),
                    &build.image,
                    self.engine.cloud_provider(),
                ) {
                    Some(x) => _applications.push(x),
                    None => {}
                }
            }

            let qe_environment = environment.to_qe_environment(
                self.engine.context(),
                &_applications,
                self.engine.cloud_provider(),
            );

            qe_environment
        };

        match environment_action {
            EnvironmentAction::EnvironmentWithFailover(
                target_environment,
                failover_environment,
            ) => {
                // let's reverse changes and rollback on the provided failover version
                let target_qe_environment = qe_environment(&target_environment);
                let failover_qe_environment = qe_environment(&failover_environment);

                let action = match failover_environment.action {
                    Action::Create => {
                        kubernetes.deploy_environment_error(&target_qe_environment);
                        kubernetes.deploy_environment(&failover_qe_environment)
                    }
                    Action::Pause => {
                        kubernetes.pause_environment_error(&target_qe_environment);
                        kubernetes.pause_environment(&failover_qe_environment)
                    }
                    Action::Delete => {
                        kubernetes.delete_environment_error(&target_qe_environment);
                        kubernetes.delete_environment(&failover_qe_environment)
                    }
                    Action::Nothing => Ok(()),
                };

                let _ = match action {
                    Ok(_) => {}
                    Err(err) => return Err(RollbackError::CommitError(err)),
                };

                Ok(())
            }
            EnvironmentAction::Environment(te) => {
                // revert changes but there is no failover environment
                let target_qe_environment = qe_environment(&te);

                let action = match te.action {
                    Action::Create => kubernetes.deploy_environment_error(&target_qe_environment),
                    Action::Pause => kubernetes.pause_environment_error(&target_qe_environment),
                    Action::Delete => kubernetes.delete_environment_error(&target_qe_environment),
                    Action::Nothing => Ok(()),
                };

                let _ = match action {
                    Ok(_) => {}
                    Err(err) => return Err(RollbackError::CommitError(err)),
                };

                Err(RollbackError::NoFailoverEnvironment)
            }
        }
    }

    pub fn commit(&mut self) -> TransactionResult {
        let mut applications_by_environment: HashMap<&Environment, Vec<Box<dyn Application>>> =
            HashMap::new();

        for step in self.steps.iter() {
            // execution loop
            self.executed_steps.push(step.clone());

            match step {
                Step::CreateKubernetes(kubernetes) => {
                    // create kubernetes
                    match kubernetes.on_create() {
                        Err(err) => {
                            warn!("ROLLBACK STARTED! an error occurred {:?}", err);
                            match self.rollback() {
                                Ok(_) => TransactionResult::Rollback(err),
                                Err(e) => {
                                    error!("ROLLBACK FAILED! fatal error: {:?}", e);
                                    return TransactionResult::UnrecoverableError(err, e);
                                }
                            }
                        }
                        _ => TransactionResult::Ok,
                    };
                }
                Step::DeleteKubernetes(kubernetes) => {
                    // delete kubernetes
                    match kubernetes.on_delete() {
                        Err(err) => {
                            warn!("ROLLBACK STARTED! an error occurred {:?}", err);
                            match self.rollback() {
                                Ok(_) => TransactionResult::Rollback(err),
                                Err(e) => {
                                    error!("ROLLBACK FAILED! fatal error: {:?}", e);
                                    return TransactionResult::UnrecoverableError(err, e);
                                }
                            }
                        }
                        _ => TransactionResult::Ok,
                    };
                }
                Step::BuildEnvironment(environment_action, option) => {
                    // build applications
                    let target_environment = match environment_action {
                        EnvironmentAction::Environment(te) => te,
                        EnvironmentAction::EnvironmentWithFailover(te, _) => te,
                    };

                    // TODO check that the image is not existing into the Container Registry before building it.

                    let apps_result = match self._build_applications(target_environment, option) {
                        Ok(applications) => match self._push_applications(applications, option) {
                            Ok(results) => {
                                let applications =
                                    results.into_iter().map(|(app, _)| app).collect::<Vec<_>>();

                                Ok(applications)
                            }
                            Err(err) => Err(err),
                        },
                        Err(err) => Err(err),
                    };

                    if apps_result.is_err() {
                        let commit_error = apps_result.err().unwrap();
                        warn!("ROLLBACK STARTED! an error occurred {:?}", commit_error);

                        return match self.rollback() {
                            Ok(_) => TransactionResult::Rollback(commit_error),
                            Err(err) => {
                                error!("ROLLBACK FAILED! fatal error: {:?}", err);
                                return TransactionResult::UnrecoverableError(commit_error, err);
                            }
                        };
                    }

                    let applications = apps_result.ok().unwrap();
                    applications_by_environment.insert(target_environment, applications);

                    // build as well the failover environment, retention could remove the application image
                    match environment_action {
                        EnvironmentAction::EnvironmentWithFailover(_, fe) => {
                            let apps_result = match self._build_applications(fe, option) {
                                Ok(applications) => {
                                    match self._push_applications(applications, option) {
                                        Ok(results) => {
                                            let applications = results
                                                .into_iter()
                                                .map(|(app, _)| app)
                                                .collect::<Vec<_>>();

                                            Ok(applications)
                                        }
                                        Err(err) => Err(err),
                                    }
                                }
                                Err(err) => Err(err),
                            };
                            if apps_result.is_err() {
                                // should never be triggered because core always should ask for working failover environment
                                let commit_error = apps_result.err().unwrap();
                                error!(
                                    "An error occurred on failover application  {:?}",
                                    commit_error
                                );
                            }
                        }
                        _ => {}
                    };
                }
                Step::DeployEnvironment(kubernetes, environment_action) => {
                    // deploy complete environment
                    match self.commit_environment(
                        *kubernetes,
                        *environment_action,
                        &applications_by_environment,
                        |qe_env| kubernetes.deploy_environment(qe_env),
                    ) {
                        TransactionResult::Ok => {}
                        err => return err,
                    };
                }
                Step::PauseEnvironment(kubernetes, environment_action) => {
                    // pause complete environment
                    match self.commit_environment(
                        *kubernetes,
                        *environment_action,
                        &applications_by_environment,
                        |qe_env| kubernetes.pause_environment(qe_env),
                    ) {
                        TransactionResult::Ok => {}
                        err => return err,
                    };
                }
                Step::DeleteEnvironment(kubernetes, environment_action) => {
                    // delete complete environment
                    match self.commit_environment(
                        *kubernetes,
                        *environment_action,
                        &applications_by_environment,
                        |qe_env| kubernetes.delete_environment(qe_env),
                    ) {
                        TransactionResult::Ok => {}
                        err => return err,
                    };
                }
            };
        }

        TransactionResult::Ok
    }

    fn commit_environment<F>(
        &self,
        kubernetes: &dyn Kubernetes,
        environment_action: &EnvironmentAction,
        applications_by_environment: &HashMap<&Environment, Vec<Box<dyn Application>>>,
        action_fn: F,
    ) -> TransactionResult
    where
        F: Fn(&crate::cloud_provider::environment::Environment) -> Result<(), EngineError>,
    {
        let target_environment = match environment_action {
            EnvironmentAction::Environment(te) => te,
            EnvironmentAction::EnvironmentWithFailover(te, _) => te,
        };

        let empty_vec = Vec::with_capacity(0);
        let built_applications = match applications_by_environment.get(target_environment) {
            Some(applications) => applications,
            None => &empty_vec,
        };

        let qe_environment = target_environment.to_qe_environment(
            self.engine.context(),
            built_applications,
            kubernetes.cloud_provider(),
        );

        let _ = match self.check_environment(&qe_environment) {
            TransactionResult::Ok => {}
            err => return err, // which it means that an error occurred
        };

        let execution_id = self.engine.context().execution_id();

        // inner function - I use it instead of closure because of ?Sized
        fn get_final_progress_info<T>(service: &Box<T>, execution_id: &str) -> ProgressInfo
        where
            T: Service + ?Sized,
        {
            ProgressInfo::new(
                service.progress_scope(),
                ProgressLevel::Info,
                None::<&str>,
                execution_id,
            )
        };

        // send the back the right progress status
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
            let progress_info = get_final_progress_info(service, execution_id);

            if !is_error {
                match action {
                    Action::Create => lh.started(progress_info),
                    Action::Pause => lh.paused(progress_info),
                    Action::Delete => lh.deleted(progress_info),
                    Action::Nothing => {} // nothing to do here?
                };
                return;
            }

            match action {
                Action::Create => lh.start_error(progress_info),
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
                        kubernetes,
                        &target_environment.action,
                        service,
                        execution_id,
                        true,
                    );
                }

                for service in &qe_environment.stateless_services {
                    send_progress(
                        kubernetes,
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
                        kubernetes,
                        &target_environment.action,
                        service,
                        execution_id,
                        false,
                    );
                }

                for service in &qe_environment.stateless_services {
                    send_progress(
                        kubernetes,
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

enum Step<'a> {
    // init and create all the necessary resources (Network, Kubernetes)
    CreateKubernetes(&'a dyn Kubernetes),
    DeleteKubernetes(&'a dyn Kubernetes),
    BuildEnvironment(&'a EnvironmentAction, DeploymentOption),
    DeployEnvironment(&'a dyn Kubernetes, &'a EnvironmentAction),
    PauseEnvironment(&'a dyn Kubernetes, &'a EnvironmentAction),
    DeleteEnvironment(&'a dyn Kubernetes, &'a EnvironmentAction),
}

impl<'a> Clone for Step<'a> {
    fn clone(&self) -> Self {
        match self {
            Step::CreateKubernetes(k) => Step::CreateKubernetes(*k),
            Step::DeleteKubernetes(k) => Step::DeleteKubernetes(*k),
            Step::BuildEnvironment(e, option) => Step::BuildEnvironment(*e, option.clone()),
            Step::DeployEnvironment(k, e) => Step::DeployEnvironment(*k, *e),
            Step::PauseEnvironment(k, e) => Step::PauseEnvironment(*k, *e),
            Step::DeleteEnvironment(k, e) => Step::DeleteEnvironment(*k, *e),
        }
    }
}

#[derive(Debug)]
pub enum RollbackError {
    CommitError(EngineError),
    NoFailoverEnvironment,
    Nothing,
}

pub enum TransactionResult {
    Ok,
    Rollback(EngineError),
    UnrecoverableError(EngineError, RollbackError),
}
