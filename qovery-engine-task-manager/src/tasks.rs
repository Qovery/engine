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
use qovery_engine::models::{Context, ProgressInfo, ProgressLevel, ProgressListener, ProgressScope, ProgressStep, EnvironmentAction};
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

        let progress_step = match self.request.action {
            Action::Create => ProgressStep::Create,
            Action::Pause => ProgressStep::Create, // this is preferable to create a kubernetes cluster instead of deleting it
            Action::Delete => ProgressStep::Delete,
        };

        self.update_status(
            &sender,
            Status::Running {
                message: None,
                context: ActionContext::new(
                    ProgressScope::Infrastructure {
                        execution_id: self.id().to_string(),
                    },
                    progress_step.clone(),
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

        let session = match engine.session() {
            Ok(session) => session,
            Err(err) => {
                self.update_status(
                    &sender,
                    Status::TerminatedWithError {
                        message: Some(format!("failed to get engine session {:?}", err)),
                        context: ActionContext::new(
                            ProgressScope::Infrastructure {
                                execution_id: self.id().to_string(),
                            },
                            progress_step,
                            ProgressLevel::Error,
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
                    Status::Terminated {
                        message: None,
                        context: ActionContext::new(
                            ProgressScope::Infrastructure {
                                execution_id: self.id().to_string(),
                            },
                            progress_step,
                            ProgressLevel::Info,
                            self.id().to_string(),
                        ),
                    },
                );
            }
            TransactionResult::Rollback(commit_err) => {
                let ac = ActionContext::new(
                    ProgressScope::Infrastructure {
                        execution_id: self.id().to_string(),
                    },
                    progress_step,
                    ProgressLevel::Warn,
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::TerminatedWithError {
                        message: Some(format!("rollback error - commit error: {:?}", commit_err)),
                        context: ac,
                    },
                );
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                let ac = ActionContext::new(
                    ProgressScope::Infrastructure {
                        execution_id: self.id().to_string(),
                    },
                    progress_step,
                    ProgressLevel::Error,
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::TerminatedWithError {
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

        let progress_step = match self.request.action {
            Action::Create => ProgressStep::Deploy,
            Action::Pause => ProgressStep::Pause,
            Action::Delete => ProgressStep::Delete,
        };

        let target_environment_id = self
            .request
            .target_environment
            .as_ref()
            .unwrap()
            .id
            .to_string();

        self.update_status(
            &sender,
            Status::Running {
                message: None,
                context: ActionContext::new(
                    ProgressScope::Environment {
                        id: target_environment_id.clone(),
                    },
                    progress_step.clone(),
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
                    Status::TerminatedWithError {
                        message: Some(format!("failed to get engine session {:?}", err)),
                        context: ActionContext::new(
                            ProgressScope::Environment {
                                id: target_environment_id.clone(),
                            },
                            progress_step.clone(),
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
            nodes.borrow(),
        );

        let environment_action = match self.request.environment_action() {
            None => {
              self.update_status(&sender,
              Status::Error {
                  message: Some("failed to get environment action, self.request.environment_action() returned None variant")
                  context: ActionContext::new(ProgressScope::Environment {
                      id: target_environment_id.clone()
                  },
                  progress_step.clone(),
                      ProgressLevel::Error,
                      self.id().to_string()
                  ),
              });
                return;
            },
            Some(ea) => ea
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
                    Status::Terminated {
                        message: None,
                        context: ActionContext::new(
                            ProgressScope::Environment {
                                id: target_environment_id.clone(),
                            },
                            progress_step,
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
                    progress_step,
                    ProgressLevel::Warn,
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::TerminatedWithError {
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
                    progress_step,
                    ProgressLevel::Error,
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::TerminatedWithError {
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
}

impl<T> ProgressListener for MyProgressListener<T>
where
    T: Task + Clone + 'static,
{
    fn on_progress(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Running {
            message: info.message,
            context: ActionContext::new(
                info.scope,
                info.step,
                info.level,
                info.execution_id.to_string(),
            ),
        });

        match self.sender.send(Ok(it)) {
            Ok(_) => {}
            Err(err) => error!("{:?}", err),
        };
    }

    fn on_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Error {
            message: info.message,
            context: ActionContext::new(
                info.scope,
                info.step,
                info.level,
                info.execution_id.to_string(),
            ),
        });

        match self.sender.send(Ok(it)) {
            Ok(_) => {}
            Err(err) => error!("{:?}", err),
        };
    }

    fn on_complete(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Terminated {
            message: info.message,
            context: ActionContext::new(
                info.scope,
                info.step,
                info.level,
                info.execution_id.to_string(),
            ),
        });

        match self.sender.send(Ok(it)) {
            Ok(_) => {}
            Err(err) => error!("{:?}", err),
        };
    }

    fn on_complete_with_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::TerminatedWithError {
            message: info.message,
            context: ActionContext::new(
                info.scope,
                info.step,
                info.level,
                info.execution_id.to_string(),
            ),
        });

        match self.sender.send(Ok(it)) {
            Ok(_) => {}
            Err(err) => error!("{:?}", err),
        };
    }
}
