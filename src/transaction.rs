use std::borrow::{Borrow, BorrowMut};
use std::cell::RefCell;

use crate::build_platform::{Build, GitRepository, Image};
use crate::cloud_provider::error::KubernetesError;
use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::container_registry::{PushError, PushResult};
use crate::git::Credentials;
use crate::models::{Action, Application, Environment, EnvironmentError};

pub struct Transaction<'a> {
    pub config: Config,
    steps: Vec<Step<'a>>,
    build_listeners: Vec<Box<dyn ProgressListener>>,
    deploy_listeners: Vec<Box<dyn ProgressListener>>,
}

impl<'a> Transaction<'a> {
    pub fn new(config: Config) -> Self {
        Transaction::<'a> {
            config,
            steps: vec![],
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

    pub fn add_deploy_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.deploy_listeners.push(listener);
    }

    fn build_and_get_images_to_deploy(&self, environment: &Environment) -> Vec<Image> {
        let apps_to_build = environment
            .applications
            .iter()
            .filter(|app| app.action == Action::Create);

        let images_built: Vec<_> = apps_to_build
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

        images_built
            .iter()
            .for_each(|image| match self.config.container_registry.push(image) {
                Ok(_) => {}
                Err(err) => match err {
                    PushError::CredentialsError => panic!("registry: credentials errors"),
                    PushError::ImageAlreadyExists => panic!("registry: image already exists"),
                    PushError::ImagePushFailed => panic!("registry: image push failed"),
                    PushError::ImageTagFailed => panic!("registry: image tag failed"),
                },
            });

        images_built
    }

    pub fn commit(&self) {
        // TODO check cloud_provider and Kubernetes is initialized
        // TODO init cloud_provider and Kubernetes otherwise

        self.steps.iter().for_each(|step| match step {
            Step::Build(environment) => {
                // build applications
                self.build_and_get_images_to_deploy(environment);
                // TODO check success or fallback
            }
            Step::Deploy(environment) => {
                // deploy environment
                // TODO check success or fallback
            }
            Step::CreateKubernetes(kubernetes) => {
                // create kubernetes
                kubernetes.on_create();
                // TODO check success or fallback
            }
            Step::DeleteKubernetes(kubernetes) => {
                // delete kubernetes
                kubernetes.on_delete();
                // TODO check success or fallback
            }
        })
    }
}

enum Step<'a> {
    // init and create all the necessary resources (Network, Kubernetes)
    CreateKubernetes(&'a dyn Kubernetes),
    DeleteKubernetes(&'a dyn Kubernetes),
    Build(&'a Environment),
    Deploy(&'a Environment),
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
