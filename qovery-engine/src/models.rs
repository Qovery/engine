use std::hash::Hash;
use std::rc::Rc;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use rusoto_core::Region;
use serde::{Deserialize, Serialize};

use crate::build_platform::Image;
use crate::cloud_provider::aws::databases::PostgreSQL;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::environment::Kind;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{DatabaseOptions, Service, StatefulService, StatelessService};
use crate::cloud_provider::Kind as CPKind;
use crate::cloud_provider::{CloudProvider as CP, CloudProvider};

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Environment {
    pub owner_id: String,
    pub project_id: String,
    pub environment_id: String,
    pub action: Action,
    pub is_production: bool,
    pub applications: Vec<Application>,
    pub routers: Vec<Router>,
    pub databases: Vec<Database>,
}

impl Environment {
    pub fn is_valid(&self) -> Result<(), EnvironmentError> {
        Ok(())
    }

    pub fn to_qe_environment(
        &self,
        built_applications: &Vec<Box<dyn crate::cloud_provider::service::Application>>,
        cloud_provider: &dyn CloudProvider,
    ) -> crate::cloud_provider::environment::Environment {
        let applications = self
            .applications
            .iter()
            .map(|x| {
                x.to_stateless_service(
                    built_applications
                        .iter()
                        .find(|y| x.id.as_str() == y.id())
                        .unwrap()
                        .image(), // FIXME not safe
                    cloud_provider,
                )
            })
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();

        let routers = self
            .routers
            .iter()
            .map(|x| x.to_stateless_service(cloud_provider))
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();

        let mut stateless_services = routers;
        stateless_services.extend(applications);

        let databases = self
            .databases
            .iter()
            .map(|x| x.to_stateful_service(cloud_provider))
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();

        let stateful_services = databases;

        crate::cloud_provider::environment::Environment::new(
            match self.is_production {
                true => Kind::Production,
                false => Kind::Development,
            },
            self.environment_id.as_str(),
            self.project_id.as_str(),
            self.owner_id.as_str(),
            stateless_services,
            stateful_services,
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum Action {
    Create,
    Pause,
    Delete,
    Idle,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
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

impl Application {
    pub fn to_stateless_service(
        &self,
        image: &Image,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<dyn StatelessService>> {
        match cloud_provider.kind() {
            CPKind::AWS => Some(Box::new(
                crate::cloud_provider::aws::application::Application::new(
                    self.id.as_str(),
                    self.name.as_str(),
                    image.clone(),
                ),
            )),
            CPKind::GCP => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct GitCredentials {
    pub login: String,
    pub access_token: String,
    pub expired_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Storage {}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Router {
    pub id: String,
    pub name: String,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

impl Router {
    pub fn to_stateless_service(
        &self,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<dyn StatelessService>> {
        match cloud_provider.kind() {
            CPKind::AWS => None,
            CPKind::GCP => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct CustomDomain {}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Route {}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Database {
    pub kind: DatabaseKind,
    pub action: Action,
    pub id: String,
    pub name: String,
    pub version: String,
    pub fqdn_id: String,
    pub fqdn: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub disk_size_in_mb: u32,
    pub host_provider: String,
    pub status_url: String,
    pub snapshot: Option<Snapshot>,
}

impl Database {
    pub fn to_stateful_service(
        &self,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<dyn StatefulService>> {
        match cloud_provider.kind() {
            CPKind::AWS => match self.kind {
                DatabaseKind::PostgreSQL => {
                    let db: Box<dyn StatefulService> = Box::new(PostgreSQL::new(
                        self.id.as_str(),
                        self.name.as_str(),
                        self.version.as_str(),
                        DatabaseOptions {
                            login: self.username.clone(),
                            password: self.password.clone(),
                            host: self.fqdn.clone(),
                            port: self.port.clone(),
                        },
                    ));

                    Some(db)
                }
                _ => None,
            },
            CPKind::GCP => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum DatabaseKind {
    PostgreSQL,
    MySQL,
    MongoDB,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Snapshot {}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub enum EnvironmentError {}

pub struct ProgressInfo {
    pub step_name: String,
    pub percent: u8,
    pub message: String,
}

impl ProgressInfo {
    pub fn new(step_name: &str, percent: u8, message: &str) -> Self {
        ProgressInfo {
            step_name: step_name.to_string(),
            percent,
            message: message.to_string(),
        }
    }
}

pub trait ProgressListener {
    fn on_progress(&self, info: ProgressInfo);
    fn on_complete(&self, info: ProgressInfo);
    fn on_error(&self, info: ProgressInfo);
}

pub type Listeners = Vec<Rc<Box<dyn ProgressListener>>>;
