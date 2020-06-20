pub mod aws;
pub mod gcp;

pub trait CloudProvider<'a> {
    fn name(&self) -> &'a str;
    fn region(&self) -> &'a str;
    fn is_valid(&self) -> bool;
    fn on_create(&self);
    fn kubernetes(self) -> Box<dyn Kubernetes>;
    fn services(&self) -> Vec<Box<dyn Service>>;
    fn create_service(&self, service: Box<dyn StatefulService>);
}

pub trait StatefulService: Service + Create {}

pub trait Service {
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn environment_type(&self) -> EnvironmentType;
}

pub enum EnvironmentType {
    Production,
    Development,
}

pub trait Create {
    fn on_create(&self, target: Box<dyn CloudProvider>);
    fn on_create_error(&self, target: Box<dyn CloudProvider>);
}

pub trait Delete {
    fn on_delete(&self, target: Box<dyn CloudProvider>);
}

pub trait Snapshot {
    fn on_snapshot(&self, target: Box<dyn CloudProvider>);
}

pub trait Clone {
    fn on_clone(&self, target: Box<dyn CloudProvider>);
}

pub trait Upgrade {
    fn on_upgrade(&self, target: Box<dyn CloudProvider>);
}

pub trait Downgrade {
    fn on_downgrade(&self, target: Box<dyn CloudProvider>);
}

pub struct DatabaseOptions<'a> {
    login: &'a str,
    password: &'a str,
    host: &'a str,
    port: u16,
    // TODO add others fields
}

pub enum DatabaseType<'a> {
    PostgreSQL(&'a DatabaseOptions<'a>),
    MongoDB(&'a DatabaseOptions<'a>),
    MySQL(&'a DatabaseOptions<'a>),
}

pub enum ServiceType<'a> {
    Application,
    Database(DatabaseType<'a>),
}

pub trait Kubernetes {
    fn name(&self) -> &str;
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn on_create(&self);
    fn on_upgrade(&self);
    fn on_downgrade(&self);
    fn on_delete(&self);
    fn create_namespace(&self);
    fn services(&self) -> &Vec<Box<dyn Service>>;
}

pub fn do_launch_workflow<'a, T: CloudProvider<'a>>(cp: &T) {
    cp.on_create();
}
