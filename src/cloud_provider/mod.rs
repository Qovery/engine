use crate::cloud_provider::error::{CloudProviderError, KubernetesError};

pub mod aws;
pub mod error;
pub mod gcp;

pub trait CloudProvider {
    fn name(&self) -> CloudProviderName;
    fn is_valid(&self) -> Result<(), CloudProviderError>;
    fn kubernetes_clusters(self) -> Result<Vec<Box<dyn Kubernetes>>, CloudProviderError>;
}

pub trait StatefulService: Service + Create + Delete {}

pub trait Service {
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn environment_type(&self) -> EnvironmentType;
}

pub enum CloudProviderName {
    AWS,
    GCP,
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
    fn on_delete_error(&self, target: Box<dyn CloudProvider>);
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
    fn version(&self) -> &str;
    fn region(&self) -> &str;
    fn is_valid(&self) -> Result<(), KubernetesError>;
    fn on_create(&self) -> Result<(), KubernetesError>;
    fn on_create_error(&self) -> Result<(), KubernetesError>;
    fn on_upgrade(&self) -> Result<(), KubernetesError>;
    fn on_upgrade_error(&self) -> Result<(), KubernetesError>;
    fn on_downgrade(&self) -> Result<(), KubernetesError>;
    fn on_downgrade_error(&self) -> Result<(), KubernetesError>;
    fn on_delete(&self) -> Result<(), KubernetesError>;
    fn on_delete_error(&self) -> Result<(), KubernetesError>;
    fn create_namespace(&self) -> Result<(), KubernetesError>;
    fn services(&self) -> Result<Vec<Box<dyn Service>>, KubernetesError>;
    fn create_service(&self, service: Box<dyn StatefulService>) -> Result<(), KubernetesError>;
}
