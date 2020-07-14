use crate::build_platform::Image;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd::CmdError;
use std::io::Error;

pub trait StatelessService<C, K>: Service + Create<C, K> + Delete<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
}

pub trait StatefulService<C, K>:
    Service + Create<C, K> + Delete<C, K> + Backup<C, K> + Clone<C, K> + Upgrade<C, K> + Downgrade<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
}

pub trait Service {
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn is_valid(&self) -> Result<(), ServiceError>;
}

pub trait Create<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
    fn on_create(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
    fn on_create_error(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
}

pub trait Delete<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
    fn on_delete(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
    fn on_delete_error(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
}

pub trait Backup<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
    fn on_backup(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
    fn on_backup_error(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
    fn on_restore(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
    fn on_restore_error(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
}

pub trait Clone<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
    fn on_clone(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
    fn on_clone_error(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
}

pub trait Upgrade<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
    fn on_upgrade(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
    fn on_upgrade_error(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
}

pub trait Downgrade<C, K>
where
    C: CloudProvider,
    K: Kubernetes<C>,
{
    fn on_downgrade(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
    fn on_downgrade_error(&self, target: &DeploymentTarget<C, K>) -> Result<(), ServiceError>;
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
    Cmd(CmdError),
    Error(Error),
}

impl From<std::io::Error> for ServiceError {
    fn from(err: Error) -> Self {
        ServiceError::Error(err)
    }
}

impl From<CmdError> for ServiceError {
    fn from(err: CmdError) -> Self {
        ServiceError::Cmd(err)
    }
}
