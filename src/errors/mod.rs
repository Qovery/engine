pub mod io;

extern crate url;

use crate::error::{EngineError as LegacyEngineError, EngineErrorCause, EngineErrorScope};
use crate::events::EventDetails;
use url::Url;

pub struct SimpleError {
    message: String,
    message_safe: String,
}

#[derive(Clone, Debug)]
pub enum Tag {
    UnsupportedInstanceType(String),
}

impl SimpleError {
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn message_safe(&self) -> &str {
        &self.message_safe
    }

    pub fn new_from_safe_message(message: String) -> Self {
        SimpleError::new(message.clone(), message)
    }

    pub fn new(message: String, message_safe: String) -> Self {
        SimpleError { message, message_safe }
    }
}

#[derive(Debug)]
pub struct UserEngineError {
    event_details: EventDetails,
    log_message: String,
    raw_message_safe: Option<String>,
    link: Option<Url>,
    hint_message: Option<String>,
}

impl UserEngineError {
    pub fn new(
        event_details: EventDetails,
        log_message: String,
        raw_message_safe: Option<String>,
        link: Option<Url>,
        hint_message: Option<String>,
    ) -> Self {
        UserEngineError {
            event_details,
            log_message,
            raw_message_safe,
            link,
            hint_message,
        }
    }
}

impl From<EngineError> for UserEngineError {
    fn from(error: EngineError) -> Self {
        UserEngineError::new(
            error.event_details,
            error.user_log_message,
            error.raw_message_safe,
            error.link,
            error.hint_message,
        )
    }
}

#[derive(Clone, Debug)]
pub struct EngineError {
    tag: Tag,
    event_details: EventDetails,
    qovery_log_message: String,
    user_log_message: String,
    raw_message: Option<String>,
    raw_message_safe: Option<String>,
    link: Option<Url>,
    hint_message: Option<String>,
}

impl EngineError {
    pub fn event_details(&self) -> &EventDetails {
        &self.event_details
    }
    pub fn qovery_log_message(&self) -> &str {
        &self.qovery_log_message
    }
    pub fn user_log_message(&self) -> &str {
        &self.user_log_message
    }
    pub fn raw_message(&self) -> Option<String> {
        self.raw_message.clone()
    }
    pub fn raw_message_without_secrets(&self) -> Option<String> {
        self.raw_message_safe.clone()
    }
    pub fn link(&self) -> &Option<Url> {
        &self.link
    }
    pub fn hint_message(&self) -> &Option<String> {
        &self.hint_message
    }

    fn new(
        event_details: EventDetails,
        tag: Tag,
        qovery_log_message: String,
        user_log_message: String,
        raw_message: Option<String>,
        raw_message_safe: Option<String>,
        link: Option<Url>,
        hint_message: Option<String>,
    ) -> Self {
        EngineError {
            event_details,
            tag,
            qovery_log_message,
            user_log_message,
            raw_message,
            raw_message_safe,
            link,
            hint_message,
        }
    }

    pub fn to_user_error(self) -> UserEngineError {
        UserEngineError::from(self)
    }

    pub fn to_legacy_engine_error(self) -> LegacyEngineError {
        LegacyEngineError::new(
            EngineErrorCause::Internal,
            EngineErrorScope::from(self.event_details.transmitter()),
            self.event_details.execution_id().to_string(),
            self.raw_message_safe,
        )
    }

    pub fn new_unsupported_instance_type(
        event_details: EventDetails,
        requested_instance_type: &str,
        raw_message: String,
    ) -> EngineError {
        let message = format!("`{}` instance type is not supported", requested_instance_type);
        EngineError::new(
            event_details,
            Tag::UnsupportedInstanceType(requested_instance_type.to_string()),
            message.to_string(),
            message.to_string(),
            Some(raw_message.clone()),
            Some(raw_message),
            None, // TODO(documentation): Create a page entry to details this error
            Some("Selected instance type is not supported, please check provider's documentation.".to_string()),
        )
    }
}
