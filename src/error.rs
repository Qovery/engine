use crate::error::QError::{Credentials, Error};
use rusoto_core::RusotoError;
use rusoto_eks::ListClustersError;

pub type QResult<T> = Result<T, QError>;

#[derive(Debug)]
pub enum QError {
    Credentials,
    Error(Box<dyn std::error::Error>),
    Unknown,
}

impl From<Box<dyn std::error::Error>> for QError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Error(error)
    }
}

impl<E> From<RusotoError<E>> for QError {
    fn from(error: RusotoError<E>) -> Self {
        match error {
            RusotoError::Credentials(_) => QError::Credentials,
            RusotoError::Service(_) => QError::Unknown,
            RusotoError::HttpDispatch(_) => QError::Unknown,
            RusotoError::Validation(_) => QError::Unknown,
            RusotoError::ParseError(_) => QError::Unknown,
            RusotoError::Unknown(e) => {
                if e.status == 403 {
                    QError::Credentials
                } else {
                    QError::Unknown
                }
            }
            RusotoError::Blocking => QError::Unknown,
        }
    }
}
