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
use qovery_engine::s3;
use qovery_engine::transaction::{CommitError, TransactionResult};

use crate::models::{Action, Request};
use crate::task_manager::{ActionContext, InternalTask, Message, Status, Task};
use qovery_engine::cmd::utilities::CmdError;
use std::path::Path;

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

    fn send_status(&self, sender: &Sender<Message>, status: Status) {
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

        send_progress(
            self,
            &self.request,
            &sender,
            self.action_context(ProgressLevel::Info),
            None,
            false,
            false,
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
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Error),
                    Some(format!("failed to get engine session {:?}", err)),
                    true,
                    false,
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
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Info),
                    None,
                    false,
                    true,
                );
            }
            TransactionResult::Rollback(commit_err) => {
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Warn),
                    Some(format!("rollback error - commit error: {:?}", commit_err)),
                    true,
                    true,
                );
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Error),
                    Some(format!(
                        "unrecoverable error - commit error: {:?} - rollback error: {:?}",
                        commit_err, rollback_err
                    )),
                    true,
                    true,
                );
            }
        }

        match qovery_engine::fs::create_workspace_archive(
            engine.context().workspace_root_dir(),
            engine.context().execution_id(),
        ) {
            Ok(file) => {
                let secrets_key = self
                    .request
                    .cloud_provider
                    .terraform_state_credentials
                    .secret_access_key
                    .clone();
                let access_key = self
                    .request
                    .cloud_provider
                    .terraform_state_credentials
                    .access_key_id
                    .clone();
                let organization_id = self.request.organization_id.clone();
                let bucket_name = "qovery-terrafom-tfstates";
                let s3_status = s3::push_object(
                    &access_key,
                    &secrets_key,
                    bucket_name,
                    format!("archives/{}/{}", organization_id, file).as_str(),
                    file.as_str(),
                );
                match s3_status {
                    Ok(_) => info!("Archive successfully pushed to Qovery S3"),
                    Err(e) => warn!("While pushing archive to s3, {:}", e),
                }
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

    fn action_context(&self, level: ProgressLevel) -> ActionContext {
        let target_environment_id = self
            .request
            .target_environment
            .as_ref()
            .unwrap()
            .id
            .to_string();

        ActionContext::new(
            ProgressScope::Environment {
                id: target_environment_id,
            },
            level,
            self.id().to_string(),
        )
    }
}

impl Task for EnvironmentTask {
    fn group_id(&self) -> &str {
        self.group_id.as_str()
    }

    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn send_status(&self, sender: &Sender<Message>, status: Status) {
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

        send_progress(
            self,
            &self.request,
            &sender,
            self.action_context(ProgressLevel::Info),
            None,
            false,
            false,
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
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Error),
                    Some(format!("failed to get engine session {:?}", err)),
                    true,
                    true,
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
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Error),
                    Some("failed to get environment action, self.request.environment_action() returned None variant".to_string()),
                    true,
                    false,
                );
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
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Info),
                    None,
                    false,
                    true,
                );
            }
            TransactionResult::Rollback(commit_err) => {
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Warn),
                    Some(format!("rollback error - commit error: {:?}", commit_err)),
                    true,
                    true,
                );
            }
            TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
                send_progress(
                    self,
                    &self.request,
                    &sender,
                    self.action_context(ProgressLevel::Error),
                    Some(format!(
                        "unrecoverable error - commit error: {:?} - rollback error: {:?}",
                        commit_err, rollback_err
                    )),
                    true,
                    true,
                );
            }
        }

        match qovery_engine::fs::create_workspace_archive(
            engine.context().workspace_root_dir(),
            engine.context().execution_id(),
        ) {
            Ok(file) => {
                let secrets_key = self
                    .request
                    .cloud_provider
                    .terraform_state_credentials
                    .secret_access_key
                    .clone();
                let access_key = self
                    .request
                    .cloud_provider
                    .terraform_state_credentials
                    .access_key_id
                    .clone();
                let organization_id = self.request.organization_id.clone();
                let bucket_name = "qovery-terrafom-tfstates";
                let s3_status = s3::push_object(
                    &access_key,
                    &secrets_key,
                    bucket_name,
                    format!("archives/{}/{}", organization_id, file).as_str(),
                    file.as_str(),
                );
                match s3_status {
                    Ok(_) => info!("Archive successfully pushed to Qovery S3"),
                    Err(e) => warn!("While pushing archive to s3, {:}", e),
                }
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
        let it = self.get_internal_task(Status::DeploymentInProgress {
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
        let it = self.get_internal_task(Status::Deployed {
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
        let it = self.get_internal_task(Status::DeploymentError {
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

fn send_progress(
    task: &dyn Task,
    request: &Request,
    sender: &Sender<Message>,
    context: ActionContext,
    message: Option<String>,
    is_error: bool,
    is_final: bool,
) {
    let status = if is_error {
        match request.action {
            Action::Create => Status::DeploymentError { message, context },
            Action::Pause => Status::PauseError { message, context },
            Action::Delete => Status::DeleteError { message, context },
        }
    } else if is_final {
        match request.action {
            Action::Create => Status::Deployed { message, context },
            Action::Pause => Status::Paused { message, context },
            Action::Delete => Status::Deleted { message, context },
        }
    } else {
        match request.action {
            Action::Create => Status::DeploymentInProgress { message, context },
            Action::Pause => Status::PauseInProgress { message, context },
            Action::Delete => Status::DeleteInProgress { message, context },
        }
    };

    task.send_status(sender, status);
}
