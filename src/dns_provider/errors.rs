use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum DnsProviderError {
    #[error("Invalid credentials error.")]
    InvalidCredentials,
    #[error("Invalid API url error.")]
    InvalidApiUrl,
}
