use std::borrow::Borrow;

use crossbeam_channel::Sender;

use qovery_engine::cloud_provider::CloudProviderError;
use qovery_engine::config::Config;
use qovery_engine::error::ConfigurationError;
use qovery_engine::transaction::TransactionResult;

use crate::models::{Action, Request};
use crate::task_manager::{InternalTask, Message, Status, Task};

#[derive(Clone)]
pub struct InfrastructureTask {
    request: Request,
}

impl InfrastructureTask {
    pub fn new(request: Request) -> Self {
        InfrastructureTask { request }
    }
}

impl Task for InfrastructureTask {
    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn update_status(&self, sender: &Sender<Message>, status: Status) {
        let it = InternalTask {
            task: Box::new(self.clone()),
            status,
        };
        let _ = sender.send(Ok(it));
    }

    fn run(&self, sender: Sender<Message>) {
        info!("infrastructure task {} started", self.id());
        self.update_status(&sender, Status::Running { message: None });

        let build_platform = self.request.build_platform.as_engine_build_platform();
        let cloud_provider = self.request.cloud_provider.as_engine_cloud_provider();
        let container_registry = self
            .request
            .container_registry
            .as_engine_container_registry();

        let config = Config::new(
            build_platform.borrow(),
            container_registry.borrow(),
            cloud_provider.borrow(),
        );

        // FIXME - return errors with Sender
        let session = match config.session() {
            Ok(session) => Some(session),
            Err(err) => {
                // FIXME return error message
                self.update_status(&sender, Status::Failed { message: None });
                None
            }
        };

        if session.is_none() {
            self.update_status(&sender, Status::Failed { message: None });
            return;
        }

        let mut tx = session.unwrap().transaction();

        let nodes = self
            .request
            .cloud_provider
            .kubernetes
            .as_engine_kubernetes_nodes();

        let kubernetes = self
            .request
            .cloud_provider
            .kubernetes
            .as_engine_kubernetes(cloud_provider.borrow(), nodes.borrow());

        match self.request.action {
            Action::Create => tx.create_kubernetes(kubernetes.borrow()),
            Action::Delete => tx.delete_kubernetes(kubernetes.borrow()),
        };

        // TODO implement on_progress callback and send status update in real time

        match tx.commit() {
            TransactionResult::Ok => {
                self.update_status(&sender, Status::Done { message: None });
            }
            TransactionResult::Rollback(commit_err) => {
                // FIXME return error message
                self.update_status(&sender, Status::Failed { message: None });
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                // FIXME return error message
                self.update_status(&sender, Status::Failed { message: None });
            }
        }

        info!("infrastructure task {} finished", self.id());
    }
}

#[derive(Clone)]
pub struct EnvironmentTask {
    request: Request,
}

impl EnvironmentTask {
    pub fn new(request: Request) -> Self {
        EnvironmentTask { request }
    }
}

impl Task for EnvironmentTask {
    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn update_status(&self, sender: &Sender<Message>, status: Status) {
        let it = InternalTask {
            task: Box::new(self.clone()),
            status,
        };
        let _ = sender.send(Ok(it));
    }

    fn run(&self, sender: Sender<Message>) {
        info!("environment task {} started", self.id());
        self.update_status(&sender, Status::Running { message: None });

        let build_platform = self.request.build_platform.as_engine_build_platform();
        let cloud_provider = self.request.cloud_provider.as_engine_cloud_provider();
        let container_registry = self
            .request
            .container_registry
            .as_engine_container_registry();

        let config = Config::new(
            build_platform.borrow(),
            container_registry.borrow(),
            cloud_provider.borrow(),
        );

        // FIXME - return errors with Sender
        let session = match config.session() {
            Ok(session) => Some(session),
            Err(err) => {
                // FIXME return error message
                self.update_status(&sender, Status::Failed { message: None });
                None
            }
        };

        if session.is_none() {
            self.update_status(&sender, Status::Failed { message: None });
            return;
        }

        let mut tx = session.unwrap().transaction();

        let nodes = self
            .request
            .cloud_provider
            .kubernetes
            .as_engine_kubernetes_nodes();

        let kubernetes = self
            .request
            .cloud_provider
            .kubernetes
            .as_engine_kubernetes(cloud_provider.borrow(), nodes.borrow());

        let environment = self.request.environment.as_ref().unwrap();

        match self.request.action {
            Action::Create => tx.deploy(kubernetes.borrow(), environment),
            Action::Delete => unimplemented!(),
        };

        // TODO implement on_progress callback and send status update in real time

        match tx.commit() {
            TransactionResult::Ok => {
                self.update_status(&sender, Status::Done { message: None });
            }
            TransactionResult::Rollback(commit_err) => {
                // FIXME return error message
                self.update_status(&sender, Status::Failed { message: None });
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                // FIXME return error message
                self.update_status(&sender, Status::Failed { message: None });
            }
        }

        info!("environment task {} finished", self.id());
    }
}
