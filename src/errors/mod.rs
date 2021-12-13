pub mod io;

extern crate url;

use crate::error::{EngineError as LegacyEngineError, EngineErrorCause, EngineErrorScope};
use crate::events::EventDetails;
use url::Url;

/// SimpleError: simple error, mostly returned by third party tools.
pub struct SimpleError {
    /// message: full error message, can contains unsafe text such as passwords and tokens.
    message: String,
    /// message_safe: error message omitting displaying any protected data such as passwords and tokens.
    message_safe: String,
}

impl SimpleError {
    /// Returns SimpleError message. May contains unsafe text such as passwords and tokens.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns SimpleError message_safe omitting all unsafe text such as passwords and tokens.
    pub fn message_safe(&self) -> &str {
        &self.message_safe
    }

    /// Creates a new SimpleError from safe message. To be used when message is safe.
    pub fn new_from_safe_message(message: String) -> Self {
        SimpleError::new(message.clone(), message)
    }

    /// Creates a new SimpleError having both a safe and an unsafe message.
    pub fn new(message: String, message_safe: String) -> Self {
        SimpleError { message, message_safe }
    }
}

#[derive(Clone, Debug)]
/// Tag: unique identifier for an error.
pub enum Tag {
    /// Unknown: unknown error.
    Unknown,
    /// UnsupportedInstanceType: represents an unsupported instance type for the given cloud provider.
    UnsupportedInstanceType,
}

#[derive(Clone, Debug)]
/// EngineError: represents an engine error. Engine will always returns such errors carrying context infos easing monitoring and debugging.
pub struct EngineError {
    /// tag: error unique identifier
    tag: Tag,
    /// event_details: holds context details in which error was triggered such as organization ID, cluster ID, etc.
    event_details: EventDetails,
    /// qovery_log_message: message targeted toward Qovery team, carrying eventual debug / more fine grained messages easing investigations.
    qovery_log_message: String,
    /// user_log_message: message targeted toward Qovery users, might avoid any useless info for users such as Qovery specific identifiers and so on.
    user_log_message: String,
    /// raw_message: raw error message such as command input / output which may contains unsafe text such as plain passwords / tokens.
    raw_message: Option<String>,
    /// raw_message_safe: raw error message such as command input / output in which all unsafe data is omitted (passwords and tokens).
    raw_message_safe: Option<String>,
    /// link: link to error documentation (qovery blog, forum, etc.)
    link: Option<Url>,
    /// hint_message: an hint message aiming to give an hint to the user. For example: "Happens when application port has been changed but application hasn't been restarted.".
    hint_message: Option<String>,
}

impl EngineError {
    /// Returns error's unique identifier.
    pub fn tag(&self) -> &Tag {
        &self.tag
    }

    /// Returns error's event details.
    pub fn event_details(&self) -> &EventDetails {
        &self.event_details
    }

    /// Returns qovery log message.
    pub fn qovery_log_message(&self) -> &str {
        &self.qovery_log_message
    }

    /// Returns user log message.
    pub fn user_log_message(&self) -> &str {
        &self.user_log_message
    }

    /// Returns proper error message (safe if exists, otherwise raw, otherwise default error message).
    pub fn message(&self) -> String {
        if let Some(msg) = &self.raw_message_safe {
            return msg.to_string();
        }

        if let Some(msg) = &self.raw_message {
            return msg.to_string();
        }

        "no error message defined".to_string()
    }

    /// Returns error's link.
    pub fn link(&self) -> &Option<Url> {
        &self.link
    }

    /// Returns error's hint message.
    pub fn hint_message(&self) -> &Option<String> {
        &self.hint_message
    }

    /// Creates new EngineError.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `tag`: Error unique identifier.
    /// * `qovery_log_message`: Error log message targeting Qovery team for investigation / monitoring purposes.
    /// * `user_log_message`: Error log message targeting Qovery user, avoiding any extending pointless details.
    /// * `raw_message`: Error raw message such as command input / output which may contains unsafe text such as plain passwords / tokens.
    /// * `raw_message_safe`: Error raw message such as command input / output where any unsafe data as been omitted (such as plain passwords / tokens).
    /// * `link`: Link documenting the given error.
    /// * `hint_message`: hint message aiming to give an hint to the user. For example: "Happens when application port has been changed but application hasn't been restarted.".
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

    /// Converts to legacy engine error easing migration.
    pub fn to_legacy_engine_error(self) -> LegacyEngineError {
        LegacyEngineError::new(
            EngineErrorCause::Internal,
            EngineErrorScope::from(self.event_details.transmitter()),
            self.event_details.execution_id().to_string(),
            self.raw_message_safe,
        )
    }

    /// Creates new unknown error.
    ///
    /// Note: do not use unless really needed, every error should have a clear type.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `qovery_log_message`: Error log message targeting Qovery team for investigation / monitoring purposes.
    /// * `user_log_message`: Error log message targeting Qovery user, avoiding any extending pointless details.
    /// * `raw_message`: Error raw message such as command input / output which may contains unsafe text such as plain passwords / tokens.
    /// * `raw_message_safe`: Error raw message such as command input / output where any unsafe data as been omitted (such as plain passwords / tokens).
    /// * `link`: Link documenting the given error.
    /// * `hint_message`: hint message aiming to give an hint to the user. For example: "Happens when application port has been changed but application hasn't been restarted.".
    pub fn new_unknown(
        event_details: EventDetails,
        qovery_log_message: String,
        user_log_message: String,
        raw_message: Option<String>,
        raw_message_safe: Option<String>,
        link: Option<Url>,
        hint_message: Option<String>,
    ) -> EngineError {
        EngineError::new(
            event_details,
            Tag::Unknown,
            qovery_log_message,
            user_log_message,
            raw_message,
            raw_message_safe,
            link,
            hint_message,
        )
    }

    /// Creates new error for unsupported instance type.
    ///
    /// Cloud provider doesn't support the requested instance type.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `requested_instance_type`: Raw requested instance type string.
    /// * `raw_message`: Error raw message such as command input / output which may contains unsafe text such as plain passwords / tokens.
    pub fn new_unsupported_instance_type(
        event_details: EventDetails,
        requested_instance_type: &str,
        raw_message: String,
    ) -> EngineError {
        let message = format!("`{}` instance type is not supported", requested_instance_type);
        EngineError::new(
            event_details,
            Tag::UnsupportedInstanceType,
            message.to_string(),
            message,
            Some(raw_message.clone()),
            Some(raw_message),
            None, // TODO(documentation): Create a page entry to details this error
            Some("Selected instance type is not supported, please check provider's documentation.".to_string()),
        )
    }
}
