use crate::build_platform::Image;
use crate::cloud_provider::error::{CloudProviderError, KubernetesError, ServiceError};

pub mod application;
pub mod aws;
pub mod error;
pub mod gcp;

pub trait CloudProvider {
    fn name(&self) -> CloudProviderName;
    fn is_valid(&self) -> Result<(), CloudProviderError>;
    fn kubernetes_clusters(self) -> Result<Vec<Box<dyn Kubernetes>>, CloudProviderError>;
}

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

pub enum CloudProviderName {
    AWS,
    GCP,
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
    fn create_service(&self, service: &dyn Service) -> Result<(), KubernetesError>;
    fn delete_service(&self, service: &dyn Service) -> Result<(), KubernetesError>;
}
