use crate::build_platform::{Build, GitRepository, Image};
use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::git::Credentials;
use crate::models::{Action, Application, Environment};
use std::borrow::BorrowMut;
use std::cell::RefCell;

pub struct Transaction<'a> {
    pub config: Config<'a>,
    steps: Vec<Step>,
    build_listeners: Vec<Box<dyn ProgressListener>>,
    deploy_listeners: Vec<Box<dyn ProgressListener>>,
}

impl<'a> Transaction<'a> {
    pub fn new(config: Config<'a>) -> Self {
        Transaction::<'a> {
            config,
            steps: vec![],
            build_listeners: vec![],
            deploy_listeners: vec![],
        }
    }

    pub fn build(&mut self) {
        self.steps.push(Step::Build);
    }

    pub fn add_build_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.build_listeners.push(listener);
    }

    pub fn deploy(&mut self) {
        self.steps.push(Step::Deploy);
    }

    pub fn add_deploy_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.deploy_listeners.push(listener);
    }

    pub fn commit(&self) {
        // TODO check cloud_provider and Kubernetes is initialized
        // TODO init cloud_provider and Kubernetes otherwise

        // build applications
        self.steps.iter().for_each(|step| match step {
            Step::Build => {
                println!("build");

                &self
                    .config
                    .environment
                    .applications
                    .iter()
                    .filter(|app| app.action == Action::Create)
                    .for_each(|app| {
                        &self.config.build_platform.build(Build {
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
                        });
                    });
            }
            Step::Deploy => {
                println!("deploy");
            }
        })
    }
}

enum Step {
    Build,
    Deploy,
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
