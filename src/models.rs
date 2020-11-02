use std::hash::Hash;
use std::rc::Rc;

use chrono::{DateTime, Utc};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};

use crate::build_platform::{Build, BuildOptions, GitRepository, Image};
use crate::cloud_provider::aws::databases::{MongoDB, MySQL, PostgreSQL};
use crate::cloud_provider::service::{DatabaseOptions, StatefulService, StatelessService};
use crate::cloud_provider::CloudProvider;
use crate::cloud_provider::Kind as CPKind;
use crate::git::Credentials;
use crate::models::DatabaseKind::Mongodb;

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
    pub external_services: Vec<ExternalService>,
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
        let external_services = self
            .external_services
            .iter()
            .map(
                |x| match built_applications.iter().find(|y| x.id.as_str() == y.id()) {
                    Some(app) => {
                        x.to_stateless_service(context, app.image().clone(), cloud_provider)
                    }
                    _ => x.to_stateless_service(context, x.to_image(), cloud_provider),
                },
            )
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();

        let applications = self
            .applications
            .iter()
            .map(
                |x| match built_applications.iter().find(|y| x.id.as_str() == y.id()) {
                    Some(app) => {
                        x.to_stateless_service(context, app.image().clone(), cloud_provider)
                    }
                    _ => x.to_stateless_service(context, x.to_image(), cloud_provider),
                },
            )
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

        // orders is important, first external services, then applications and then routers.
        let mut stateless_services = external_services;
        stateless_services.extend(applications);
        // routers are deployed lastly to avoid to be blacklisted if we request TLS certificates
        // while an app does not start for some reason.
        stateless_services.extend(routers);

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
    pub cpu_burst: String,
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
                    self.cpu_burst.clone(),
                    self.total_ram_in_mib,
                    self.total_instances,
                    image.clone(),
                    self.storage
                        .iter()
                        .map(|s| s.to_aws_storage())
                        .collect::<Vec<_>>(),
                    self.environment_variables
                        .iter()
                        .map(|ev| ev.to_aws_application_environment_variable())
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
        image: Image,
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
                    self.cpu_burst.clone(),
                    self.total_ram_in_mib,
                    self.total_instances,
                    image,
                    self.storage
                        .iter()
                        .map(|s| s.to_aws_storage())
                        .collect::<Vec<_>>(),
                    self.environment_variables
                        .iter()
                        .map(|ev| ev.to_aws_application_environment_variable())
                        .collect::<Vec<_>>(),
                ),
            )),
            CPKind::GCP => None,
            _ => None,
            //TODO to implement
        }
    }

    pub fn to_image(&self) -> Image {
        Image {
            application_id: self.id.clone(),
            name: self.name.clone(),
            tag: self.commit_id.clone(),
            commit_id: self.commit_id.clone(),
            registry_url: None,
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
                dockerfile_path: self.dockerfile_path.clone(),
            },
            image: self.to_image(),
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
    pub fn to_aws_application_environment_variable(
        &self,
    ) -> crate::cloud_provider::aws::application::EnvironmentVariable {
        crate::cloud_provider::aws::application::EnvironmentVariable {
            key: self.key.clone(),
            value: self.value.clone(),
        }
    }

    pub fn to_aws_external_service_environment_variable(
        &self,
    ) -> crate::cloud_provider::aws::external_service::EnvironmentVariable {
        crate::cloud_provider::aws::external_service::EnvironmentVariable {
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
    pub database_instance_type: String,
    pub database_disk_type: String,
}

impl Database {
    pub fn to_stateful_service(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<dyn StatefulService>> {
        let database_options = DatabaseOptions {
            login: self.username.clone(),
            password: self.password.clone(),
            host: self.fqdn.clone(),
            port: self.port,
            disk_size_in_gib: self.disk_size_in_gib,
            database_disk_type: self.database_disk_type.clone(),
        };

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
                        self.database_instance_type.as_str(),
                        database_options,
                    ));

                    Some(db)
                }
                DatabaseKind::Mysql => {
                    let db: Box<dyn StatefulService> = Box::new(MySQL::new(
                        context.clone(),
                        self.id.as_str(),
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.version.as_str(),
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options,
                    ));

                    Some(db)
                }
                DatabaseKind::Mongodb => {
                    let db: Box<dyn StatefulService> = Box::new(MongoDB::new(
                        context.clone(),
                        self.id.as_str(),
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.version.as_str(),
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options,
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

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct ExternalService {
    pub action: Action,
    pub id: String,
    pub name: String,
    pub total_cpus: String,
    pub total_ram_in_mib: u32,
    pub git_url: String,
    pub git_credentials: GitCredentials,
    pub branch: String,
    pub commit_id: String,
    pub on_create_dockerfile_path: String,
    pub on_pause_dockerfile_path: String,
    pub on_delete_dockerfile_path: String,
    pub environment_variables: Vec<EnvironmentVariable>,
}

impl ExternalService {
    pub fn to_application<'a>(
        &self,
        context: &Context,
        image: &Image,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<(dyn crate::cloud_provider::service::Application)>> {
        match cloud_provider.kind() {
            CPKind::AWS => Some(Box::new(
                crate::cloud_provider::aws::external_service::ExternalService::new(
                    context.clone(),
                    self.id.as_str(),
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    image.clone(),
                    self.environment_variables
                        .iter()
                        .map(|ev| ev.to_aws_external_service_environment_variable())
                        .collect::<Vec<_>>(),
                ),
            )),
            CPKind::GCP => None,
            _ => None,
            //TODO to implement
        }
    }

    pub fn to_stateless_service<'a>(
        &self,
        context: &Context,
        image: Image,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<(dyn crate::cloud_provider::service::StatelessService)>> {
        match cloud_provider.kind() {
            CPKind::AWS => Some(Box::new(
                crate::cloud_provider::aws::external_service::ExternalService::new(
                    context.clone(),
                    self.id.as_str(),
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    image,
                    self.environment_variables
                        .iter()
                        .map(|ev| ev.to_aws_external_service_environment_variable())
                        .collect::<Vec<_>>(),
                ),
            )),
            CPKind::GCP => None,
            _ => None,
            //TODO to implement
        }
    }

    pub fn to_image(&self) -> Image {
        Image {
            application_id: self.id.clone(),
            name: self.name.clone(),
            tag: self.commit_id.clone(),
            commit_id: self.commit_id.clone(),
            registry_url: None,
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
                dockerfile_path: match self.action {
                    Action::Create => self.on_create_dockerfile_path.clone(),
                    Action::Pause => self.on_pause_dockerfile_path.clone(),
                    Action::Delete => self.on_delete_dockerfile_path.clone(),
                    Action::Nothing => self.on_create_dockerfile_path.clone(),
                },
            },
            image: self.to_image(),
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

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentError {}

#[derive(Clone)]
pub struct ProgressInfo {
    pub created_at: DateTime<Utc>,
    pub scope: ProgressScope,
    pub level: ProgressLevel,
    pub message: Option<String>,
    pub execution_id: String,
}

impl ProgressInfo {
    pub fn new<T: Into<String>, X: Into<String>>(
        scope: ProgressScope,
        level: ProgressLevel,
        message: Option<T>,
        execution_id: X,
    ) -> Self {
        ProgressInfo {
            created_at: Utc::now(),
            scope,
            level,
            message: match message {
                Some(msg) => Some(msg.into()),
                _ => None,
            },
            execution_id: execution_id.into(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProgressScope {
    Queued,
    Infrastructure { execution_id: String },
    Database { id: String },
    Application { id: String },
    ExternalService { id: String },
    Router { id: String },
    Environment { id: String },
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProgressLevel {
    Debug,
    Info,
    Warn,
    Error,
}

pub trait ProgressListener {
    fn start_in_progress(&self, info: ProgressInfo);
    fn pause_in_progress(&self, info: ProgressInfo);
    fn delete_in_progress(&self, info: ProgressInfo);
    fn error(&self, info: ProgressInfo);
    fn started(&self, info: ProgressInfo);
    fn paused(&self, info: ProgressInfo);
    fn deleted(&self, info: ProgressInfo);
    fn start_error(&self, info: ProgressInfo);
    fn pause_error(&self, info: ProgressInfo);
    fn delete_error(&self, info: ProgressInfo);
}

pub type Listener = Rc<Box<dyn ProgressListener>>;
pub type Listeners = Vec<Listener>;

pub struct ListenersHelper<'a> {
    listeners: &'a Listeners,
}

impl<'a> ListenersHelper<'a> {
    pub fn new(listeners: &'a Listeners) -> Self {
        ListenersHelper { listeners }
    }

    pub fn start_in_progress(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.start_in_progress(info.clone()));
    }

    pub fn pause_in_progress(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.pause_in_progress(info.clone()));
    }

    pub fn delete_in_progress(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.delete_in_progress(info.clone()));
    }

    pub fn error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.error(info.clone()));
    }

    pub fn started(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.started(info.clone()));
    }

    pub fn paused(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.paused(info.clone()));
    }

    pub fn deleted(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.deleted(info.clone()));
    }

    pub fn start_error(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.start_error(info.clone()));
    }

    pub fn pause_error(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.pause_error(info.clone()));
    }

    pub fn delete_error(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.delete_error(info.clone()));
    }
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct Context {
    execution_id: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<String>,
    metadata: Option<Metadata>,
}

// trait used to reimplement clone without same fields
// this trait is used for Context struct
pub trait Clone2 {
    fn clone_not_same_execution_id(&self) -> Self;
}

// for test we need to clone context but to change the directory workspace used
// to to this we just have to suffix the execution id in tests
impl Clone2 for Context {
    fn clone_not_same_execution_id(&self) -> Context {
        let mut new = self.clone();
        let suffix = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .collect::<String>();
        new.execution_id = format!("{}-{}", self.execution_id, suffix);
        new
    }
}

impl Context {
    pub fn new(
        execution_id: &str,
        workspace_root_dir: &str,
        lib_root_dir: &str,
        docker_host: Option<String>,
        metadata: Option<Metadata>,
    ) -> Self {
        Context {
            execution_id: execution_id.to_string(),
            workspace_root_dir: workspace_root_dir.to_string(),
            lib_root_dir: lib_root_dir.to_string(),
            docker_host,
            metadata,
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

    pub fn docker_tcp_socket(&self) -> Option<&String> {
        self.docker_host.as_ref()
    }

    pub fn metadata(&self) -> Option<&Metadata> {
        self.metadata.as_ref()
    }
}

/// put everything you want here that is required to change the behaviour of the request.
/// E.g you can indicate that this request is a test, then you can adapt the behaviour as you want.
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Metadata {
    pub test: Option<bool>,
}

impl Metadata {
    pub fn new(test: Option<bool>) -> Self {
        Metadata { test }
    }
}
