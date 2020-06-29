use rusoto_core::RusotoError;

use crate::cloud_provider::error::CloudProviderError::Error;
use crate::cmd::CmdError;
use std::process::ExitStatus;

#[derive(Debug)]
pub enum CloudProviderError {
    Credentials,
    Error(Box<dyn std::error::Error>),
    Unknown,
}

impl From<Box<dyn std::error::Error>> for CloudProviderError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Error(error)
    }
}

impl<E> From<RusotoError<E>> for CloudProviderError {
    fn from(error: RusotoError<E>) -> Self {
        match error {
            RusotoError::Credentials(_) => CloudProviderError::Credentials,
            RusotoError::Service(_) => CloudProviderError::Unknown,
            RusotoError::HttpDispatch(_) => CloudProviderError::Unknown,
            RusotoError::Validation(_) => CloudProviderError::Unknown,
            RusotoError::ParseError(_) => CloudProviderError::Unknown,
            RusotoError::Unknown(e) => {
                if e.status == 403 {
                    CloudProviderError::Credentials
                } else {
                    CloudProviderError::Unknown
                }
            }
            RusotoError::Blocking => CloudProviderError::Unknown,
        }
    }
}

#[derive(Debug)]
pub enum KubernetesError {
    Cmd(CmdError),
    Io(std::io::Error),
    Create(ExitStatus),
    Error,
}

impl From<std::io::Error> for KubernetesError {
    fn from(error: std::io::Error) -> Self {
        KubernetesError::Io(error)
    }
}

impl From<CmdError> for KubernetesError {
    fn from(error: CmdError) -> Self {
        KubernetesError::Cmd(error)
    }
}

#[derive(Debug)]
pub enum ServiceError {}

#[derive(Debug)]
pub enum DeployError {
    Error,
}
