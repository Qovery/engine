#![feature(unboxed_closures)]
#![feature(fn_traits)]

use std::any::Any;
use std::borrow::Borrow;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crossbeam_channel::Sender;

use qovery_engine::cloud_provider::kubernetes::KubernetesError;
use qovery_engine::cloud_provider::service::ServiceError;
use qovery_engine::cloud_provider::CloudProviderError;
use qovery_engine::engine::Engine;
use qovery_engine::error::ConfigurationError;
use qovery_engine::models::{
    Context, EnvironmentAction, ProgressInfo, ProgressLevel, ProgressListener, ProgressScope,
};
use qovery_engine::transaction::{CommitError, TransactionResult};

use crate::models::{Action, Request};
use crate::task_manager::{ActionContext, InternalTask, Message, Status, Task};

#[derive(Clone)]
pub struct InfrastructureTask {
    context: Context,
    request: Request,
    pre_run_callback: Arc<Box<dyn Fn(&dyn Task) -> bool + Send + Sync>>,
}

impl InfrastructureTask {
    pub fn new(
        context: Context,
        request: Request,
        pre_run_callback: Box<dyn Fn(&dyn Task) -> bool + Send + Sync>,
    ) -> Self {
        InfrastructureTask {
            context,
            request,
            pre_run_callback: Arc::new(pre_run_callback),
        }
    }

    fn action_context(&self, level: ProgressLevel) -> ActionContext {
        ActionContext::new(
            ProgressScope::Infrastructure {
                execution_id: self.id().to_string(),
            },
            level,
            self.id().to_string(),
        )
    }

    fn infrastructure_id(&self) -> String {
        format!(
            "{}-{}-{}",
            self.request.cloud_provider.id.as_str(),
            self.request.container_registry.id.as_str(),
            self.request.build_platform.id.as_str()
        )
    }
}

impl Task for InfrastructureTask {
    fn group_id(&self) -> &str {
        self.request.organization_id.as_str()
    }

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

    fn pre_run(&self) -> bool {
        (self.pre_run_callback)(self)
    }

    fn run(&self, sender: Sender<Message>) {
        info!(
            "infrastructure task {} started with infrastructure id {}",
            self.id(),
            self.infrastructure_id()
        );

        self.update_status(
            &sender,
            Status::StartInProgress {
                message: None,
                context: self.action_context(ProgressLevel::Info),
            },
        );

        let my_progress_listener: Rc<Box<dyn ProgressListener>> =
            Rc::new(Box::new(MyProgressListener {
                task: self.clone(),
                sender: sender.clone(),
            }));

        let engine = self.request.engine(&self.context, my_progress_listener);

        let session = match engine.session() {
            Ok(session) => session,
            Err(err) => {
                self.update_status(
                    &sender,
                    Status::StartError {
                        message: Some(format!("failed to get engine session {:?}", err)),
                        context: self.action_context(ProgressLevel::Error),
                    },
                );

                return;
            }
        };

        let mut tx = session.transaction();

        let nodes = self
            .request
            .cloud_provider
            .kubernetes
            .to_engine_kubernetes_nodes();

        let kubernetes = self.request.cloud_provider.kubernetes.to_engine_kubernetes(
            engine.context(),
            engine.cloud_provider(),
            engine.dns_provider(),
            nodes.borrow(),
        );

        match self.request.action {
            Action::Create => tx.create_kubernetes(kubernetes.borrow()),
            Action::Pause => tx.create_kubernetes(kubernetes.borrow()),
            Action::Delete => tx.delete_kubernetes(kubernetes.borrow()),
        };

        // TODO implement on_progress callback and send status update in real time

        match tx.commit() {
            TransactionResult::Ok => {
                self.update_status(
                    &sender,
                    Status::Started {
                        message: None,
                        context: self.action_context(ProgressLevel::Info),
                    },
                );
            }
            TransactionResult::Rollback(commit_err) => {
                self.update_status(
                    &sender,
                    Status::StartError {
                        message: Some(format!("rollback error - commit error: {:?}", commit_err)),
                        context: self.action_context(ProgressLevel::Warn),
                    },
                );
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                self.update_status(
                    &sender,
                    Status::StartError {
                        message: Some(format!(
                            "unrecoverable error - commit error: {:?} - rollback error: {:?}",
                            commit_err, rollback_err
                        )),
                        context: self.action_context(ProgressLevel::Error),
                    },
                );
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
    group_id: String,
    context: Context,
    request: Request,
    pre_run_callback: Arc<Box<dyn Fn(&dyn Task) -> bool + Send + Sync>>,
}

impl EnvironmentTask {
    pub fn new(
        context: Context,
        request: Request,
        pre_run_callback: Box<dyn Fn(&dyn Task) -> bool + Send + Sync>,
    ) -> Self {
        EnvironmentTask {
            group_id: request.target_environment.as_ref().unwrap().id.clone(),
            context,
            request,
            pre_run_callback: Arc::new(pre_run_callback),
        }
    }
}

impl Task for EnvironmentTask {
    fn group_id(&self) -> &str {
        self.group_id.as_str()
    }

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

    fn pre_run(&self) -> bool {
        (self.pre_run_callback)(self)
    }

    fn run(&self, sender: Sender<Message>) {
        info!("environment task {} started", self.id());

        let target_environment_id = self
            .request
            .target_environment
            .as_ref()
            .unwrap()
            .id
            .to_string();

        self.update_status(
            &sender,
            Status::StartInProgress {
                message: None,
                context: ActionContext::new(
                    ProgressScope::Environment {
                        id: target_environment_id.clone(),
                    },
                    ProgressLevel::Info,
                    self.id().to_string(),
                ),
            },
        );

        let my_progress_listener: Rc<Box<dyn ProgressListener>> =
            Rc::new(Box::new(MyProgressListener {
                task: self.clone(),
                sender: sender.clone(),
            }));

        let engine = self.request.engine(&self.context, my_progress_listener);

        // FIXME - return errors with Sender
        let session = match engine.session() {
            Ok(session) => session,
            Err(err) => {
                // FIXME return error message
                self.update_status(
                    &sender,
                    Status::StartError {
                        message: Some(format!("failed to get engine session {:?}", err)),
                        context: ActionContext::new(
                            ProgressScope::Environment {
                                id: target_environment_id.clone(),
                            },
                            ProgressLevel::Info,
                            self.id().to_string(),
                        ),
                    },
                );

                return;
            }
        };

        let mut tx = session.transaction();

        let nodes = self
            .request
            .cloud_provider
            .kubernetes
            .to_engine_kubernetes_nodes();

        let kubernetes = self.request.cloud_provider.kubernetes.to_engine_kubernetes(
            engine.context(),
            engine.cloud_provider(),
            engine.dns_provider(),
            nodes.borrow(),
        );

        let environment_action = match self.request.environment_action() {
            None => {
                self.update_status(&sender,
                                   Status::Error {
                                       message: Some("failed to get environment action, self.request.environment_action() returned None variant".to_string()),
                                       context: ActionContext::new(
                                           ProgressScope::Environment {
                                               id: target_environment_id.clone()
                                           },
                                           ProgressLevel::Error,
                                           self.id().to_string(),
                                       ),
                                   });
                return;
            }
            Some(ea) => ea,
        };

        match self.request.action {
            Action::Create => tx.deploy_environment(kubernetes.borrow(), &environment_action),
            Action::Pause => tx.pause_environment(kubernetes.borrow(), &environment_action),
            Action::Delete => tx.delete_environment(kubernetes.borrow(), &environment_action),
        };

        match tx.commit() {
            TransactionResult::Ok => {
                self.update_status(
                    &sender,
                    Status::Started {
                        message: None,
                        context: ActionContext::new(
                            ProgressScope::Environment {
                                id: target_environment_id.clone(),
                            },
                            ProgressLevel::Info,
                            self.id().to_string(),
                        ),
                    },
                );
            }
            TransactionResult::Rollback(commit_err) => {
                let ac = ActionContext::new(
                    ProgressScope::Environment {
                        id: target_environment_id.clone(),
                    },
                    ProgressLevel::Warn,
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::StartError {
                        message: Some(format!("rollback error - commit error: {:?}", commit_err)),
                        context: ac,
                    },
                );
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                let ac = ActionContext::new(
                    ProgressScope::Environment {
                        id: target_environment_id.clone(),
                    },
                    ProgressLevel::Error,
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::StartError {
                        message: Some(format!(
                            "unrecoverable error - commit error: {:?} - rollback error: {:?}",
                            commit_err, rollback_err
                        )),
                        context: ac,
                    },
                );
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

    fn send(&self, internal_task: InternalTask) {
        match self.sender.send(Ok(internal_task)) {
            Ok(_) => {}
            Err(err) => error!("{:?}", err),
        };
    }

    fn action_context(&self, info: ProgressInfo) -> ActionContext {
        ActionContext::new(info.scope, info.level, info.execution_id.to_string())
    }
}

impl<T> ProgressListener for MyProgressListener<T>
where
    T: Task + Clone + 'static,
{
    fn start_in_progress(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::StartInProgress {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn pause_in_progress(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::PauseInProgress {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn delete_in_progress(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::DeleteInProgress {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Error {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn started(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Started {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn paused(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Paused {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn deleted(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Deleted {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn start_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::StartError {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn pause_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::PauseError {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }

    fn delete_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::DeleteError {
            message: info.message.clone(),
            context: self.action_context(info),
        });

        self.send(it);
    }
}
