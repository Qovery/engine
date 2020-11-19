use std::error::Error;
use std::process::ExitStatus;

use rusoto_core::RusotoError;

pub type Type = String;
pub type Id = String;
pub type Name = String;

#[derive(Debug)]
pub struct EngineError {
    pub cause: EngineErrorCause,
    pub scope: EngineErrorScope,
    pub execution_id: String,
    pub message: Option<String>,
}

impl EngineError {
    pub fn new<T, S>(
        cause: EngineErrorCause,
        scope: EngineErrorScope,
        execution_id: T,
        message: Option<S>,
    ) -> Self
    where
        T: Into<String>,
        S: Into<String>,
    {
        EngineError {
            cause,
            scope,
            execution_id: execution_id.into(),
            message: match message {
                Some(message) => Some(message.into()),
                _ => None,
            },
        }
    }
}

#[derive(Debug)]
pub enum EngineErrorScope {
    Engine,
    BuildPlatform(Id, Name),
    ContainerRegistry(Id, Name),
    CloudProvider(Id, Name),
    Kubernetes(Id, Name),
    DnsProvider(Id, Name),
    Environment(Id, Name),
    Database(Id, Type, Name),
    Application(Id, Name),
    Router(Id, Name),
    ExternalService(Id, Name),
}

#[derive(Debug)]
pub enum EngineErrorCause {
    Internal,
    User(&'static str),
}

#[derive(Debug)]
pub struct SimpleError {
    pub kind: SimpleErrorKind,
    pub message: Option<String>,
}

#[derive(Debug)]
pub enum SimpleErrorKind {
    Command(ExitStatus),
    Other,
}

impl SimpleError {
    pub fn new<T: Into<String>>(kind: SimpleErrorKind, message: Option<T>) -> Self {
        SimpleError {
            kind,
            message: match message {
                Some(message) => Some(message.into()),
                _ => None,
            },
        }
    }
}

impl From<std::io::Error> for SimpleError {
    fn from(err: std::io::Error) -> Self {
        SimpleError::new(SimpleErrorKind::Other, Some(err.to_string()))
    }
}

pub fn cast_simple_error_to_engine_error<X, T: Into<String>>(
    scope: EngineErrorScope,
    execution_id: T,
    input: Result<X, SimpleError>,
) -> Result<X, EngineError> {
    match input {
        Err(simple_error) => {
            let message = match simple_error.kind {
                SimpleErrorKind::Command(exit_status) => format!(
                    "{} ({})",
                    simple_error.message.unwrap_or("<no message>".into()),
                    exit_status
                ),
                SimpleErrorKind::Other => simple_error.message.unwrap_or("<no message>".into()),
            };

            Err(EngineError::new(
                EngineErrorCause::Internal,
                scope,
                execution_id,
                Some(message),
            ))
        }
        Ok(x) => Ok(x),
    }
}
