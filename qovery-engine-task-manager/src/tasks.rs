use std::borrow::Borrow;
use std::rc::Rc;

use crossbeam_channel::Sender;

use qovery_engine::cloud_provider::CloudProviderError;
use qovery_engine::config::Config;
use qovery_engine::error::ConfigurationError;
use qovery_engine::models::{ProgressInfo, ProgressListener};
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

        let my_progress_listener: Rc<Box<dyn ProgressListener>> =
            Rc::new(Box::new(MyProgressListener {
                task: self.clone(),
                sender: sender.clone(),
            }));

        let mut build_platform = self
            .request
            .build_platform
            .as_engine_build_platform(self.id());

        build_platform.add_listener(my_progress_listener.clone());

        let mut cloud_provider = self
            .request
            .cloud_provider
            .as_engine_cloud_provider(self.id(), self.request.organization_id.as_str());

        cloud_provider.add_listener(my_progress_listener.clone());

        let mut container_registry = self
            .request
            .container_registry
            .as_engine_container_registry(self.id());

        container_registry.add_listener(my_progress_listener.clone());

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

        let kubernetes = self.request.cloud_provider.kubernetes.as_engine_kubernetes(
            self.id(),
            cloud_provider.borrow(),
            nodes.borrow(),
        );

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

        let my_progress_listener: Rc<Box<dyn ProgressListener>> =
            Rc::new(Box::new(MyProgressListener {
                task: self.clone(),
                sender: sender.clone(),
            }));

        let mut build_platform = self
            .request
            .build_platform
            .as_engine_build_platform(self.id());

        build_platform.add_listener(my_progress_listener.clone());

        let mut cloud_provider = self
            .request
            .cloud_provider
            .as_engine_cloud_provider(self.id(), self.request.organization_id.as_str());

        cloud_provider.add_listener(my_progress_listener.clone());

        let mut container_registry = self
            .request
            .container_registry
            .as_engine_container_registry(self.id());

        container_registry.add_listener(my_progress_listener.clone());

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

        let kubernetes = self.request.cloud_provider.kubernetes.as_engine_kubernetes(
            self.id(),
            cloud_provider.borrow(),
            nodes.borrow(),
        );

        let environment_action = self.request.environment_action().unwrap();

        match self.request.action {
            Action::Create => {
                tx.build_environment(&environment_action);
                tx.deploy_environment(kubernetes.borrow(), &environment_action);
            }
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

struct MyProgressListener<T>
where
    T: Task + Clone + 'static,
{
    task: T,
    sender: Sender<Message>,
}

impl<T> MyProgressListener<T>
where
    T: Task + Clone + 'static,
{
    fn get_internal_task(&self, status: Status) -> InternalTask {
        InternalTask {
            task: Box::new(self.task.clone()),
            status,
        }
    }
}

impl<T> ProgressListener for MyProgressListener<T>
where
    T: Task + Clone + 'static,
{
    fn on_progress(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Running {
            message: Some(info.message),
        });

        let it = self.sender.send(Ok(it));
    }

    fn on_complete(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Done {
            message: Some(info.message),
        });

        let it = self.sender.send(Ok(it));
    }

    fn on_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Error {
            message: Some(info.message),
        });

        let it = self.sender.send(Ok(it));
    }
}
