use crate::errors::EngineError;
use crate::events::EventDetails;
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum DnsProviderError {
    #[error("Invalid credentials error.")]
    InvalidCredentials,
    #[error("Invalid API url error.")]
    InvalidApiUrl,
}

impl DnsProviderError {
    pub fn to_engine_error(&self, event_details: EventDetails) -> EngineError {
        match self {
            DnsProviderError::InvalidCredentials => {
                EngineError::new_error_on_dns_provider_invalid_credentials(event_details)
            }
            DnsProviderError::InvalidApiUrl => EngineError::new_error_on_dns_provider_invalid_api_url(event_details),
        }
    }
}
