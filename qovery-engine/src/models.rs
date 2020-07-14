use std::hash::Hash;
use std::rc::Rc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cloud_provider::aws::databases::PostgreSQL;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{DatabaseOptions, Service};
use crate::cloud_provider::Kind as CPKind;
use crate::cloud_provider::{CloudProvider as CP, CloudProvider};

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Environment {
    pub owner_id: String,
    pub project_id: String,
    pub environment_id: String,
    pub action: Action,
    pub applications: Vec<Application>,
    pub routers: Vec<Router>,
    pub databases: Vec<Database>,
}

impl Environment {
    pub fn is_valid(&self) -> Result<(), EnvironmentError> {
        Ok(())
    }

    pub fn as_qovery_engine_environment<C, K>(
        &self,
    ) -> crate::cloud_provider::environment::Environment<C, K>
    where
        C: CloudProvider,
        K: Kubernetes<C>,
    {
        crate::cloud_provider::environment::Environment::new(
            self.environment_id.as_str(),
            self.project_id.as_str(),
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum Action {
    Create,
    Pause,
    Delete,
    Idle,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub git_url: String,
    pub commit_id: String,
    pub action: Action,
    pub git_credentials: GitCredentials,
    pub storage: Vec<Storage>,
    pub environment_variables: Vec<EnvironmentVariable>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct GitCredentials {
    pub login: String,
    pub access_token: String,
    pub expired_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Storage {}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Router {
    pub id: String,
    pub name: String,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct CustomDomain {}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Route {}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Database {
    pub kind: String,
    pub action: Action,
    pub id: String,
    pub name: String,
    pub version: String,
    pub fqdn_id: String,
    pub fqdn: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub disk_size_in_mb: u32,
    pub host_provider: String,
    pub status_url: String,
    pub snapshot: Option<Snapshot>,
}

impl Database {
    pub fn to_service(&self, cloud_provider: &dyn CP) -> Option<Box<dyn Service>> {
        match cloud_provider.kind() {
            CPKind::AWS => match self.kind.to_lowercase().as_str() {
                "postgresql" => Some(Box::new(PostgreSQL {
                    id: self.id.clone(),
                    name: self.name.clone(),
                    version: self.version.clone(),
                    options: DatabaseOptions {
                        login: self.username.clone(),
                        password: self.password.clone(),
                        host: self.fqdn.clone(),
                        port: self.port.clone(),
                    },
                })),
                _ => None,
            },
            CPKind::GCP => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Snapshot {}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub enum EnvironmentError {}

pub struct ProgressInfo {
    pub step_name: String,
    pub percent: u8,
    pub message: String,
}

impl ProgressInfo {
    pub fn new(step_name: &str, percent: u8, message: &str) -> Self {
        ProgressInfo {
            step_name: step_name.to_string(),
            percent,
            message: message.to_string(),
        }
    }
}

pub trait ProgressListener {
    fn on_progress(&self, info: ProgressInfo);
    fn on_complete(&self, info: ProgressInfo);
    fn on_error(&self, info: ProgressInfo);
}

pub type Listeners = Vec<Rc<Box<dyn ProgressListener>>>;
