use std::borrow::Borrow;
use std::rc::Rc;

use crossbeam_channel::Sender;

use qovery_engine::cloud_provider::CloudProviderError;
use qovery_engine::engine::Engine;
use qovery_engine::error::ConfigurationError;
use qovery_engine::models::{Context, ProgressInfo, ProgressListener};
use qovery_engine::transaction::{TransactionResult, CommitError, ActionContext};

use crate::models::{Action, Request};
use crate::task_manager::{InternalTask, Message, Status, Task};
use qovery_engine::cloud_provider::kubernetes::KubernetesError;
use qovery_engine::cloud_provider::service::ServiceError;
use std::any::Any;

#[derive(Clone)]
pub struct InfrastructureTask {
    context: Context,
    request: Request,
}

impl InfrastructureTask {
    pub fn new(context: Context, request: Request) -> Self {
        InfrastructureTask { context, request }
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
        self.update_status(&sender, Status::Running { message: None, context: None });

        let my_progress_listener: Rc<Box<dyn ProgressListener>> =
            Rc::new(Box::new(MyProgressListener {
                task: self.clone(),
                sender: sender.clone(),
            }));

        let engine = self.request.engine(&self.context, my_progress_listener);

        // FIXME - return errors with Sender
        let session = match engine.session() {
            Ok(session) => Some(session),
            Err(err) => {
                // FIXME return error message
                self.update_status(&sender, Status::Failed { message: None, context: None });
                None
            }
        };

        if session.is_none() {
            self.update_status(&sender, Status::Failed { message: None, context: None });
            return;
        }

        let mut tx = session.unwrap().transaction();

        let nodes = self
            .request
            .cloud_provider
            .kubernetes
            .to_engine_kubernetes_nodes();

        let kubernetes = self.request.cloud_provider.kubernetes.to_engine_kubernetes(
            engine.context(),
            engine.cloud_provider(),
            nodes.borrow(),
        );

        match self.request.action {
            Action::Create => tx.create_kubernetes(kubernetes.borrow()),
            Action::Delete => tx.delete_kubernetes(kubernetes.borrow()),
        };

        // TODO implement on_progress callback and send status update in real time

        match tx.commit() {
            TransactionResult::Ok => {
                self.update_status(&sender, Status::Done { message: None, context: None });
            }
            TransactionResult::Rollback(commit_err) => {
                // FIXME return error message
                let err = ServiceError::from(commit_err);
                let ac = Option::from(err);
                self.update_status(&sender, Status::Failed {
                    message: None,
                    context: ac,
                })
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                // FIXME return error message
                let err = ServiceError::from(commit_err);
                let ac = Option::from(err);
                self.update_status(&sender, Status::Failed {
                    message: None,
                    context: Option::from(ac),
                })
            }
        }

        match qovery_engine::fs::create_workspace_archive(
            engine.context().workspace_root_dir(),
            engine.context().execution_id(),
        ) {
            Ok(file) => {
                // TODO upload archive
            }
            Err(err) => error!("{:?}", err),
        };

        info!("infrastructure task {} finished", self.id());
    }
}

#[derive(Clone)]
pub struct EnvironmentTask {
    context: Context,
    request: Request,
}

impl EnvironmentTask {
    pub fn new(context: Context, request: Request) -> Self {
        EnvironmentTask { context, request }
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
        self.update_status(&sender, Status::Running { message: None, context: None });

        let my_progress_listener: Rc<Box<dyn ProgressListener>> =
            Rc::new(Box::new(MyProgressListener {
                task: self.clone(),
                sender: sender.clone(),
            }));

        let engine = self.request.engine(&self.context, my_progress_listener);

        // FIXME - return errors with Sender
        let session = match engine.session() {
            Ok(session) => Some(session),
            Err(err) => {
                // FIXME return error message
                self.update_status(&sender, Status::Failed { message: None, context: None });
                None
            }
        };

        if session.is_none() {
            self.update_status(&sender, Status::Failed { message: None, context: None });
            return;
        }

        let mut tx = session.unwrap().transaction();

        let nodes = self
            .request
            .cloud_provider
            .kubernetes
            .to_engine_kubernetes_nodes();

        let kubernetes = self.request.cloud_provider.kubernetes.to_engine_kubernetes(
            engine.context(),
            engine.cloud_provider(),
            nodes.borrow(),
        );

        let environment_action = self.request.environment_action().unwrap();

        match self.request.action {
            Action::Create => {
                tx.deploy_environment(kubernetes.borrow(), &environment_action);
            }
            Action::Delete => unimplemented!(),
        };

        // TODO implement on_progress callback and send status update in real time

        match tx.commit() {
            TransactionResult::Ok => {
                self.update_status(&sender, Status::Done { message: None, context: None });
            }
            TransactionResult::Rollback(commit_err) => {
                // FIXME return error message
                let ac = Option::from(ServiceError::from(commit_err));
                self.update_status(&sender, Status::Failed { message: None, context: Option::from(ac) });
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                // FIXME return error message
                let ac = Option::from(ServiceError::from(commit_err));
                self.update_status(&sender, Status::Failed { message: None, context: Option::from(ac) });
            }
        }

        match qovery_engine::fs::create_workspace_archive(
            engine.context().workspace_root_dir(),
            engine.context().execution_id(),
        ) {
            Ok(file) => {
                // TODO upload archive
            }
            Err(err) => error!("{:?}", err),
        };

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
            context: None
        });

        let it = self.sender.send(Ok(it));
    }

    fn on_complete(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Done {
            message: Some(info.message),
            context: None
        });

        let it = self.sender.send(Ok(it));
    }

    fn on_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Error {
            message: Some(info.message),
            context: None
        });

        let it = self.sender.send(Ok(it));
    }
}
