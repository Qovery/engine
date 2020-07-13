use crate::build_platform::Image;
use crate::cloud_provider::CloudProvider;

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
    fn environment_type(&self) -> EnvironmentType;
}

pub enum EnvironmentType {
    Production,
    Development,
}

pub trait Create {
    fn on_create(&self, target: &dyn CloudProvider);
    fn on_create_error(&self, target: &dyn CloudProvider);
}

pub trait Delete {
    fn on_delete(&self, target: &dyn CloudProvider);
    fn on_delete_error(&self, target: &dyn CloudProvider);
}

pub trait Backup {
    fn on_backup(&self, target: &dyn CloudProvider);
    fn on_backup_error(&self, target: &dyn CloudProvider);
    fn on_restore(&self, target: &dyn CloudProvider);
    fn on_restore_error(&self, target: &dyn CloudProvider);
}

pub trait Clone {
    fn on_clone(&self, target: &dyn CloudProvider);
    fn on_clone_error(&self, target: &dyn CloudProvider);
}

pub trait Upgrade {
    fn on_upgrade(&self, target: &dyn CloudProvider);
    fn on_upgrade_error(&self, target: &dyn CloudProvider);
}

pub trait Downgrade {
    fn on_downgrade(&self, target: &dyn CloudProvider);
    fn on_downgrade_error(&self, target: &dyn CloudProvider);
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
pub enum ServiceError {}
