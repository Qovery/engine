use crate::build_platform::{
    Build, BuildError, BuildOptions, EnvironmentVariable, GitRepository, Image,
};
use crate::cloud_provider::application::Application;
use crate::cloud_provider::error::{DeployError, KubernetesError, ServiceError};
use crate::cloud_provider::{Kubernetes, Service};
use crate::config::Config;
use crate::container_registry::{PushError, PushResult};
use crate::git::Credentials;
use crate::models::{Action, Environment, EnvironmentError};
use std::borrow::Borrow;
use std::collections::HashMap;

pub struct Transaction<'a> {
    pub config: Config<'a>,
    steps: Vec<Step<'a>>,
    executed_steps: Vec<Step<'a>>,
    build_listeners: Vec<Box<dyn ProgressListener>>,
    deploy_listeners: Vec<Box<dyn ProgressListener>>,
}

impl<'a> Transaction<'a> {
    pub fn new(config: Config<'a>) -> Self {
        Transaction::<'a> {
            config,
            steps: vec![],
            executed_steps: vec![],
            build_listeners: vec![],
            deploy_listeners: vec![],
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

    pub fn build(&mut self, environment: &'a Environment) -> Result<(), EnvironmentError> {
        match environment.is_valid() {
            Ok(_) => {
                self.steps.push(Step::Build(environment));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn add_build_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.build_listeners.push(listener);
    }

    pub fn deploy(
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

    pub fn deploy_service(
        &mut self,
        kubernetes: &'a dyn Kubernetes,
        service: &'a dyn Service,
    ) -> Result<(), ServiceError> {
        match service.is_valid() {
            Ok(_) => {
                self.steps.push(Step::DeployService(kubernetes, service));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn add_deploy_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.deploy_listeners.push(listener);
    }

    fn _build_applications(
        &self,
        environment: &Environment,
    ) -> Result<Vec<Application>, BuildError> {
        let apps_to_build = environment
            .applications
            .iter()
            .filter(|app| app.action == Action::Create); // TODO configurable?

        let applications: Vec<_> = apps_to_build
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
            .map(|(a, r)| Application {
                id: a.id.clone(),
                name: a.name.clone(),
                image: r.ok().unwrap().build.image,
            })
            .collect();

        Ok(applications)
    }

    fn _push_applications(
        &self,
        applications: &Vec<Application>,
    ) -> Result<Vec<PushResult>, PushError> {
        let push_results: Vec<PushResult> = applications
            .iter()
            .map(
                |app| match self.config.container_registry.push(app.image.clone()) {
                    Ok(x) => Ok(x),
                    Err(err) => return Err(err), // stop on error
                },
            )
            .map(|x| x.ok().unwrap())
            .collect();

        Ok(push_results)
    }

    fn _deploy_service(
        &self,
        kubernetes: &'a dyn Kubernetes,
        service: &'a dyn Service,
    ) -> Result<(), DeployError> {
        // TODO

        match kubernetes.create_service(service) {
            Ok(_) => {}
            Err(err) => return Err(DeployError::Error),
        }

        Ok(())
    }

    fn _deploy_service_with_transaction_error(
        &self,
        kubernetes: &'a dyn Kubernetes,
        service: &'a dyn Service,
    ) -> TransactionResult {
        match self._deploy_service(kubernetes, service) {
            Err(err) => match self.rollback() {
                Ok(_) => TransactionResult::Rollback(CommitError::Deploy(err)),
                Err(e) => TransactionResult::UnrecoverableError(CommitError::Deploy(err), e),
            },
            _ => TransactionResult::Ok,
        }
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
                Step::Build(environment) => {
                    // revert build applications
                }
                Step::DeployService(kubernetes, service) => {
                    // TODO push the last version? and then delete if there is no valid version?
                    match kubernetes.delete_service(*service) {
                        Err(err) => return Err(RollbackError::Error),
                        _ => {}
                    };
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
        let mut applications_by_environment: HashMap<&Environment, Vec<Application>> =
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
                Step::Build(environment) => {
                    // build applications
                    let apps_result = match self._build_applications(environment) {
                        Ok(applications) => match self._push_applications(&applications) {
                            Ok(_) => Ok(applications),
                            Err(err) => Err(CommitError::Push(err)),
                        },
                        Err(err) => Err(CommitError::Build(err)),
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
                Step::DeployService(kubernetes, service) => {
                    // deploy environment
                    match self._deploy_service_with_transaction_error(*kubernetes, *service) {
                        TransactionResult::Ok => {}
                        err => return err,
                    }
                }
                Step::DeployEnvironment(kubernetes, environment) => {
                    // deploy complete environment

                    // deploy databases
                    let databases_results = environment
                        .databases
                        .iter()
                        .map(|db| db.to_service(self.config.cloud_provider.borrow()))
                        .filter(|s| s.is_some()) // TODO raise an error if service = none?
                        .map(|s| s.unwrap())
                        .map(|service| {
                            self._deploy_service_with_transaction_error(
                                *kubernetes,
                                service.borrow(),
                            )
                        })
                        .collect::<Vec<_>>();

                    for t in databases_results.into_iter() {
                        match t {
                            TransactionResult::Ok => {}
                            err => return err,
                        }
                    }

                    // deploy applications
                    let transaction_results = match applications_by_environment.remove(environment)
                    {
                        Some(apps) => apps
                            .iter()
                            .map(|app| {
                                self._deploy_service_with_transaction_error(*kubernetes, app)
                            })
                            .collect::<Vec<_>>(),
                        None => vec![], // TODO return an error?
                    };

                    for t in transaction_results.into_iter() {
                        match t {
                            TransactionResult::Ok => {}
                            err => return err,
                        }
                    }
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
    Build(&'a Environment),
    DeployService(&'a dyn Kubernetes, &'a dyn Service),
    DeployEnvironment(&'a dyn Kubernetes, &'a Environment),
}

impl<'a> Clone for Step<'a> {
    fn clone(&self) -> Self {
        match self {
            Step::CreateKubernetes(k) => Step::CreateKubernetes(*k),
            Step::DeleteKubernetes(k) => Step::DeleteKubernetes(*k),
            Step::Build(e) => Step::Build(*e),
            Step::DeployService(k, s) => Step::DeployService(*k, *s),
            Step::DeployEnvironment(k, e) => Step::DeployEnvironment(*k, *e),
        }
    }
}

pub enum CommitError {
    CreateKubernetes(KubernetesError),
    DeleteKubernetes(KubernetesError),
    Build(BuildError),
    Push(PushError),
    Deploy(DeployError),
}

pub enum RollbackError {
    Error,
}

pub enum TransactionResult {
    Ok,
    Rollback(CommitError),
    UnrecoverableError(CommitError, RollbackError),
}

pub struct ProgressInfo {
    percent: u8,
    message: String,
}

pub trait ProgressListener {
    fn on_progress(&self, info: &ProgressInfo);
    fn on_complete(&self, info: &ProgressInfo);
    fn on_error(&self, info: &ProgressInfo);
}
