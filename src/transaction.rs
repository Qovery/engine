use std::borrow::{Borrow, BorrowMut};
use std::cell::RefCell;

use crate::build_platform::{Build, BuildError, GitRepository, Image};
use crate::cloud_provider::error::KubernetesError;
use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::container_registry::{PushError, PushResult};
use crate::git::Credentials;
use crate::models::{Action, Application, Environment, EnvironmentError};
use crate::transaction::Step::CreateKubernetes;
use std::collections::HashMap;

pub struct Transaction<'a> {
    pub config: Config,
    steps: Vec<Step<'a>>,
    executed_steps: Vec<Step<'a>>,
    build_listeners: Vec<Box<dyn ProgressListener>>,
    deploy_listeners: Vec<Box<dyn ProgressListener>>,
}

impl<'a> Transaction<'a> {
    pub fn new(config: Config) -> Self {
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

    pub fn deploy(&mut self, environment: &'a Environment) -> Result<(), EnvironmentError> {
        match environment.is_valid() {
            Ok(_) => {
                self.steps.push(Step::Deploy(environment));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn deploy_with_image(
        &mut self,
        environment: &'a Environment,
        image: Image,
    ) -> Result<(), EnvironmentError> {
        match environment.is_valid() {
            Ok(_) => {
                self.steps.push(Step::DeployWithImage(environment, image));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn add_deploy_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.deploy_listeners.push(listener);
    }

    fn build_applications(&self, environment: &Environment) -> Result<Vec<Image>, BuildError> {
        let apps_to_build = environment
            .applications
            .iter()
            .filter(|app| app.action == Action::Create);

        let images: Vec<_> = apps_to_build
            .map(|app| {
                self.config.build_platform.build(Build {
                    git_repository: GitRepository {
                        url: app.git_url.clone(),
                        credentials: Some(Credentials {
                            login: app.git_credentials.login.clone(),
                            password: app.git_credentials.access_token.clone(),
                        }),
                        commit_id: Some(app.commit_id.clone()),
                    },
                    image: Image {
                        name: app.name.clone(),
                        tag: app.commit_id.clone(),
                        commit_id: app.commit_id.clone(),
                    },
                })
            })
            .filter(|r| r.is_ok())
            .map(|r| r.ok().unwrap().build.image)
            .collect();

        Ok(images)
    }

    fn push_images(&self, images: Vec<Image>) -> Result<Vec<PushResult>, PushError> {
        let push_results: Vec<PushResult> = images
            .into_iter()
            .map(|image| match self.config.container_registry.push(image) {
                Ok(x) => Ok(x),
                Err(err) => return Err(err), // stop on error
            })
            .map(|x| x.ok().unwrap())
            .collect();

        Ok(push_results)
    }

    pub fn rollback(&self) -> Result<(), RollbackError> {
        self.executed_steps.iter().for_each(|step| match step {
            Step::CreateKubernetes(kubernetes) => {
                // revert kubernetes creation
                kubernetes.on_create_error();
            }
            Step::DeleteKubernetes(kubernetes) => {
                // revert kubernetes deletion
                kubernetes.on_delete_error();
            }
            Step::Build(environment) => {
                // revert build applications
            }
            Step::Deploy(environment) => {
                // revert environment deployment
            }
            Step::DeployWithImage(environment, image) => {
                // revert environment deployment
            }
        });

        Ok(())
    }

    fn deploy_environment(environment: &Environment, image: &Image) {}

    pub fn commit(&mut self) -> TransactionResult {
        let mut image_by_environment: HashMap<&'a Environment, Vec<Image>> = HashMap::new();

        for step in self.steps.iter() {
            // execution loop
            self.executed_steps.push(step.clone());

            match step {
                Step::CreateKubernetes(kubernetes) => {
                    // create kubernetes
                    kubernetes.on_create();
                    // TODO check success or rollback
                }
                Step::DeleteKubernetes(kubernetes) => {
                    // delete kubernetes
                    kubernetes.on_delete();
                    // TODO check success or rollback
                }
                Step::Build(environment) => {
                    // build applications
                    let push_results = match self.build_applications(environment) {
                        Ok(images) => match self.push_images(images) {
                            Ok(r) => Ok(r),
                            Err(err) => Err(CommitError::Deploy(err)),
                        },
                        Err(err) => Err(CommitError::Build(err)),
                    };

                    if push_results.is_err() {
                        let commit_error = push_results.err().unwrap();

                        return match self.rollback() {
                            Ok(_) => TransactionResult::Rollback(commit_error),
                            Err(err) => TransactionResult::UnrecoverableError(commit_error, err),
                        };
                    }

                    let images: Vec<_> = push_results
                        .ok()
                        .unwrap()
                        .into_iter()
                        .map(|x| x.image)
                        .collect();

                    image_by_environment.insert(environment, images);
                }
                Step::Deploy(environment) => {
                    // deploy environment
                    // TODO check success or rollback
                }
                Step::DeployWithImage(environment, image) => {
                    // deploy environment
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
    Deploy(&'a Environment),
    DeployWithImage(&'a Environment, Image),
}

impl<'a> Clone for Step<'a> {
    fn clone(&self) -> Self {
        match self {
            Step::CreateKubernetes(x) => Step::CreateKubernetes(*x),
            Step::DeleteKubernetes(x) => Step::DeleteKubernetes(*x),
            Step::Build(x) => Step::Build(*x),
            Step::Deploy(x) => Step::Deploy(*x),
            Step::DeployWithImage(x, i) => Step::DeployWithImage(x, i.clone()),
        }
    }
}

pub enum CommitError {
    CreateKubernetes(KubernetesError),
    DeleteKubernetes(KubernetesError),
    Build(BuildError),
    Deploy(PushError),
}

pub enum RollbackError {}

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
