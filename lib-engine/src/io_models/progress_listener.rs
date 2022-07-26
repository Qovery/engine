use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct ProgressInfo {
    pub created_at: DateTime<Utc>,
    pub scope: ProgressScope,
    pub level: ProgressLevel,
    pub message: Option<String>,
    pub execution_id: String,
}

impl ProgressInfo {
    pub fn new<T: Into<String>, X: Into<String>>(
        scope: ProgressScope,
        level: ProgressLevel,
        message: Option<T>,
        execution_id: X,
    ) -> Self {
        ProgressInfo {
            created_at: Utc::now(),
            scope,
            level,
            message: message.map(|msg| msg.into()),
            execution_id: execution_id.into(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProgressScope {
    Queued,
    Infrastructure { execution_id: String },
    Database { id: String },
    Application { id: String },
    Router { id: String },
    Environment { id: String },
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProgressLevel {
    Debug,
    Info,
    Warn,
    Error,
}

pub trait ProgressListener: Send + Sync {
    fn deployment_in_progress(&self, info: ProgressInfo);
    fn pause_in_progress(&self, info: ProgressInfo);
    fn delete_in_progress(&self, info: ProgressInfo);
    fn error(&self, info: ProgressInfo);
    fn deployed(&self, info: ProgressInfo);
    fn paused(&self, info: ProgressInfo);
    fn deleted(&self, info: ProgressInfo);
    fn deployment_error(&self, info: ProgressInfo);
    fn pause_error(&self, info: ProgressInfo);
    fn delete_error(&self, info: ProgressInfo);
}

pub struct NoOpProgressListener {}

impl ProgressListener for NoOpProgressListener {
    fn deployment_in_progress(&self, _info: ProgressInfo) {}
    fn pause_in_progress(&self, _info: ProgressInfo) {}
    fn delete_in_progress(&self, _info: ProgressInfo) {}
    fn error(&self, _info: ProgressInfo) {}
    fn deployed(&self, _info: ProgressInfo) {}
    fn paused(&self, _info: ProgressInfo) {}
    fn deleted(&self, _info: ProgressInfo) {}
    fn deployment_error(&self, _info: ProgressInfo) {}
    fn pause_error(&self, _info: ProgressInfo) {}
    fn delete_error(&self, _info: ProgressInfo) {}
}

pub type Listener = Arc<Box<dyn ProgressListener>>;
pub type Listeners = Vec<Listener>;

pub struct ListenersHelper<'a> {
    listeners: &'a Listeners,
}

impl<'a> ListenersHelper<'a> {
    pub fn new(listeners: &'a Listeners) -> Self {
        ListenersHelper { listeners }
    }

    pub fn deployment_in_progress(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.deployment_in_progress(info.clone()));
    }

    pub fn upgrade_in_progress(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.deployment_in_progress(info.clone()));
    }

    pub fn pause_in_progress(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.pause_in_progress(info.clone()));
    }

    pub fn delete_in_progress(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.delete_in_progress(info.clone()));
    }

    pub fn error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.error(info.clone()));
    }

    pub fn deployed(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.deployed(info.clone()));
    }

    pub fn paused(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.paused(info.clone()));
    }

    pub fn deleted(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.deleted(info.clone()));
    }

    pub fn deployment_error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.deployment_error(info.clone()));
    }

    pub fn pause_error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.pause_error(info.clone()));
    }

    pub fn delete_error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.delete_error(info.clone()));
    }
}
