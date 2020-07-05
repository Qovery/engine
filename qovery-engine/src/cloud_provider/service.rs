use crate::build_platform::Image;
use crate::cloud_provider::CloudProvider;

pub trait StatefulService<'a>: Service + Create<'a> + Delete<'a> {}

pub trait StatelessService<'a>: Service + Create<'a> + Delete<'a> {}

pub trait Service {
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn is_valid(&self) -> Result<(), ServiceError>;
    fn image(&self) -> &Image;
    fn environment_type(&self) -> EnvironmentType;
}

pub enum EnvironmentType {
    Production,
    Development,
}

pub trait Create<'a> {
    fn on_create(&self, target: &'a dyn CloudProvider);
    fn on_create_error(&self, target: &'a dyn CloudProvider);
}

pub trait Delete<'a> {
    fn on_delete(&self, target: &'a dyn CloudProvider);
    fn on_delete_error(&self, target: &'a dyn CloudProvider);
}

pub trait Snapshot<'a> {
    fn on_snapshot(&self, target: &'a dyn CloudProvider);
}

pub trait Clone<'a> {
    fn on_clone(&self, target: &'a dyn CloudProvider);
}

pub trait Upgrade<'a> {
    fn on_upgrade(&self, target: &'a dyn CloudProvider);
}

pub trait Downgrade<'a> {
    fn on_downgrade(&self, target: &'a dyn CloudProvider);
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
}

#[derive(Debug)]
pub enum ServiceError {}
