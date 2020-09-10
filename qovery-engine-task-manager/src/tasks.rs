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
use qovery_engine::models::{Context, ProgressInfo, ProgressLevel, ProgressListener, ProgressStep};
use qovery_engine::transaction::{CommitError, TransactionResult};

use crate::models::{Action, Request};
use crate::task_manager::{ActionContext, InternalTask, Message, Scope, Status, Task};

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
            Action::Create => ProgressStep::CreateKubernetes,
            Action::Pause => ProgressStep::CreateKubernetes, // this is preferable to create a kubernetes cluster instead of deleting it
            Action::Delete => ProgressStep::DeleteKubernetes,
        };

        self.update_status(
            &sender,
            Status::Running {
                message: None,
                context: ActionContext::new(
                    Scope::Infrastructure,
                    progress_step.clone(),
                    ProgressLevel::Info,
                    self.infrastructure_id(),
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
                    Status::Failed {
                        message: Some(format!("failed to get engine session {:?}", err)),
                        context: ActionContext::new(
                            Scope::Infrastructure,
                            progress_step,
                            ProgressLevel::Error,
                            self.infrastructure_id(),
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
                    Status::Done {
                        message: None,
                        context: ActionContext::new(
                            Scope::Infrastructure,
                            progress_step,
                            ProgressLevel::Info,
                            self.infrastructure_id(),
                            self.id().to_string(),
                        ),
                    },
                );
            }
            TransactionResult::Rollback(commit_err) => {
                let ac = ActionContext::new(
                    Scope::Infrastructure,
                    progress_step,
                    ProgressLevel::Warn,
                    self.request
                        .target_environment
                        .as_ref()
                        .unwrap()
                        .id
                        .to_string(),
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::Failed {
                        message: Some(format!("rollback error - commit error: {:?}", commit_err)),
                        context: ac,
                    },
                );
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                let ac = ActionContext::new(
                    Scope::Infrastructure,
                    progress_step,
                    ProgressLevel::Error,
                    self.request
                        .target_environment
                        .as_ref()
                        .unwrap()
                        .id
                        .to_string(),
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::Failed {
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
            Action::Create => ProgressStep::DeployEnvironment,
            Action::Pause => ProgressStep::PauseEnvironment,
            Action::Delete => ProgressStep::DeleteEnvironment,
        };

        self.update_status(
            &sender,
            Status::Running {
                message: None,
                context: ActionContext::new(
                    Scope::Environment,
                    progress_step.clone(),
                    ProgressLevel::Info,
                    self.request
                        .target_environment
                        .as_ref()
                        .unwrap()
                        .id
                        .to_string(),
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
                    Status::Failed {
                        message: Some(format!("failed to get engine session {:?}", err)),
                        context: ActionContext::new(
                            Scope::Environment,
                            progress_step.clone(),
                            ProgressLevel::Info,
                            self.request
                                .target_environment
                                .as_ref()
                                .unwrap()
                                .id
                                .to_string(),
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

        let environment_action = self.request.environment_action().unwrap();

        match self.request.action {
            Action::Create => tx.deploy_environment(kubernetes.borrow(), &environment_action),
            Action::Pause => tx.pause_environment(kubernetes.borrow(), &environment_action),
            Action::Delete => tx.delete_environment(kubernetes.borrow(), &environment_action),
        };

        match tx.commit() {
            TransactionResult::Ok => {
                self.update_status(
                    &sender,
                    Status::Done {
                        message: None,
                        context: ActionContext::new(
                            Scope::Environment,
                            progress_step,
                            ProgressLevel::Info,
                            self.request
                                .target_environment
                                .as_ref()
                                .unwrap()
                                .id
                                .to_string(),
                            self.id().to_string(),
                        ),
                    },
                );
            }
            TransactionResult::Rollback(commit_err) => {
                let ac = ActionContext::new(
                    Scope::Environment,
                    progress_step,
                    ProgressLevel::Warn,
                    self.request
                        .target_environment
                        .as_ref()
                        .unwrap()
                        .id
                        .to_string(),
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::Failed {
                        message: Some(format!("rollback error - commit error: {:?}", commit_err)),
                        context: ac,
                    },
                );
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                let ac = ActionContext::new(
                    Scope::Environment,
                    progress_step,
                    ProgressLevel::Error,
                    self.request
                        .target_environment
                        .as_ref()
                        .unwrap()
                        .id
                        .to_string(),
                    self.id().to_string(),
                );

                self.update_status(
                    &sender,
                    Status::Failed {
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
            message: Some(info.message),
            context: ActionContext::new(
                Scope::Queued,
                info.step,
                info.level,
                info.execution_id.to_string(),
                info.execution_id.to_string(),
            ),
        });

        match self.sender.send(Ok(it)) {
            Ok(_) => {}
            Err(err) => error!("{:?}", err),
        };
    }

    fn on_complete(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::Done {
            message: Some(info.message),
            context: ActionContext::new(
                Scope::Queued,
                info.step,
                info.level,
                info.execution_id.to_string(),
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
            message: Some(info.message),
            context: ActionContext::new(
                Scope::Queued,
                info.step,
                info.level,
                info.execution_id.to_string(),
                info.execution_id.to_string(),
            ),
        });

        match self.sender.send(Ok(it)) {
            Ok(_) => {}
            Err(err) => error!("{:?}", err),
        };
    }
}
