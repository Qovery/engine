use crate::events::Transmitter;
use std::process::ExitStatus;
use uuid::Uuid;

pub type Type = String;
pub type Id = Uuid;
pub type Name = String;
pub type Version = String;

#[derive(Debug)]
#[deprecated(note = "errors.EngineError to be used instead")]
pub struct EngineError {
    pub cause: EngineErrorCause,
    pub scope: EngineErrorScope,
    pub execution_id: String,
    pub message: Option<String>,
}

impl EngineError {
    pub fn new<T, S>(cause: EngineErrorCause, scope: EngineErrorScope, execution_id: T, message: Option<S>) -> Self
    where
        T: Into<String>,
        S: Into<String>,
    {
        EngineError {
            cause,
            scope,
            execution_id: execution_id.into(),
            message: message.map(|message| message.into()),
        }
    }

    pub fn is_cancel(&self) -> bool {
        self.cause == EngineErrorCause::Canceled
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
    ObjectStorage(Id, Name),
    Environment(Id, Name),
    Database(Id, Type, Name),
    Application(Id, Name, Version),
    Container(Id, Name, Version),
    Router(Id, Name),
    SecretManager(Name),
}

impl From<Transmitter> for EngineErrorScope {
    fn from(transmitter: Transmitter) -> Self {
        match transmitter {
            Transmitter::TaskManager => EngineErrorScope::Engine,
            Transmitter::BuildPlatform(id, name) => EngineErrorScope::BuildPlatform(id, name),
            Transmitter::ContainerRegistry(id, name) => EngineErrorScope::ContainerRegistry(id, name),
            Transmitter::CloudProvider(id, name) => EngineErrorScope::CloudProvider(id, name),
            Transmitter::Kubernetes(id, name) => EngineErrorScope::Kubernetes(id, name),
            Transmitter::DnsProvider(id, name) => EngineErrorScope::DnsProvider(id, name),
            Transmitter::ObjectStorage(id, name) => EngineErrorScope::ObjectStorage(id, name),
            Transmitter::Environment(id, name) => EngineErrorScope::Environment(id, name),
            Transmitter::Database(id, db_type, name) => EngineErrorScope::Database(id, db_type, name),
            Transmitter::Application(id, name, commit) => EngineErrorScope::Application(id, name, commit),
            Transmitter::Router(id, name) => EngineErrorScope::Router(id, name),
            Transmitter::SecretManager(name) => EngineErrorScope::SecretManager(name),
            Transmitter::Container(id, name, version) => EngineErrorScope::Container(id, name, version),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EngineErrorCause {
    Internal,
    Canceled,
    User(&'static str),
}

#[derive(Debug)]
#[deprecated(note = "errors.CommandError to be used instead")]
pub struct SimpleError {
    pub kind: SimpleErrorKind,
    pub message: Option<String>,
}

pub type StringError = String;

#[derive(Debug)]
pub enum SimpleErrorKind {
    Command(ExitStatus),
    Other,
}

impl SimpleError {
    pub fn new<T: Into<String>>(kind: SimpleErrorKind, message: Option<T>) -> Self {
        SimpleError {
            kind,
            message: message.map(|message| message.into()),
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
                SimpleErrorKind::Command(exit_status) => {
                    format!(
                        "{} ({})",
                        simple_error.message.unwrap_or_else(|| "<no message>".into()),
                        exit_status
                    )
                }
                SimpleErrorKind::Other => simple_error.message.unwrap_or_else(|| "<no message>".into()),
            };

            Err(EngineError::new(EngineErrorCause::Internal, scope, execution_id, Some(message)))
        }
        Ok(x) => Ok(x),
    }
}
