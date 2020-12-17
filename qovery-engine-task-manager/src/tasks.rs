#![feature(unboxed_closures)]
#![feature(fn_traits)]

use std::borrow::{Borrow, Cow};
use std::fs;
use std::rc::Rc;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use crossbeam_channel::Sender;

use qovery_engine::error::{EngineError, EngineErrorCause, EngineErrorScope, SimpleError};
use qovery_engine::models::{
    Context, EnvironmentAction, ProgressInfo, ProgressLevel, ProgressListener, ProgressScope,
};
use qovery_engine::s3;
use qovery_engine::transaction::{RollbackError, TransactionResult};

use crate::models::{Action, Request};
use crate::task_manager::{ActionContext, InternalTask, Message, PreRun, State, Status, Task};

#[derive(Clone)]
pub struct InfrastructureTask {
    context: Context,
    request: Request,
    pre_run_callback: Arc<Box<dyn Fn(&dyn Task) -> PreRun + Send + Sync>>,
}

impl InfrastructureTask {
    pub fn new(
        context: Context,
        request: Request,
        pre_run_callback: Box<dyn Fn(&dyn Task) -> PreRun + Send + Sync>,
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
            self.created_at().clone(),
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
    fn created_at(&self) -> &DateTime<Utc> {
        &self.request.created_at
    }

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

    fn pre_run(&self) -> PreRun {
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

        handle_transaction_result(
            tx.commit(),
            self,
            &self.request,
            self.action_context(ProgressLevel::Info),
            &sender,
        );

        match qovery_engine::fs::create_workspace_archive(
            engine.context().workspace_root_dir(),
            engine.context().execution_id(),
        ) {
            Ok(file) => match upload_s3_file(
                self.request.organization_id.as_str(),
                file.as_str(),
                self.request
                    .cloud_provider
                    .terraform_state_credentials
                    .secret_access_key
                    .as_str(),
                self.request
                    .cloud_provider
                    .terraform_state_credentials
                    .access_key_id
                    .as_str(),
            ) {
                Ok(_) => {
                    fs::remove_file(file);
                }
                Err(e) => error!("While uploading archive {:?}", e),
            },
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
    pre_run_callback: Arc<Box<dyn Fn(&dyn Task) -> PreRun + Send + Sync>>,
}

impl EnvironmentTask {
    pub fn new(
        context: Context,
        request: Request,
        pre_run_callback: Box<dyn Fn(&dyn Task) -> PreRun + Send + Sync>,
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
            self.created_at().clone(),
        )
    }
}

impl Task for EnvironmentTask {
    fn created_at(&self) -> &DateTime<Utc> {
        &self.request.created_at
    }

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

    fn pre_run(&self) -> PreRun {
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

        handle_transaction_result(
            tx.commit(),
            self,
            &self.request,
            self.action_context(ProgressLevel::Info),
            &sender,
        );

        match qovery_engine::fs::create_workspace_archive(
            engine.context().workspace_root_dir(),
            engine.context().execution_id(),
        ) {
            Ok(file) => match upload_s3_file(
                self.request.organization_id.as_str(),
                file.as_str(),
                self.request
                    .cloud_provider
                    .terraform_state_credentials
                    .secret_access_key
                    .as_str(),
                self.request
                    .cloud_provider
                    .terraform_state_credentials
                    .access_key_id
                    .as_str(),
            ) {
                Ok(_) => {
                    fs::remove_file(file);
                }
                Err(e) => error!("while uploading archive {:?}", e),
            },
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
        ActionContext::new(
            info.scope,
            info.level,
            info.execution_id.to_string(),
            self.task.created_at().clone(),
        )
    }
}

impl<T> ProgressListener for MyProgressListener<T>
where
    T: Task + Clone + 'static,
{
    fn start_in_progress(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::DeploymentInProgress,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn pause_in_progress(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::PauseInProgress,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn delete_in_progress(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::DeleteInProgress,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::Error,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn started(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::Deployed,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn paused(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::Paused,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn deleted(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::Deleted,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn start_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::DeploymentError,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn pause_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::PauseError,
            info.message.clone(),
            self.action_context(info),
        ));

        self.send(it);
    }

    fn delete_error(&self, info: ProgressInfo) {
        let it = self.get_internal_task(Status::new(
            State::DeleteError,
            info.message.clone(),
            self.action_context(info),
        ));

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
            Action::Create => Status::new(State::DeploymentError, message, context),
            Action::Pause => Status::new(State::PauseError, message, context),
            Action::Delete => Status::new(State::DeleteError, message, context),
        }
    } else if is_final {
        match request.action {
            Action::Create => Status::new(State::Deployed, message, context),
            Action::Pause => Status::new(State::Paused, message, context),
            Action::Delete => Status::new(State::Deleted, message, context),
        }
    } else {
        match request.action {
            Action::Create => Status::new(State::DeploymentInProgress, message, context),
            Action::Pause => Status::new(State::PauseInProgress, message, context),
            Action::Delete => Status::new(State::DeleteInProgress, message, context),
        }
    };

    task.send_status(sender, status);
}

fn handle_transaction_result(
    transaction_result: TransactionResult,
    task: &dyn Task,
    request: &Request,
    mut action_context: ActionContext,
    sender: &Sender<Message>,
) {
    match transaction_result {
        TransactionResult::Ok => {
            action_context.level = ProgressLevel::Info;

            send_progress(task, request, sender, action_context, None, false, true);
        }
        TransactionResult::Rollback(engine_error) => {
            action_context.level = ProgressLevel::Warn;

            send_progress(
                task,
                request,
                sender,
                action_context,
                Some(format_engine_error_output(engine_error, None)),
                true,
                false,
            );
        }
        TransactionResult::UnrecoverableError(engine_error, rollback_err) => {
            action_context.level = ProgressLevel::Error;

            send_progress(
                task,
                request,
                sender,
                action_context,
                Some(format_engine_error_output(engine_error, Some(rollback_err))),
                true,
                false,
            );
        }
    }
}

fn format_engine_error_output(
    engine_error: EngineError,
    rollback_error: Option<RollbackError>,
) -> String {
    let scope = match engine_error.scope {
        EngineErrorScope::Engine => String::from("Engine"),
        EngineErrorScope::BuildPlatform(id, name) => {
            format!("Build platform '{}' with id '{}'", name, id)
        }
        EngineErrorScope::ContainerRegistry(id, name) => {
            format!("Container registry '{}' with id '{}'", name, id)
        }
        EngineErrorScope::CloudProvider(id, name) => {
            format!("Cloud provider '{}' with id '{}'", name, id)
        }
        EngineErrorScope::Kubernetes(id, name) => format!("Kubernetes '{}' with id '{}'", name, id),
        EngineErrorScope::DnsProvider(id, name) => {
            format!("DNS provider '{}' with id '{}'", name, id)
        }
        EngineErrorScope::Environment(id, name) => {
            format!("Environment '{}' with id '{}'", name, id)
        }
        EngineErrorScope::Database(id, _type, name) => {
            format!("Database '{}' with id '{}'", name, id)
        }
        EngineErrorScope::Application(id, name) => {
            format!("Application '{}' with id '{}'", name, id)
        }
        EngineErrorScope::Router(id, name) => format!("Router '{}' with id '{}'", name, id),
        EngineErrorScope::ExternalService(id, name) => {
            format!("External service '{}' with id '{}'", name, id)
        }
    };

    let rollback_engine_error_message = match rollback_error {
        Some(rollback_error) => match rollback_error {
            RollbackError::CommitError(rollback_engine_error) => {
                if let Some(message) = rollback_engine_error.message {
                    Some(format!(
                        "{} (scope: {:?} | cause: {:?})",
                        message, rollback_engine_error.scope, rollback_engine_error.cause
                    ))
                } else {
                    None
                }
            }
            _ => None,
        },
        _ => None,
    };

    let rollback_message = match rollback_engine_error_message {
        Some(error_message) => format!("Rollback error: {}", error_message),
        None => String::new(),
    };

    match engine_error.cause {
        EngineErrorCause::Internal => format!(
            r#"

~~ THIS IS AN INTERNAL ERROR, THE SUPPORT TEAM HAS BEEN ALERTED ~~

Execution ID: {}
Scope: {}
{}

Error: {}

Join us on Discord (https://discord.qovery.com) to have more info and retry later the time that the support team handle the issue.
        "#,
            engine_error.execution_id,
            scope,
            rollback_message,
            engine_error.message.unwrap_or("<no error message>".into())
        ),
        EngineErrorCause::User(hint) => format!(
            r#"

Execution ID: {}
Scope: {}
{}

Error: {}

Hint: {}

Join us on Discord (https://discord.qovery.com) if you need support
        "#,
            engine_error.execution_id,
            scope,
            rollback_message,
            engine_error.message.unwrap_or("<no error message>".into()),
            hint
        ),
    }
}

pub fn get_archive_bucket_name() -> String {
    std::env::var("ARCHIVE_BUCKET_NAME").expect("env var ARCHIVE_BUCKET_NAME is mandatory")
}

fn basename(path: &str, sep: char) -> Cow<str> {
    let pieces = path.split(sep);
    match pieces.last() {
        Some(p) => p.into(),
        None => path.into(),
    }
}

fn upload_s3_file(
    organization_id: &str,
    file_path: &str,
    secrets_access_key: &str,
    access_key_id: &str,
) -> Result<(), SimpleError> {
    let object_key = format!("archives/{}/{}", organization_id, basename(file_path, '/'));
    let bucket_name = get_archive_bucket_name();
    info!(
        "Sending file {} to bucket {} object {} wth {} {}",
        file_path,
        bucket_name.clone(),
        object_key.clone(),
        access_key_id.clone(),
        secrets_access_key.clone(),
    );

    match s3::push_object(
        access_key_id,
        secrets_access_key,
        bucket_name.as_str(),
        object_key.as_str(),
        file_path,
    ) {
        Ok(_) => {
            info!("Archive successfully pushed to Qovery S3");
            return Ok(());
        }
        Err(e) => {
            warn!("Error while pushing archive to s3, {:?}", e);
            return Err(e);
        }
    };
}
