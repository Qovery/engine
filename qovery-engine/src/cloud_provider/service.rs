use crate::build_platform::Image;
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use std::io::Error;

pub trait StatelessService: Service + Create + Delete {}

pub trait StatefulService:
    Service + Create + Delete + Backup + Clone + Upgrade + Downgrade
{
}

pub trait Service {
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn is_valid(&self) -> Result<(), ServiceError>;
}

pub trait Create {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
}

pub trait Delete {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
}

pub trait Backup {
    fn on_backup(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
    fn on_backup_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
    fn on_restore(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
    fn on_restore_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
}

pub trait Clone {
    fn on_clone(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
    fn on_clone_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
}

pub trait Upgrade {
    fn on_upgrade(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
    fn on_upgrade_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
}

pub trait Downgrade {
    fn on_downgrade(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
    fn on_downgrade_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError>;
}

pub struct DatabaseOptions {
    pub login: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    // TODO add others fields
}

pub enum DatabaseType<'a> {
    PostgreSQL(&'a DatabaseOptions),
    MongoDB(&'a DatabaseOptions),
    MySQL(&'a DatabaseOptions),
}

pub enum ServiceType<'a> {
    Application,
    Database(DatabaseType<'a>),
    Router,
}

#[derive(Debug)]
pub enum ServiceError {
    Error(Error),
}

impl From<std::io::Error> for ServiceError {
    fn from(err: Error) -> Self {
        ServiceError::Error(err)
    }
}
