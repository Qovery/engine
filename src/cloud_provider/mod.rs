pub mod aws;
pub mod gcp;

pub trait CloudProvider<'a, K>
where
    K: Kubernetes,
{
    fn name(&self) -> &'a str;
    fn region(&self) -> &'a str;
    fn is_valid(&self) -> bool;
    fn on_create(&self);
    fn kubernetes(&self) -> &K;
    fn services(&self) -> Vec<Box<dyn Service>>;
    fn create_service(&self, service: Box<dyn StatefulService<K>>);
}

pub trait StatefulService<K: Kubernetes>: Service + Create<K> {}

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

pub trait Create<K>
where
    K: Kubernetes,
{
    fn on_create(&self, target: Box<dyn CloudProvider<K>>);
    fn on_create_error(&self, target: Box<dyn CloudProvider<K>>);
}

pub trait Delete<K>
where
    K: Kubernetes,
{
    fn on_delete(&self, target: Box<dyn CloudProvider<K>>);
}

pub trait Snapshot<K>
where
    K: Kubernetes,
{
    fn on_snapshot(&self, target: Box<dyn CloudProvider<K>>);
}

pub trait Clone<K>
where
    K: Kubernetes,
{
    fn on_clone(&self, target: Box<dyn CloudProvider<K>>);
}

pub trait Upgrade<K>
where
    K: Kubernetes,
{
    fn on_upgrade(&self, target: Box<dyn CloudProvider<K>>);
}

pub trait Downgrade<K>
where
    K: Kubernetes,
{
    fn on_downgrade(&self, target: Box<dyn CloudProvider<K>>);
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
    fn new() -> Self;
    fn name(&self) -> &str;
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn on_create(&self);
    fn on_upgrade(&self);
    fn on_downgrade(&self);
    fn on_delete(&self);
    fn create_namespace(&self);
    fn services(&self) -> &Vec<Box<dyn Service>>;
    fn create_service(&self, service: Box<dyn StatefulService<Self>>);
}

pub fn do_launch_workflow<'a, K: Kubernetes, T: CloudProvider<'a, K>>(cp: &T) {
    cp.on_create();
}
