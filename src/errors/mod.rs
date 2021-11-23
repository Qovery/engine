pub mod io;

extern crate url;

use crate::cloud_provider::Kind;
use url::Url;

pub struct SimpleError {
    message: String,
    message_safe: String,
}

#[derive(Debug)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug)]
pub enum Stage {
    Infrastructure(InfrastructureStep),
    Environment(EnvironmentStep),
}

#[derive(Debug)]
pub enum InfrastructureStep {
    Instantiate,
    Create,
    Upgrade,
    Delete,
}

#[derive(Debug)]
pub enum EnvironmentStep {
    Build,
    Deploy,
    Update,
    Delete,
}

#[derive(Debug)]
pub enum Tag {
    UnsupportedInstanceType(String),
}

impl SimpleError {
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn message_safe(&self) -> &str {
        &self.message_safe
    }

    pub fn new_from_safe_message(message: String) -> Self {
        SimpleError::new(message.clone(), message)
    }

    pub fn new(message: String, message_safe: String) -> Self {
        SimpleError { message, message_safe }
    }
}

#[derive(Debug)]
pub struct UserEngineError {
    provider_kind: Kind,
    execution_id: String,
    tag: Tag,
    stage: Stage,
    log_level: LogLevel,
    log_message: String,
    raw_message_safe: Option<String>,
    link: Option<Url>,
    hint_message: Option<String>,
}

impl UserEngineError {
    pub fn new(
        provider_kind: Kind,
        execution_id: String,
        tag: Tag,
        stage: Stage,
        log_level: LogLevel,
        log_message: String,
        raw_message_safe: Option<String>,
        link: Option<Url>,
        hint_message: Option<String>,
    ) -> Self {
        UserEngineError {
            provider_kind,
            execution_id,
            tag,
            stage,
            log_level,
            log_message,
            raw_message_safe,
            link,
            hint_message,
        }
    }
}

impl From<EngineError> for UserEngineError {
    fn from(error: EngineError) -> Self {
        UserEngineError::new(
            error.provider_kind,
            error.execution_id,
            error.tag,
            error.stage,
            error.user_log_level,
            error.user_log_message,
            error.raw_message_without_secrets,
            error.link,
            error.hint_message,
        )
    }
}

#[derive(Debug)]
pub struct EngineError {
    provider_kind: Kind,
    tag: Tag,
    execution_id: String,
    stage: Stage,
    qovery_log_level: LogLevel,
    qovery_log_message: String,
    user_log_level: LogLevel,
    user_log_message: String,
    raw_message: Option<String>,
    raw_message_without_secrets: Option<String>,
    link: Option<Url>,
    hint_message: Option<String>,
}

impl EngineError {
    pub fn provider_kind(&self) -> &Kind {
        &self.provider_kind
    }
    pub fn tag(&self) -> &Tag {
        &self.tag
    }
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }
    pub fn stage(&self) -> &Stage {
        &self.stage
    }
    pub fn qovery_log_level(&self) -> &LogLevel {
        &self.qovery_log_level
    }
    pub fn qovery_log_message(&self) -> &str {
        &self.qovery_log_message
    }
    pub fn user_log_level(&self) -> &LogLevel {
        &self.user_log_level
    }
    pub fn user_log_message(&self) -> &str {
        &self.user_log_message
    }
    pub fn raw_message(&self) -> Option<String> {
        self.raw_message.clone()
    }
    pub fn raw_message_without_secrets(&self) -> Option<String> {
        self.raw_message_without_secrets.clone()
    }
    pub fn link(&self) -> &Option<Url> {
        &self.link
    }
    pub fn hint_message(&self) -> &Option<String> {
        &self.hint_message
    }

    fn new(
        provider_kind: Kind,
        tag: Tag,
        execution_id: String,
        stage: Stage,
        qovery_log_level: LogLevel,
        qovery_log_message: String,
        user_log_level: LogLevel,
        user_log_message: String,
        raw_message: Option<String>,
        raw_message_without_secrets: Option<String>,
        link: Option<Url>,
        hint_message: Option<String>,
    ) -> Self {
        EngineError {
            provider_kind,
            tag,
            execution_id,
            stage,
            qovery_log_level,
            qovery_log_message,
            user_log_level,
            user_log_message,
            raw_message,
            raw_message_without_secrets,
            link,
            hint_message,
        }
    }

    pub fn to_user_error(self) -> UserEngineError {
        UserEngineError::from(self)
    }

    pub fn new_unsupported_instance_type(
        provider_kind: Kind,
        execution_id: &str,
        stage: Stage,
        requested_instance_type: &str,
        raw_message: Option<String>,
        raw_message_safe: Option<String>,
    ) -> EngineError {
        let message = format!("`{}` instance type is not supported", requested_instance_type);
        EngineError::new(
            provider_kind,
            Tag::UnsupportedInstanceType(requested_instance_type.to_string()),
            execution_id.to_string(),
            stage,
            LogLevel::Error,
            message.to_string(),
            LogLevel::Error,
            message.to_string(),
            raw_message,
            raw_message_safe,
            None, // TODO(documentation): Create a page entry to details this error
            Some("Selected instance type is not supported, please check provider's documentation.".to_string()),
        )
    }
}
