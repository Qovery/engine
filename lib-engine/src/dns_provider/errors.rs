use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum DnsProviderError {
    #[error("Invalid credentials error.")]
    InvalidCredentials,
}
