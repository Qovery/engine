use crate::errors;
use crate::events::io::EventDetails;
use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct SimpleError {
    message: String,
    message_safe: String,
}

impl From<errors::SimpleError> for SimpleError {
    fn from(error: errors::SimpleError) -> Self {
        SimpleError {
            message: error.message,
            message_safe: error.message_safe,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Tag {
    Unknown,
    UnsupportedInstanceType,
}

impl From<errors::Tag> for Tag {
    fn from(tag: errors::Tag) -> Self {
        match tag {
            errors::Tag::Unknown => Tag::Unknown,
            errors::Tag::UnsupportedInstanceType => Tag::UnsupportedInstanceType,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct EngineError {
    tag: Tag,
    event_details: EventDetails,
    qovery_log_message: String,
    user_log_message: String,
    raw_message: Option<String>,
    raw_message_safe: Option<String>,
    link: Option<String>,
    hint_message: Option<String>,
}

impl From<errors::EngineError> for EngineError {
    fn from(error: errors::EngineError) -> Self {
        EngineError {
            tag: Tag::from(error.tag),
            event_details: EventDetails::from(error.event_details),
            qovery_log_message: error.qovery_log_message,
            user_log_message: error.user_log_message,
            raw_message: error.raw_message,
            raw_message_safe: error.raw_message_safe,
            link: error.link.map(|url| url.to_string()),
            hint_message: error.hint_message,
        }
    }
}
