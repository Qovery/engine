use std::hash::{Hash, Hasher};

use chrono::{DateTime, Utc};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;

use crate::build_platform::{Build, BuildOptions, GitRepository, Image};
use crate::cloud_provider::aws::databases::mongodb::MongoDB;
use crate::cloud_provider::aws::databases::mysql::MySQL;
use crate::cloud_provider::aws::databases::postgresql::PostgreSQL;
use crate::cloud_provider::aws::databases::redis::Redis;
use crate::cloud_provider::service::{DatabaseOptions, StatefulService, StatelessService};
use crate::cloud_provider::utilities::VersionsNumber;
use crate::cloud_provider::CloudProvider;
use crate::cloud_provider::Kind as CPKind;
use crate::git::Credentials;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

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
            .map(|x| match built_applications.iter().find(|y| x.id.as_str() == y.id()) {
                Some(app) => x.to_stateless_service(context, app.image().clone(), cloud_provider),
                _ => x.to_stateless_service(context, x.to_image(), cloud_provider),
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

        // orders is important, first external services, then applications and then routers.
        let mut stateless_services = applications;
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

fn default_root_path_value() -> String {
    "/".to_string()
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub action: Action,
    pub git_url: String,
    pub git_credentials: Option<GitCredentials>,
    pub branch: String,
    pub commit_id: String,
    pub dockerfile_path: Option<String>,
    pub buildpack_language: Option<String>,
    #[serde(default = "default_root_path_value")]
    pub root_path: String,
    pub private_port: Option<u16>,
    pub total_cpus: String,
    pub cpu_burst: String,
    pub total_ram_in_mib: u32,
    pub total_instances: u16,
    pub start_timeout_in_seconds: u32,
    pub storage: Vec<Storage>,
    // Key is a String, Value is a base64 encoded String
    // Use BTreeMap to get Hash trait which is not available on HashMap
    pub environment_vars: BTreeMap<String, String>,
}

impl Application {
    pub fn to_application<'a>(
        &self,
        context: &Context,
        image: &Image,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<(dyn crate::cloud_provider::service::Application)>> {
        let environment_variables = to_environment_variable(&self.environment_vars);
        let listeners = cloud_provider.listeners().clone();

        match cloud_provider.kind() {
            CPKind::Aws => Some(Box::new(crate::cloud_provider::aws::application::Application::new(
                context.clone(),
                self.id.as_str(),
                self.action.to_service_action(),
                self.name.as_str(),
                self.private_port,
                self.total_cpus.clone(),
                self.cpu_burst.clone(),
                self.total_ram_in_mib,
                self.total_instances,
                self.start_timeout_in_seconds,
                image.clone(),
                self.storage.iter().map(|s| s.to_aws_storage()).collect::<Vec<_>>(),
                environment_variables,
                listeners,
            ))),
            CPKind::Do => Some(Box::new(
                crate::cloud_provider::digitalocean::application::Application::new(
                    context.clone(),
                    self.id.as_str(),
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.private_port,
                    self.total_cpus.clone(),
                    self.cpu_burst.clone(),
                    self.total_ram_in_mib,
                    self.total_instances,
                    self.start_timeout_in_seconds,
                    image.clone(),
                    self.storage.iter().map(|s| s.to_do_storage()).collect::<Vec<_>>(),
                    environment_variables,
                    listeners,
                ),
            )),
            CPKind::Scw => Some(Box::new(
                crate::cloud_provider::scaleway::application::Application::new(
                    context.clone(),
                    self.id.as_str(),
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.private_port,
                    self.total_cpus.clone(),
                    self.cpu_burst.clone(),
                    self.total_ram_in_mib,
                    self.total_instances,
                    self.start_timeout_in_seconds,
                    image.clone(),
                    self.storage.iter().map(|s| s.to_scw_storage()).collect::<Vec<_>>(),
                    environment_variables,
                    listeners,
                ),
            )),
        }
    }

    pub fn to_stateless_service(
        &self,
        context: &Context,
        image: Image,
        cloud_provider: &dyn CloudProvider,
    ) -> Option<Box<dyn StatelessService>> {
        let environment_variables = to_environment_variable(&self.environment_vars);
        let listeners = cloud_provider.listeners().clone();

        match cloud_provider.kind() {
            CPKind::Aws => Some(Box::new(crate::cloud_provider::aws::application::Application::new(
                context.clone(),
                self.id.as_str(),
                self.action.to_service_action(),
                self.name.as_str(),
                self.private_port,
                self.total_cpus.clone(),
                self.cpu_burst.clone(),
                self.total_ram_in_mib,
                self.total_instances,
                self.start_timeout_in_seconds,
                image,
                self.storage.iter().map(|s| s.to_aws_storage()).collect::<Vec<_>>(),
                environment_variables,
                listeners,
            ))),
            CPKind::Do => Some(Box::new(
                crate::cloud_provider::digitalocean::application::Application::new(
                    context.clone(),
                    self.id.as_str(),
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.private_port,
                    self.total_cpus.clone(),
                    self.cpu_burst.clone(),
                    self.total_ram_in_mib,
                    self.total_instances,
                    self.start_timeout_in_seconds,
                    image,
                    self.storage.iter().map(|s| s.to_do_storage()).collect::<Vec<_>>(),
                    environment_variables,
                    listeners,
                ),
            )),
            CPKind::Scw => Some(Box::new(
                crate::cloud_provider::scaleway::application::Application::new(
                    context.clone(),
                    self.id.as_str(),
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.private_port,
                    self.total_cpus.clone(),
                    self.cpu_burst.clone(),
                    self.total_ram_in_mib,
                    self.total_instances,
                    self.start_timeout_in_seconds,
                    image,
                    self.storage.iter().map(|s| s.to_scw_storage()).collect::<Vec<_>>(),
                    environment_variables,
                    listeners,
                ),
            )),
        }
    }

    pub fn to_image(&self) -> Image {
        // Image tag == hash(root_path) + commit_id truncate to 127 char
        // https://github.com/distribution/distribution/blob/6affafd1f030087d88f88841bf66a8abe2bf4d24/reference/regexp.go#L41
        let mut hasher = DefaultHasher::new();

        // If any of those variables changes, we'll get a new hash value, what results in a new image
        // build and avoids using cache. It is important to build a new image, as those variables may
        // affect the build result even if user didn't change his code.
        self.root_path.hash(&mut hasher);
        self.dockerfile_path.hash(&mut hasher);
        self.environment_vars.hash(&mut hasher);

        let mut tag = format!("{}-{}", hasher.finish(), self.commit_id);
        tag.truncate(127);

        Image {
            application_id: self.id.clone(),
            name: self.name.clone(),
            tag,
            commit_id: self.commit_id.clone(),
            registry_name: None,
            registry_secret: None,
            registry_url: None,
            registry_docker_json_config: None,
        }
    }

    pub fn to_build(&self) -> Build {
        Build {
            git_repository: GitRepository {
                url: self.git_url.clone(),
                credentials: self.git_credentials.as_ref().map(|credentials| Credentials {
                    login: credentials.login.clone(),
                    password: credentials.access_token.clone(),
                }),
                commit_id: self.commit_id.clone(),
                dockerfile_path: self.dockerfile_path.clone(),
                root_path: self.root_path.clone(),
                buildpack_language: self.buildpack_language.clone(),
            },
            image: self.to_image(),
            options: BuildOptions {
                environment_variables: self
                    .environment_vars
                    .iter()
                    .map(|(k, v)| crate::build_platform::EnvironmentVariable {
                        key: k.clone(),
                        value: String::from_utf8_lossy(&base64::decode(v.as_bytes()).unwrap_or(vec![])).into_owned(),
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

pub fn to_environment_variable(
    env_vars: &BTreeMap<String, String>,
) -> Vec<crate::cloud_provider::models::EnvironmentVariable> {
    env_vars
        .iter()
        .map(|(k, v)| crate::cloud_provider::models::EnvironmentVariable {
            key: k.clone(),
            value: v.clone(),
        })
        .collect()
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
    pub fn to_aws_storage(
        &self,
    ) -> crate::cloud_provider::models::Storage<crate::cloud_provider::aws::application::StorageType> {
        crate::cloud_provider::models::Storage {
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

    pub fn to_do_storage(
        &self,
    ) -> crate::cloud_provider::models::Storage<crate::cloud_provider::digitalocean::application::StorageType> {
        crate::cloud_provider::models::Storage {
            id: self.id.clone(),
            name: self.name.clone(),
            storage_type: crate::cloud_provider::digitalocean::application::StorageType::Standard,
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }

    pub fn to_scw_storage(
        &self,
    ) -> crate::cloud_provider::models::Storage<crate::cloud_provider::scaleway::application::StorageType> {
        crate::cloud_provider::models::Storage {
            id: self.id.clone(),
            name: self.name.clone(),
            storage_type: crate::cloud_provider::scaleway::application::StorageType::BlockSsd,
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
        let custom_domains = self
            .custom_domains
            .iter()
            .map(|x| crate::cloud_provider::models::CustomDomain {
                domain: x.domain.clone(),
                target_domain: x.target_domain.clone(),
            })
            .collect::<Vec<_>>();

        let routes = self
            .routes
            .iter()
            .map(|x| crate::cloud_provider::models::Route {
                path: x.path.clone(),
                application_name: x.application_name.clone(),
            })
            .collect::<Vec<_>>();

        let listeners = cloud_provider.listeners().clone();

        match cloud_provider.kind() {
            CPKind::Aws => {
                let router: Box<dyn StatelessService> = Box::new(crate::cloud_provider::aws::router::Router::new(
                    context.clone(),
                    self.id.as_str(),
                    self.name.as_str(),
                    self.action.to_service_action(),
                    self.default_domain.as_str(),
                    custom_domains,
                    routes,
                    listeners,
                ));
                Some(router)
            }
            CPKind::Do => {
                let router: Box<dyn StatelessService> =
                    Box::new(crate::cloud_provider::digitalocean::router::Router::new(
                        context.clone(),
                        self.id.as_str(),
                        self.name.as_str(),
                        self.action.to_service_action(),
                        self.default_domain.as_str(),
                        custom_domains,
                        routes,
                        listeners,
                    ));
                Some(router)
            }
            CPKind::Scw => {
                let router: Box<dyn StatelessService> = Box::new(crate::cloud_provider::scaleway::router::Router::new(
                    context.clone(),
                    self.id.as_str(),
                    self.name.as_str(),
                    self.action.to_service_action(),
                    self.default_domain.as_str(),
                    custom_domains,
                    routes,
                    listeners,
                ));
                Some(router)
            }
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
    #[serde(default)] // => false if not present in input
    pub activate_high_availability: bool,
    #[serde(default)] // => false if not present in input
    pub activate_backups: bool,
    #[serde(default)] // => false if not present in input
    pub publicly_accessible: bool,
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
            activate_high_availability: self.activate_high_availability,
            activate_backups: self.activate_backups,
            publicly_accessible: self.publicly_accessible,
        };

        let listeners = cloud_provider.listeners().clone();

        match cloud_provider.kind() {
            CPKind::Aws => match self.kind {
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
                        listeners,
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
                        listeners,
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
                        listeners,
                    ));

                    Some(db)
                }
                DatabaseKind::Redis => {
                    let db: Box<dyn StatefulService> = Box::new(Redis::new(
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
                        listeners,
                    ));

                    Some(db)
                }
            },
            CPKind::Do => match self.kind {
                DatabaseKind::Postgresql => {
                    let db: Box<dyn StatefulService> = Box::new(
                        crate::cloud_provider::digitalocean::databases::postgresql::PostgreSQL::new(
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
                            listeners,
                        ),
                    );

                    Some(db)
                }
                DatabaseKind::Mysql => {
                    let db: Box<dyn StatefulService> =
                        Box::new(crate::cloud_provider::digitalocean::databases::mysql::MySQL::new(
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
                            listeners,
                        ));

                    Some(db)
                }
                DatabaseKind::Redis => {
                    let db: Box<dyn StatefulService> =
                        Box::new(crate::cloud_provider::digitalocean::databases::redis::Redis::new(
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
                            listeners,
                        ));

                    Some(db)
                }
                DatabaseKind::Mongodb => {
                    let db: Box<dyn StatefulService> =
                        Box::new(crate::cloud_provider::digitalocean::databases::mongodb::MongoDB::new(
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
                            listeners,
                        ));

                    Some(db)
                }
            },
            CPKind::Scw => match self.kind {
                DatabaseKind::Postgresql => match VersionsNumber::from_str(self.version.as_str()) {
                    Ok(v) => {
                        let db: Box<dyn StatefulService> =
                            Box::new(crate::cloud_provider::scaleway::databases::postgresql::PostgreSQL::new(
                                context.clone(),
                                self.id.as_str(),
                                self.action.to_service_action(),
                                self.name.as_str(),
                                v,
                                self.fqdn.as_str(),
                                self.fqdn_id.as_str(),
                                self.total_cpus.clone(),
                                self.total_ram_in_mib,
                                self.database_instance_type.as_str(),
                                database_options,
                                listeners,
                            ));

                        Some(db)
                    }
                    Err(e) => {
                        error!("{}", format!("error while parsing postgres version, error: {}", e));
                        None
                    }
                },
                DatabaseKind::Mysql => match VersionsNumber::from_str(self.version.as_str()) {
                    Ok(v) => {
                        let db: Box<dyn StatefulService> =
                            Box::new(crate::cloud_provider::scaleway::databases::mysql::MySQL::new(
                                context.clone(),
                                self.id.as_str(),
                                self.action.to_service_action(),
                                self.name.as_str(),
                                v,
                                self.fqdn.as_str(),
                                self.fqdn_id.as_str(),
                                self.total_cpus.clone(),
                                self.total_ram_in_mib,
                                self.database_instance_type.as_str(),
                                database_options,
                                listeners,
                            ));

                        Some(db)
                    }
                    Err(e) => {
                        error!("{}", format!("error while parsing mysql version, error: {}", e));
                        None
                    }
                },
                DatabaseKind::Redis => {
                    let db: Box<dyn StatefulService> =
                        Box::new(crate::cloud_provider::scaleway::databases::redis::Redis::new(
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
                            listeners,
                        ));

                    Some(db)
                }
                DatabaseKind::Mongodb => {
                    let db: Box<dyn StatefulService> =
                        Box::new(crate::cloud_provider::scaleway::databases::mongodb::MongoDB::new(
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
                            listeners,
                        ));

                    Some(db)
                }
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatabaseKind {
    Postgresql,
    Mysql,
    Mongodb,
    Redis,
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
            message: message.map(|msg| msg.into()),
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

pub trait ProgressListener: Send + Sync {
    fn deployment_in_progress(&self, info: ProgressInfo);
    fn pause_in_progress(&self, info: ProgressInfo);
    fn delete_in_progress(&self, info: ProgressInfo);
    fn error(&self, info: ProgressInfo);
    fn deployed(&self, info: ProgressInfo);
    fn paused(&self, info: ProgressInfo);
    fn deleted(&self, info: ProgressInfo);
    fn deployment_error(&self, info: ProgressInfo);
    fn pause_error(&self, info: ProgressInfo);
    fn delete_error(&self, info: ProgressInfo);
}

pub trait Listen {
    fn listeners(&self) -> &Listeners;
    fn add_listener(&mut self, listener: Listener);
}

pub type Listener = Arc<Box<dyn ProgressListener>>;
pub type Listeners = Vec<Listener>;

pub struct ListenersHelper<'a> {
    listeners: &'a Listeners,
}

impl<'a> ListenersHelper<'a> {
    pub fn new(listeners: &'a Listeners) -> Self {
        ListenersHelper { listeners }
    }

    pub fn deployment_in_progress(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.deployment_in_progress(info.clone()));
    }

    pub fn upgrade_in_progress(&self, info: ProgressInfo) {
        self.listeners
            .iter()
            .for_each(|l| l.deployment_in_progress(info.clone()));
    }

    pub fn pause_in_progress(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.pause_in_progress(info.clone()));
    }

    pub fn delete_in_progress(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.delete_in_progress(info.clone()));
    }

    pub fn error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.error(info.clone()));
    }

    pub fn deployed(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.deployed(info.clone()));
    }

    pub fn paused(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.paused(info.clone()));
    }

    pub fn deleted(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.deleted(info.clone()));
    }

    pub fn deployment_error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.deployment_error(info.clone()));
    }

    pub fn pause_error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.pause_error(info.clone()));
    }

    pub fn delete_error(&self, info: ProgressInfo) {
        self.listeners.iter().for_each(|l| l.delete_error(info.clone()));
    }
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct Context {
    execution_id: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    test_cluster: bool,
    docker_host: Option<String>,
    features: Vec<Features>,
    metadata: Option<Metadata>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, Eq, PartialEq)]
pub enum Features {
    LogsHistory,
    MetricsHistory,
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
            .map(|e| e.to_string())
            .collect::<String>();
        new.execution_id = format!("{}-{}", self.execution_id, suffix);
        new
    }
}

impl Context {
    pub fn new(
        execution_id: String,
        workspace_root_dir: String,
        lib_root_dir: String,
        test_cluster: bool,
        docker_host: Option<String>,
        features: Vec<Features>,
        metadata: Option<Metadata>,
    ) -> Self {
        Context {
            execution_id,
            workspace_root_dir,
            lib_root_dir,
            test_cluster,
            docker_host,
            features,
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

    pub fn is_dry_run_deploy(&self) -> bool {
        match &self.metadata {
            Some(meta) => matches!(meta.dry_run_deploy, Some(true)),
            _ => false,
        }
    }

    pub fn disable_pleco(&self) -> bool {
        match &self.metadata {
            Some(meta) => meta.disable_pleco.unwrap_or(true),
            _ => true,
        }
    }

    pub fn requires_forced_upgrade(&self) -> bool {
        match &self.metadata {
            Some(meta) => matches!(meta.forced_upgrade, Some(true)),
            _ => false,
        }
    }

    pub fn is_test_cluster(&self) -> bool {
        self.test_cluster
    }

    pub fn resource_expiration_in_seconds(&self) -> Option<u32> {
        match &self.metadata {
            Some(meta) => meta.resource_expiration_in_seconds.map(|ttl| ttl),
            _ => None,
        }
    }

    pub fn docker_build_options(&self) -> Option<Vec<String>> {
        match &self.metadata {
            Some(meta) => meta
                .docker_build_options
                .clone()
                .map(|b| b.split(' ').map(|x| x.to_string()).collect()),
            _ => None,
        }
    }

    // Qovery features
    pub fn is_feature_enabled(&self, name: &Features) -> bool {
        for feature in &self.features {
            if feature == name {
                return true;
            }
        }
        false
    }
}

/// put everything you want here that is required to change the behaviour of the request.
/// E.g you can indicate that this request is a test, then you can adapt the behaviour as you want.
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Metadata {
    pub dry_run_deploy: Option<bool>,
    pub resource_expiration_in_seconds: Option<u32>,
    pub docker_build_options: Option<String>,
    pub forced_upgrade: Option<bool>,
    pub disable_pleco: Option<bool>,
}

impl Metadata {
    pub fn new(
        dry_run_deploy: Option<bool>,
        resource_expiration_in_seconds: Option<u32>,
        docker_build_options: Option<String>,
        forced_upgrade: Option<bool>,
        disable_pleco: Option<bool>,
    ) -> Self {
        Metadata {
            dry_run_deploy,
            resource_expiration_in_seconds,
            docker_build_options,
            forced_upgrade,
            disable_pleco,
        }
    }
}

/// Represent a String path instead of passing a PathBuf struct
pub type StringPath = String;
