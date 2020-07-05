use crate::cloud_provider::aws::databases::PostgreSQL;
use crate::cloud_provider::service::{DatabaseOptions, Service};
use crate::cloud_provider::CloudProvider as CP;
use crate::cloud_provider::Kind as CPKind;
use chrono::{DateTime, Utc};
use std::hash::Hash;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Environment {
    pub deployment: Deployment,
    pub owner_id: String,
    pub project_id: String,
    pub environment_id: String,
    pub environment_type: String,
    pub action: Action,
    pub cloud_provider: CloudProvider,
    pub applications: Vec<Application>,
    pub routers: Vec<Router>,
    pub databases: Vec<Database>,
}

impl Environment {
    pub fn is_valid(&self) -> Result<(), EnvironmentError> {
        Ok(())
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Deployment {
    pub id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum Action {
    Create,
    Pause,
    Delete,
    Idle,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct CloudProvider {
    pub name: String,
    pub region: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
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

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct GitCredentials {
    pub login: String,
    pub access_token: String,
    pub expired_at: DateTime<Utc>,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Storage {}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Router {
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct CustomDomain {}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Route {}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Database {
    pub id: String,
    // TODO serde rename property
    pub _type: String,
    pub version: String,
    pub name: String,
    pub fqdn_id: String,
    pub fqdn: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub disk_size_in_mb: u32,
    pub host_provider: String,
    pub to_delete: bool,
    pub status_url: String,
    pub snapshot: Snapshot,
}

impl Database {
    pub fn to_service(&self, cloud_provider: &dyn CP) -> Option<Box<dyn Service>> {
        match cloud_provider.kind() {
            CPKind::AWS => match self._type.to_lowercase().as_str() {
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

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Snapshot {}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum EnvironmentError {}
