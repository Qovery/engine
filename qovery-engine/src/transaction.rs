use std::borrow::Borrow;
use std::collections::HashMap;

use crate::build_platform::{
    Build, BuildError, BuildOptions, EnvironmentVariable, GitRepository, Image,
};
use crate::cloud_provider::kubernetes::{Kubernetes, KubernetesError};
use crate::cloud_provider::service::Application;
use crate::cloud_provider::service::{Service, ServiceError, StatefulService, StatelessService};
use crate::cloud_provider::DeployError;
use crate::config::Config;
use crate::container_registry::{PushError, PushResult};
use crate::git::Credentials;
use crate::models::{Action, Environment, EnvironmentError};
use crate::transaction::CommitError::NotValidService;
use itertools::Itertools;

pub struct Transaction<'a> {
    pub config: Config<'a>,
    steps: Vec<Step<'a>>,
    executed_steps: Vec<Step<'a>>,
}

impl<'a> Transaction<'a> {
    pub fn new(config: Config<'a>) -> Self {
        Transaction::<'a> {
            config,
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

    pub fn build_environment(
        &mut self,
        environment: &'a Environment,
    ) -> Result<(), EnvironmentError> {
        match environment.is_valid() {
            Ok(_) => {
                self.steps.push(Step::BuildEnvironment(environment));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn deploy_environment(
        &mut self,
        kubernetes: &'a dyn Kubernetes,
        environment: &'a Environment,
    ) -> Result<(), EnvironmentError> {
        match environment.is_valid() {
            Ok(_) => {
                self.steps
                    .push(Step::DeployEnvironment(kubernetes, environment));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn _build_applications(
        &self,
        environment: &Environment,
    ) -> Result<Vec<Box<dyn Application>>, BuildError> {
        let apps_to_build = environment
            .applications
            .iter()
            .filter(|app| app.action == Action::Create); // TODO configurable?

        let applications: Vec<Box<dyn Application>> = apps_to_build
            .map(|app| {
                let result = self.config.build_platform.build(Build {
                    git_repository: GitRepository {
                        url: app.git_url.clone(),
                        credentials: Some(Credentials {
                            login: app.git_credentials.login.clone(),
                            password: app.git_credentials.access_token.clone(),
                        }),
                        commit_id: Some(app.commit_id.clone()),
                        dockerfile_path: ".".to_string(),
                    },
                    image: Image {
                        name: app.name.clone(),
                        tag: app.commit_id.clone(),
                        commit_id: app.commit_id.clone(),
                    },
                    options: BuildOptions {
                        environment_variables: app
                            .environment_variables
                            .iter()
                            .map(|ev| EnvironmentVariable {
                                key: ev.key.clone(),
                                value: ev.value.clone(),
                            })
                            .collect::<Vec<_>>(),
                    },
                });

                (app, result)
            })
            .filter(|(_, r)| r.is_ok())
            .map(|(a, r)| {
                a.to_application(
                    environment.execution_id.as_str(),
                    &r.ok().unwrap().build.image,
                    self.config.cloud_provider,
                )
            })
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect();

        Ok(applications)
    }

    fn _push_applications(
        &self,
        applications: &Vec<Box<dyn Application>>,
    ) -> Result<Vec<PushResult>, PushError> {
        let results: Vec<_> = applications
            .iter()
            .map(|app| self.config.container_registry.push(app.image().clone()))
            .collect();

        let mut push_results: Vec<PushResult> = vec![];
        for r in results.into_iter() {
            match r {
                Ok(push_result) => push_results.push(push_result),
                Err(err) => return Err(err), // stop on error
            }
        }

        Ok(push_results)
    }

    pub fn rollback(&self) -> Result<(), RollbackError> {
        for step in self.executed_steps.iter() {
            match step {
                Step::CreateKubernetes(kubernetes) => {
                    // revert kubernetes creation
                    match kubernetes.on_create_error() {
                        Err(err) => return Err(RollbackError::Error),
                        _ => {}
                    };
                }
                Step::DeleteKubernetes(kubernetes) => {
                    // revert kubernetes deletion
                    match kubernetes.on_delete_error() {
                        Err(err) => return Err(RollbackError::Error),
                        _ => {}
                    };
                }
                Step::BuildEnvironment(environment) => {
                    // revert build applications
                }
                Step::DeployEnvironment(kubernetes, environment) => {
                    // revert environment deployment
                    // TODO revert applications and services with the last version,
                    // TODO if there is no valid state then delete the applications?
                }
            }
        }

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
                        Err(err) => match self.rollback() {
                            Ok(_) => {
                                TransactionResult::Rollback(CommitError::CreateKubernetes(err))
                            }
                            Err(e) => TransactionResult::UnrecoverableError(
                                CommitError::CreateKubernetes(err),
                                e,
                            ),
                        },
                        _ => TransactionResult::Ok,
                    };
                }
                Step::DeleteKubernetes(kubernetes) => {
                    // delete kubernetes
                    match kubernetes.on_delete() {
                        Err(err) => match self.rollback() {
                            Ok(_) => {
                                TransactionResult::Rollback(CommitError::DeleteKubernetes(err))
                            }
                            Err(e) => TransactionResult::UnrecoverableError(
                                CommitError::DeleteKubernetes(err),
                                e,
                            ),
                        },
                        _ => TransactionResult::Ok,
                    };
                }
                Step::BuildEnvironment(environment) => {
                    // build applications
                    let apps_result = match self._build_applications(environment) {
                        Ok(applications) => match self._push_applications(&applications) {
                            Ok(_) => Ok(applications),
                            Err(err) => Err(CommitError::PushImage(err)),
                        },
                        Err(err) => Err(CommitError::BuildImage(err)),
                    };

                    if apps_result.is_err() {
                        let commit_error = apps_result.err().unwrap();

                        return match self.rollback() {
                            Ok(_) => TransactionResult::Rollback(commit_error),
                            Err(err) => TransactionResult::UnrecoverableError(commit_error, err),
                        };
                    }

                    let applications = apps_result.ok().unwrap();
                    applications_by_environment.insert(environment, applications);
                }
                Step::DeployEnvironment(kubernetes, environment) => {
                    // deploy complete environment
                    let built_applications = applications_by_environment.get(environment).unwrap(); // FIXME unsafe?

                    let qe_environment = environment
                        .to_qe_environment(built_applications, kubernetes.cloud_provider());

                    for service in qe_environment.stateful_services.iter() {
                        match service.is_valid() {
                            Err(service_error) => {
                                return match self.rollback() {
                                    Ok(_) => TransactionResult::Rollback(
                                        CommitError::NotValidService(service_error),
                                    ),
                                    Err(err) => TransactionResult::UnrecoverableError(
                                        CommitError::NotValidService(service_error),
                                        err,
                                    ),
                                }
                            }
                            _ => {}
                        };
                    }

                    let _ = match kubernetes.deploy_environment(&qe_environment) {
                        Ok(_) => {}
                        Err(err) => {
                            return TransactionResult::Rollback(CommitError::DeployEnvironment(
                                err,
                            ));
                        }
                    };
                }
            };
        }

        TransactionResult::Ok
    }
}

enum Step<'a> {
    // init and create all the necessary resources (Network, Kubernetes)
    CreateKubernetes(&'a dyn Kubernetes),
    DeleteKubernetes(&'a dyn Kubernetes),
    BuildEnvironment(&'a Environment),
    DeployEnvironment(&'a dyn Kubernetes, &'a Environment),
}

impl<'a> Clone for Step<'a> {
    fn clone(&self) -> Self {
        match self {
            Step::CreateKubernetes(k) => Step::CreateKubernetes(*k),
            Step::DeleteKubernetes(k) => Step::DeleteKubernetes(*k),
            Step::BuildEnvironment(e) => Step::BuildEnvironment(*e),
            Step::DeployEnvironment(k, e) => Step::DeployEnvironment(*k, *e),
        }
    }
}

pub enum CommitError {
    CreateKubernetes(KubernetesError),
    DeleteKubernetes(KubernetesError),
    DeployEnvironment(KubernetesError),
    NotValidService(ServiceError),
    BuildImage(BuildError),
    PushImage(PushError),
    DeployImage(DeployError),
}

pub enum RollbackError {
    Error,
}

pub enum TransactionResult {
    Ok,
    Rollback(CommitError),
    UnrecoverableError(CommitError, RollbackError),
}
