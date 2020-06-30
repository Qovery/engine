use rusoto_core::RusotoError;

#[derive(Debug, Eq, PartialEq)]
pub enum ContainerRegistryError {
    Credentials,
    Unknown,
}

impl<E> From<RusotoError<E>> for ContainerRegistryError {
    fn from(error: RusotoError<E>) -> Self {
        match error {
            RusotoError::Credentials(_) => ContainerRegistryError::Credentials,
            RusotoError::Service(_) => ContainerRegistryError::Unknown,
            RusotoError::HttpDispatch(_) => ContainerRegistryError::Unknown,
            RusotoError::Validation(_) => ContainerRegistryError::Unknown,
            RusotoError::ParseError(_) => ContainerRegistryError::Unknown,
            RusotoError::Unknown(e) => {
                if e.status == 403 {
                    ContainerRegistryError::Credentials
                } else {
                    ContainerRegistryError::Unknown
                }
            }
            RusotoError::Blocking => ContainerRegistryError::Unknown,
        }
    }
}
