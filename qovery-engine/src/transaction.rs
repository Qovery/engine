use std::borrow::{Borrow, BorrowMut};
use std::collections::HashMap;

use itertools::Itertools;

use crate::build_platform::{
    Build, BuildError, BuildOptions, EnvironmentVariable, GitRepository, Image,
};
use crate::cloud_provider::kubernetes::{Kubernetes, KubernetesError};
use crate::cloud_provider::service::Application;
use crate::cloud_provider::service::{Service, ServiceError, StatefulService, StatelessService};
use crate::cloud_provider::DeployError;
use crate::container_registry::{PushError, PushResult};
use crate::engine::Engine;
use crate::git::Credentials;
use crate::models::{Action, Environment, EnvironmentAction, EnvironmentError};
use crate::transaction::CommitError::NotValidService;
use serde::{Deserialize, Serialize};

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

    pub fn create_kubernetes(
        &mut self,
        kubernetes: &'a dyn Kubernetes,
    ) -> Result<(), KubernetesError> {
        match kubernetes.is_valid() {
            Ok(_) => {
                self.steps.push(Step::CreateKubernetes(kubernetes));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn delete_kubernetes(
        &mut self,
        kubernetes: &'a dyn Kubernetes,
    ) -> Result<(), KubernetesError> {
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
    ) -> Result<Vec<Box<dyn Application>>, BuildError> {
        let apps_to_build = environment
            .applications
            .iter()
            // build only applications that are set with Action: Create
            .filter(|app| app.action == Action::Create);

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
    ) -> Result<Vec<(Box<dyn Application>, PushResult)>, PushError> {
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
                Err(err) => return Err(err), // stop on error // TODO add error! log message here
            }
        }

        Ok(results)
    }

    fn check_environment(
        &self,
        environment: &crate::cloud_provider::environment::Environment,
    ) -> TransactionResult {
        match environment.is_valid() {
            Err(service_error) => {
                warn!("ROLLBACK STARTED! an error occurred {:?}", service_error);
                return match self.rollback() {
                    Ok(_) => {
                        TransactionResult::Rollback(CommitError::NotValidService(service_error))
                    }
                    Err(err) => {
                        error!("ROLLBACK FAILED! fatal error: {:?}", err);
                        TransactionResult::UnrecoverableError(
                            CommitError::NotValidService(service_error),
                            err,
                        )
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
                        Err(err) => return Err(RollbackError::CreateKubernetes(err)),
                        _ => {}
                    };
                }
                Step::DeleteKubernetes(kubernetes) => {
                    // revert kubernetes deletion
                    match kubernetes.on_delete_error() {
                        Err(err) => return Err(RollbackError::DeleteKubernetes(err)),
                        _ => {}
                    };
                }
                Step::BuildEnvironment(environment_action, option) => {
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

    /// This function is a wrapper to correctly revert all changes of a attempt deployment AND
    /// if a failover environment is provided, then rollback.
    fn rollback_environment(
        &self,
        kubernetes: &dyn Kubernetes,
        environment_action: &EnvironmentAction,
    ) -> Result<(), RollbackError> {
        let qe_environment = |environment: &Environment| {
            let mut _applications = Vec::with_capacity(environment.applications.len());

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

            let qe_environment = environment.to_qe_environment(
                self.engine.context(),
                &_applications,
                self.engine.cloud_provider(),
            );

            qe_environment
        };

        let (target_environment, failover_environment) = match environment_action {
            EnvironmentAction::EnvironmentWithFailover(te, fe) => (te, fe),
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
                    Err(err) => {
                        return Err(match te.action {
                            Action::Create => RollbackError::DeployEnvironment(err),
                            Action::Pause => RollbackError::PauseEnvironment(err),
                            Action::Delete => RollbackError::DeleteEnvironment(err),
                            Action::Nothing => RollbackError::Error, // it can't happens
                        });
                    }
                };

                return Err(RollbackError::NoFailoverEnvironment);
            }
        };

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
            Err(err) => {
                return Err(match failover_environment.action {
                    Action::Create => RollbackError::DeployEnvironment(err),
                    Action::Pause => RollbackError::PauseEnvironment(err),
                    Action::Delete => RollbackError::DeleteEnvironment(err),
                    Action::Nothing => RollbackError::Error, // it can't happens
                });
            }
        };

        Ok(())
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
                                Ok(_) => {
                                    TransactionResult::Rollback(CommitError::CreateKubernetes(err))
                                }
                                Err(e) => {
                                    error!("ROLLBACK FAILED! fatal error: {:?}", e);
                                    TransactionResult::UnrecoverableError(
                                        CommitError::CreateKubernetes(err),
                                        e,
                                    )
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
                                Ok(_) => {
                                    TransactionResult::Rollback(CommitError::DeleteKubernetes(err))
                                }
                                Err(e) => {
                                    error!("ROLLBACK FAILED! fatal error: {:?}", e);
                                    TransactionResult::UnrecoverableError(
                                        CommitError::DeleteKubernetes(err),
                                        e,
                                    )
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

                    let apps_result = match self._build_applications(target_environment, option) {
                        Ok(applications) => match self._push_applications(applications, option) {
                            Ok(results) => {
                                let applications =
                                    results.into_iter().map(|(app, _)| app).collect::<Vec<_>>();

                                Ok(applications)
                            }
                            Err(err) => Err(CommitError::PushImage(err)),
                        },
                        Err(err) => Err(CommitError::BuildImage(err)),
                    };

                    if apps_result.is_err() {
                        let commit_error = apps_result.err().unwrap();
                        warn!("ROLLBACK STARTED! an error occurred {:?}", commit_error);

                        return match self.rollback() {
                            Ok(_) => TransactionResult::Rollback(commit_error),
                            Err(err) => {
                                error!("ROLLBACK FAILED! fatal error: {:?}", err);
                                TransactionResult::UnrecoverableError(commit_error, err)
                            }
                        };
                    }

                    let applications = apps_result.ok().unwrap();
                    applications_by_environment.insert(target_environment, applications);
                }
                Step::DeployEnvironment(kubernetes, environment_action) => {
                    // deploy complete environment
                    match self.commit_environment(
                        *kubernetes,
                        *environment_action,
                        &applications_by_environment,
                        |qe_env| kubernetes.deploy_environment(qe_env),
                        |err| CommitError::DeployEnvironment(err),
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
                        |err| CommitError::PauseEnvironment(err),
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
                        |err| CommitError::DeleteEnvironment(err),
                    ) {
                        TransactionResult::Ok => {}
                        err => return err,
                    };
                }
            };
        }

        TransactionResult::Ok
    }

    fn commit_environment<F, E>(
        &self,
        kubernetes: &dyn Kubernetes,
        environment_action: &EnvironmentAction,
        applications_by_environment: &HashMap<&Environment, Vec<Box<dyn Application>>>,
        action_fn: F,
        commit_error: E,
    ) -> TransactionResult
    where
        F: Fn(&crate::cloud_provider::environment::Environment) -> Result<(), KubernetesError>,
        E: Fn(KubernetesError) -> CommitError,
    {
        let target_environment = match environment_action {
            EnvironmentAction::Environment(te) => te,
            EnvironmentAction::EnvironmentWithFailover(te, _) => te,
        };

        let built_applications = applications_by_environment.get(target_environment).unwrap(); // FIXME unsafe?

        let qe_environment = target_environment.to_qe_environment(
            self.engine.context(),
            built_applications,
            kubernetes.cloud_provider(),
        );

        let _ = match self.check_environment(&qe_environment) {
            TransactionResult::Ok => {}
            err => return err, // which it means that an error occurred
        };

        let _ = match action_fn(&qe_environment) {
            Err(err) => {
                return match self.rollback() {
                    Ok(_) => TransactionResult::Rollback(commit_error(err)),
                    Err(rollback_err) => {
                        error!("ROLLBACK FAILED! fatal error: {:?}", rollback_err);
                        TransactionResult::UnrecoverableError(commit_error(err), rollback_err)
                    }
                }
            }
            _ => {}
        };

        TransactionResult::Ok
    }
}

#[derive(Clone)]
pub struct DeploymentOption {
    force_build: bool,
    force_push: bool,
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
pub enum CommitError {
    CreateKubernetes(KubernetesError),
    DeleteKubernetes(KubernetesError),
    DeployEnvironment(KubernetesError),
    PauseEnvironment(KubernetesError),
    DeleteEnvironment(KubernetesError),
    NotValidService(ServiceError),
    BuildImage(BuildError),
    PushImage(PushError),
    DeployImage(DeployError),
}

#[derive(Debug)]
pub enum RollbackError {
    CreateKubernetes(KubernetesError),
    DeleteKubernetes(KubernetesError),
    DeployEnvironment(KubernetesError),
    PauseEnvironment(KubernetesError),
    DeleteEnvironment(KubernetesError),
    NotValidService(ServiceError),
    BuildImage(BuildError),
    PushImage(PushError),
    DeployImage(DeployError),
    NoFailoverEnvironment,
    Error,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ActionContext {
    pub kind: Kind,
    pub id: String,
    pub execution_id: String
}

impl ActionContext {
    pub fn new(kind: Kind, id: String, execution_id: String) -> Self { ActionContext { kind, id, execution_id } }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Service, Application, Router, Environment, Execution
}

pub enum TransactionResult {
    Ok,
    Rollback(CommitError),
    UnrecoverableError(CommitError, RollbackError),
}
