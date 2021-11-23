use super::url::Url;
use crate::cloud_provider::Kind;
use crate::errors;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl From<LogLevel> for errors::LogLevel {
    fn from(log_level: LogLevel) -> Self {
        match log_level {
            LogLevel::Debug => errors::LogLevel::Debug,
            LogLevel::Info => errors::LogLevel::Info,
            LogLevel::Warning => errors::LogLevel::Warning,
            LogLevel::Error => errors::LogLevel::Error,
            LogLevel::Critical => errors::LogLevel::Critical,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum Stage {
    Infrastructure(InfrastructureStep),
    Environment(EnvironmentStep),
}

impl From<Stage> for errors::Stage {
    fn from(stage: Stage) -> Self {
        match stage {
            Stage::Infrastructure(steps) => errors::Stage::Infrastructure(errors::InfrastructureStep::from(steps)),
            Stage::Environment(steps) => errors::Stage::Environment(errors::EnvironmentStep::from(steps)),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum InfrastructureStep {
    Instantiate,
    Create,
    Upgrade,
    Delete,
}

impl From<InfrastructureStep> for errors::InfrastructureStep {
    fn from(step: InfrastructureStep) -> Self {
        match step {
            InfrastructureStep::Instantiate => errors::InfrastructureStep::Instantiate,
            InfrastructureStep::Create => errors::InfrastructureStep::Create,
            InfrastructureStep::Upgrade => errors::InfrastructureStep::Upgrade,
            InfrastructureStep::Delete => errors::InfrastructureStep::Delete,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum EnvironmentStep {
    Build,
    Deploy,
    Update,
    Delete,
}

impl From<EnvironmentStep> for errors::EnvironmentStep {
    fn from(step: EnvironmentStep) -> Self {
        match step {
            EnvironmentStep::Build => errors::EnvironmentStep::Build,
            EnvironmentStep::Deploy => errors::EnvironmentStep::Deploy,
            EnvironmentStep::Update => errors::EnvironmentStep::Update,
            EnvironmentStep::Delete => errors::EnvironmentStep::Delete,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum Tag {
    UnsupportedInstanceType(String),
}

impl From<Tag> for errors::Tag {
    fn from(tag: Tag) -> Self {
        match tag {
            Tag::UnsupportedInstanceType(s) => errors::Tag::UnsupportedInstanceType(s),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct UserEngineError {
    provider_kind: Kind,
    execution_id: String,
    tag: Tag,
    stage: Stage,
    log_level: LogLevel,
    log_message: String,
    raw_message_safe: Option<String>,
    link: Option<String>,
    hint_message: Option<String>,
}

impl From<UserEngineError> for errors::UserEngineError {
    fn from(e: UserEngineError) -> Self {
        errors::UserEngineError::new(
            e.provider_kind,
            e.execution_id,
            errors::Tag::from(e.tag),
            errors::Stage::from(e.stage),
            errors::LogLevel::from(e.log_level),
            e.log_message,
            e.raw_message_safe,
            match e.link {
                Some(url) => match Url::from_str(&url) {
                    Ok(url) => Some(url),
                    Err(_) => None,
                },
                None => None,
            },
            e.hint_message,
        )
    }
}
