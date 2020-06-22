use chrono::{DateTime, Utc};

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

pub struct Deployment {
    pub id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Eq, PartialEq)]
pub enum Action {
    Create,
    Pause,
    Delete,
    Idle,
}

pub struct CloudProvider {
    pub name: String,
    pub region: String,
}

#[derive(Clone)]
pub struct Application {
    pub name: String,
    pub git_url: String,
    pub commit_id: String,
    pub action: Action,
    pub git_credentials: GitCredentials,
    pub storage: Vec<Storage>,
}

#[derive(Clone)]
pub struct GitCredentials {
    pub login: String,
    pub access_token: String,
    pub expired_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Storage {}

pub struct Router {
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

pub struct CustomDomain {}

pub struct Route {}

pub struct Database {
    pub snapshot: Snapshot,
}

pub struct Snapshot {}

#[derive(Debug)]
pub enum EnvironmentError {}
