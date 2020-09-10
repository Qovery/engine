use std::hash::Hash;
use std::rc::Rc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::build_platform::{Build, BuildOptions, GitRepository, Image};
use crate::cloud_provider::aws::databases::PostgreSQL;
use crate::cloud_provider::service::{DatabaseOptions, StatefulService, StatelessService};
use crate::cloud_provider::CloudProvider;
use crate::cloud_provider::Kind as CPKind;
use crate::git::Credentials;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum EnvironmentAction {
    Environment(TargetEnvironment),
    EnvironmentWithFailover(TargetEnvironment, FailoverEnvironment),
}

pub type TargetEnvironment = Environment;
pub type FailoverEnvironment = Environment;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Environment {
    pub execution_id: String,
    pub id: String,
    pub kind: Kind,
    pub owner_id: String,
    pub project_id: String,
    pub organization_id: String,
    pub action: Action,
    pub applications: Vec<Application>,
    pub routers: Vec<Router>,
    pub databases: Vec<Database>,
    pub clone_from_environment_id: Option<String>,
}

impl Environment {
    pub fn is_valid(&self) -> Result<(), EnvironmentError> {
        Ok(())
    }

    pub fn to_qe_environment(
        &self,
        context: &Context,
        built_applications: &Vec<Box<dyn crate::cloud_provider::service::Application>>,
        cloud_provider: &dyn CloudProvider,
    ) -> crate::cloud_provider::environment::Environment {
        let applications = self
            .applications
            .iter()
            .map(|x| {
                x.to_stateless_service(
                    context,
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
            .map(|x| x.to_stateless_service(context, cloud_provider))
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();

        let mut stateless_services = routers;
        stateless_services.extend(applications);

        let databases = self
            .databases
            .iter()
            .map(|x| x.to_stateful_service(context, cloud_provider))
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();

        let stateful_services = databases;

        crate::cloud_provider::environment::Environment::new(
            match self.kind {
                Kind::Production => crate::cloud_provider::environment::Kind::Production,
                Kind::Development => crate::cloud_provider::environment::Kind::Development,
            },
            self.id.as_str(),
            self.project_id.as_str(),
            self.owner_id.as_str(),
            self.organization_id.as_str(),
            stateless_services,
            stateful_services,
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Production,
    Development,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Action {
    Create,
    Pause,
    Delete,
    Nothing,
}

impl Action {
    pub fn to_service_action(&self) -> crate::cloud_provider::service::Action {
        match self {
            Action::Create => crate::cloud_provider::service::Action::Create,
            Action::Pause => crate::cloud_provider::service::Action::Pause,
            Action::Delete => crate::cloud_provider::service::Action::Delete,
            Action::Nothing => crate::cloud_provider::service::Action::Nothing,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub action: Action,
    pub git_url: String,
    pub git_credentials: GitCredentials,
    pub branch: String,
    pub commit_id: String,
    pub dockerfile_path: String,
    pub private_port: Option<u16>,
    pub total_cpus: String,
    pub total_ram_in_mib: u32,
    pub total_instances: u16,
    pub storage: Vec<Storage>,
    pub environment_variables: Vec<EnvironmentVariable>,
}

impl Application {
    pub fn to_application<'a>(
        &self,
        context: &Context,
        image: &Image,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<(dyn crate::cloud_provider::service::Application)>> {
        match cloud_provider.kind() {
            CPKind::AWS => Some(Box::new(
                crate::cloud_provider::aws::application::Application::new(
                    context.clone(),
                    self.id.as_str(),
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.private_port,
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.total_instances,
                    image.clone(),
                    self.storage
                        .iter()
                        .map(|s| s.to_aws_storage())
                        .collect::<Vec<_>>(),
                    self.environment_variables
                        .iter()
                        .map(|ev| ev.to_aws_environment_variable())
                        .collect::<Vec<_>>(),
                ),
            )),
            CPKind::GCP => None,
            _ => None,
            //TODO to implement
        }
    }

    pub fn to_stateless_service(
        &self,
        context: &Context,
        image: &Image,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<dyn StatelessService>> {
        match cloud_provider.kind() {
            CPKind::AWS => Some(Box::new(
                crate::cloud_provider::aws::application::Application::new(
                    context.clone(),
                    self.id.as_str(),
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.private_port,
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.total_instances,
                    image.clone(),
                    self.storage
                        .iter()
                        .map(|s| s.to_aws_storage())
                        .collect::<Vec<_>>(),
                    self.environment_variables
                        .iter()
                        .map(|ev| ev.to_aws_environment_variable())
                        .collect::<Vec<_>>(),
                ),
            )),
            CPKind::GCP => None,
            _ => None,
            //TODO to implement
        }
    }

    pub fn to_build(&self) -> Build {
        Build {
            git_repository: GitRepository {
                url: self.git_url.clone(),
                credentials: Some(Credentials {
                    login: self.git_credentials.login.clone(),
                    password: self.git_credentials.access_token.clone(),
                }),
                commit_id: self.commit_id.clone(),
                dockerfile_path: ".".to_string(),
            },
            image: Image {
                name: self.name.clone(),
                tag: self.commit_id.clone(),
                commit_id: self.commit_id.clone(),
                registry_url: None,
            },
            options: BuildOptions {
                environment_variables: self
                    .environment_variables
                    .iter()
                    .map(|ev| crate::build_platform::EnvironmentVariable {
                        key: ev.key.clone(),
                        value: ev.value.clone(),
                    })
                    .collect::<Vec<_>>(),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

impl EnvironmentVariable {
    pub fn to_aws_environment_variable(
        &self,
    ) -> crate::cloud_provider::aws::application::EnvironmentVariable {
        crate::cloud_provider::aws::application::EnvironmentVariable {
            key: self.key.clone(),
            value: self.value.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct GitCredentials {
    pub login: String,
    pub access_token: String,
    pub expired_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Storage {
    pub id: String,
    pub name: String,
    pub storage_type: StorageType,
    pub size_in_gib: u16,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageType {
    SlowHdd,
    Hdd,
    Ssd,
    FastSsd,
}

impl Storage {
    pub fn to_aws_storage(&self) -> crate::cloud_provider::aws::application::Storage {
        crate::cloud_provider::aws::application::Storage {
            id: self.id.clone(),
            name: self.name.clone(),
            storage_type: match self.storage_type {
                StorageType::SlowHdd => crate::cloud_provider::aws::application::StorageType::SC1,
                StorageType::Hdd => crate::cloud_provider::aws::application::StorageType::ST1,
                StorageType::Ssd => crate::cloud_provider::aws::application::StorageType::GP2,
                StorageType::FastSsd => crate::cloud_provider::aws::application::StorageType::IO1,
            },
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Router {
    pub id: String,
    pub name: String,
    pub action: Action,
    pub default_domain: String,
    pub public_port: u16,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

impl Router {
    pub fn to_stateless_service(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<dyn StatelessService>> {
        match cloud_provider.kind() {
            CPKind::AWS => {
                let router: Box<dyn StatelessService> =
                    Box::new(crate::cloud_provider::aws::router::Router::new(
                        context.clone(),
                        self.id.as_str(),
                        self.name.as_str(),
                        self.default_domain.as_str(),
                        self.custom_domains
                            .iter()
                            .map(|x| crate::cloud_provider::aws::router::CustomDomain {
                                domain: x.domain.clone(),
                                target_domain: x.target_domain.clone(),
                            })
                            .collect::<Vec<_>>(),
                        self.routes
                            .iter()
                            .map(|x| crate::cloud_provider::aws::router::Route {
                                path: x.path.clone(),
                                application_name: x.application_name.clone(),
                            })
                            .collect::<Vec<_>>(),
                    ));
                Some(router)
            }
            CPKind::GCP => None,
            _ => None,
            //TODO to implement
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct CustomDomain {
    pub domain: String,
    pub target_domain: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Route {
    pub path: String,
    pub application_name: String,
}

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
    pub total_cpus: String,
    pub total_ram_in_mib: u32,
    pub disk_size_in_gib: u32,
}

impl Database {
    pub fn to_stateful_service(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<dyn StatefulService>> {
        match cloud_provider.kind() {
            CPKind::AWS => match self.kind {
                DatabaseKind::Postgresql => {
                    let db: Box<dyn StatefulService> = Box::new(PostgreSQL::new(
                        context.clone(),
                        self.id.as_str(),
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.version.as_str(),
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        DatabaseOptions {
                            login: self.username.clone(),
                            password: self.password.clone(),
                            host: self.fqdn.clone(),
                            port: self.port,
                            disk_size_in_gib: self.disk_size_in_gib,
                        },
                    ));

                    Some(db)
                }
                _ => None,
            },
            CPKind::GCP => None,
            _ => None,
            //TODO to implement
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatabaseKind {
    Postgresql,
    Mysql,
    Mongodb,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentError {}

#[derive(Clone)]
pub struct ProgressInfo {
    pub created_at: DateTime<Utc>,
    pub step: ProgressStep,
    pub level: ProgressLevel,
    pub message: String,
    pub execution_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProgressStep {
    WaitingToRun,
    BootstrapInfrastructure,
    CreateKubernetes,
    BuildApplication,
    DeployEnvironment,
    PauseEnvironment,
    DeleteEnvironment,
    DeleteKubernetes,
    DeleteInfrastructure,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProgressLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl ProgressInfo {
    pub fn new<T: Into<String>, X: Into<String>>(
        step: ProgressStep,
        level: ProgressLevel,
        message: T,
        execution_id: X,
    ) -> Self {
        ProgressInfo {
            created_at: Utc::now(),
            step,
            level,
            message: message.into(),
            execution_id: execution_id.into(),
        }
    }
}

pub trait ProgressListener {
    fn on_progress(&self, info: ProgressInfo);
    fn on_complete(&self, info: ProgressInfo);
    fn on_error(&self, info: ProgressInfo);
}

pub type Listeners = Vec<Rc<Box<dyn ProgressListener>>>;

pub struct ListenersHelper<'a> {
    listeners: &'a Listeners,
}

impl<'a> ListenersHelper<'a> {
    pub fn new(listeners: &'a Listeners) -> Self {
        ListenersHelper { listeners }
    }

    pub fn on_progress(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.on_progress(info.clone()));
    }

    pub fn on_complete(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.on_complete(info.clone()));
    }

    pub fn on_error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.on_error(info.clone()));
    }
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct Context {
    execution_id: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<String>,
}

impl Context {
    pub fn new(
        execution_id: &str,
        workspace_root_dir: &str,
        lib_root_dir: &str,
        docker_host: Option<String>,
    ) -> Self {
        Context {
            execution_id: execution_id.to_string(),
            workspace_root_dir: workspace_root_dir.to_string(),
            lib_root_dir: lib_root_dir.to_string(),
            docker_host,
        }
    }

    pub fn execution_id(&self) -> &str {
        self.execution_id.as_str()
    }

    pub fn workspace_root_dir(&self) -> &str {
        self.workspace_root_dir.as_str()
    }

    pub fn lib_root_dir(&self) -> &str {
        self.lib_root_dir.as_str()
    }

    pub fn docker_tcp_socket(&self) -> &Option<String> {
        &self.docker_host
    }
}
