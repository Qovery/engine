use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum DnsProviderError {
    #[error("Invalid credentials error.")]
    InvalidCredentials,
    #[error("Invalid API url error.")]
    InvalidApiUrl,
}
