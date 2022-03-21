use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use itertools::Itertools;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::build_platform::{Build, BuildOptions, Credentials, GitRepository, Image, SshKey};
use crate::cloud_provider::aws::application::ApplicationAws;
use crate::cloud_provider::aws::databases::mongodb::MongoDbAws;
use crate::cloud_provider::aws::databases::mysql::MySQLAws;
use crate::cloud_provider::aws::databases::postgresql::PostgreSQLAws;
use crate::cloud_provider::aws::databases::redis::RedisAws;
use crate::cloud_provider::aws::router::RouterAws;
use crate::cloud_provider::digitalocean::application::ApplicationDo;
use crate::cloud_provider::digitalocean::databases::mongodb::MongoDo;
use crate::cloud_provider::digitalocean::databases::mysql::MySQLDo;
use crate::cloud_provider::digitalocean::databases::postgresql::PostgresDo;
use crate::cloud_provider::digitalocean::databases::redis::RedisDo;
use crate::cloud_provider::digitalocean::router::RouterDo;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::scaleway::application::ApplicationScw;
use crate::cloud_provider::scaleway::databases::mongodb::MongoDbScw;
use crate::cloud_provider::scaleway::databases::mysql::MySQLScw;
use crate::cloud_provider::scaleway::databases::postgresql::PostgresScw;
use crate::cloud_provider::scaleway::databases::redis::RedisScw;
use crate::cloud_provider::scaleway::router::RouterScw;
use crate::cloud_provider::service::DatabaseOptions;
use crate::cloud_provider::utilities::VersionsNumber;
use crate::cloud_provider::CloudProvider;
use crate::cloud_provider::Kind as CPKind;
use crate::cmd::docker::Docker;
use crate::container_registry::ContainerRegistryInfo;
use crate::logger::Logger;
use crate::utilities::get_image_tag;

#[derive(Clone, Debug)]
pub struct QoveryIdentifier {
    raw_long_id: String,
    short: String,
}

impl QoveryIdentifier {
    pub fn new(raw_long_id: String, raw_short_id: String) -> Self {
        QoveryIdentifier {
            raw_long_id,
            short: raw_short_id,
        }
    }

    pub fn new_from_long_id(raw_long_id: String) -> Self {
        QoveryIdentifier::new(
            raw_long_id.to_string(),
            QoveryIdentifier::extract_short(raw_long_id.as_str()),
        )
    }

    pub fn new_random() -> Self {
        Self::new_from_long_id(uuid::Uuid::new_v4().to_string())
    }

    fn extract_short(raw: &str) -> String {
        let max_execution_id_chars: usize = 8;
        match raw.char_indices().nth(max_execution_id_chars - 1) {
            None => raw.to_string(),
            Some((_, _)) => raw[..max_execution_id_chars].to_string(),
        }
    }

    pub fn short(&self) -> &str {
        &self.short
    }
}

impl From<String> for QoveryIdentifier {
    fn from(s: String) -> Self {
        QoveryIdentifier::new_from_long_id(s)
    }
}

impl Display for QoveryIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.raw_long_id.as_str())
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentRequest {
    pub execution_id: String,
    pub id: String,
    pub owner_id: String,
    pub project_id: String,
    pub organization_id: String,
    pub action: Action,
    pub applications: Vec<Application>,
    pub routers: Vec<Router>,
    pub databases: Vec<Database>,
    pub clone_from_environment_id: Option<String>,
}

impl EnvironmentRequest {
    pub fn to_environment_domain(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        container_registry: &ContainerRegistryInfo,
        logger: Box<dyn Logger>,
    ) -> Environment {
        let builds = self
            .applications
            .iter()
            .filter(|app| app.action == Action::Create)
            .map(|app| app.to_build(&container_registry))
            .collect();

        //FIXME: remove those flatten as it hide errors regarding conversion to model data type
        let applications = self
            .applications
            .iter()
            .map(|x| x.to_application_domain(context, x.to_image(container_registry), cloud_provider, logger.clone()))
            .flatten()
            .collect::<Vec<_>>();

        let routers = self
            .routers
            .iter()
            .map(|x| x.to_router_domain(context, cloud_provider, logger.clone()))
            .flatten()
            .collect::<Vec<_>>();

        let databases = self
            .databases
            .iter()
            .map(|x| x.to_database_domain(context, cloud_provider, logger.clone()))
            .flatten()
            .collect::<Vec<_>>();

        Environment::new(
            self.id.as_str(),
            self.project_id.as_str(),
            self.owner_id.as_str(),
            self.organization_id.as_str(),
            self.action.to_service_action(),
            applications,
            routers,
            databases,
            builds,
        )
    }
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
pub enum Protocol {
    HTTP,
    TCP,
    UDP,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Port {
    pub id: String,
    pub long_id: uuid::Uuid,
    pub port: u16,
    pub public_port: Option<u16>,
    pub name: Option<String>,
    pub publicly_accessible: bool,
    pub protocol: Protocol,
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
    pub ports: Vec<Port>,
    pub total_cpus: String,
    pub cpu_burst: String,
    pub total_ram_in_mib: u32,
    pub min_instances: u32,
    pub max_instances: u32,
    pub start_timeout_in_seconds: u32,
    pub storage: Vec<Storage>,
    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    pub environment_vars: BTreeMap<String, String>,
}

impl Application {
    pub fn to_application_domain(
        &self,
        context: &Context,
        image: Image,
        cloud_provider: &dyn CloudProvider,
        logger: Box<dyn Logger>,
    ) -> Option<Box<dyn crate::cloud_provider::service::Application>> {
        let environment_variables = to_environment_variable(&self.environment_vars);
        let listeners = cloud_provider.listeners().clone();

        match cloud_provider.kind() {
            CPKind::Aws => Some(Box::new(ApplicationAws::new(
                context.clone(),
                self.id.as_str(),
                self.action.to_service_action(),
                self.name.as_str(),
                self.ports.clone(),
                self.total_cpus.clone(),
                self.cpu_burst.clone(),
                self.total_ram_in_mib,
                self.min_instances,
                self.max_instances,
                self.start_timeout_in_seconds,
                image,
                self.storage.iter().map(|s| s.to_aws_storage()).collect::<Vec<_>>(),
                environment_variables,
                listeners,
                logger.clone(),
            ))),
            CPKind::Do => Some(Box::new(ApplicationDo::new(
                context.clone(),
                self.id.as_str(),
                self.action.to_service_action(),
                self.name.as_str(),
                self.ports.clone(),
                self.total_cpus.clone(),
                self.cpu_burst.clone(),
                self.total_ram_in_mib,
                self.min_instances,
                self.max_instances,
                self.start_timeout_in_seconds,
                image,
                self.storage.iter().map(|s| s.to_do_storage()).collect::<Vec<_>>(),
                environment_variables,
                listeners,
                logger.clone(),
            ))),
            CPKind::Scw => Some(Box::new(ApplicationScw::new(
                context.clone(),
                self.id.as_str(),
                self.action.to_service_action(),
                self.name.as_str(),
                self.ports.clone(),
                self.total_cpus.clone(),
                self.cpu_burst.clone(),
                self.total_ram_in_mib,
                self.min_instances,
                self.max_instances,
                self.start_timeout_in_seconds,
                image,
                self.storage.iter().map(|s| s.to_scw_storage()).collect::<Vec<_>>(),
                environment_variables,
                listeners,
                logger.clone(),
            ))),
        }
    }

    pub fn to_image(&self, cr_info: &ContainerRegistryInfo) -> Image {
        Image {
            application_id: self.id.clone(),
            name: (cr_info.get_image_name)(&self.name),
            tag: get_image_tag(
                &self.root_path,
                &self.dockerfile_path,
                &self.environment_vars,
                &self.commit_id,
            ),
            commit_id: self.commit_id.clone(),
            registry_name: cr_info.registry_name.clone(),
            registry_url: cr_info.endpoint.clone(),
            registry_docker_json_config: cr_info.registry_docker_json_config.clone(),
        }
    }

    pub fn to_build(&self, registry_url: &ContainerRegistryInfo) -> Build {
        // Retrieve ssh keys from env variables
        const ENV_GIT_PREFIX: &str = "GIT_SSH_KEY";
        let env_ssh_keys: Vec<(String, String)> = self
            .environment_vars
            .iter()
            .filter_map(|(name, value)| {
                if name.starts_with(ENV_GIT_PREFIX) {
                    Some((name.clone(), value.clone()))
                } else {
                    None
                }
            })
            .collect();

        // Get passphrase and public key if provided by the user
        let mut ssh_keys: Vec<SshKey> = Vec::with_capacity(env_ssh_keys.len());
        for (ssh_key_name, private_key) in env_ssh_keys {
            let private_key = if let Ok(Ok(private_key)) = base64::decode(private_key).map(String::from_utf8) {
                private_key
            } else {
                error!("Invalid base64 environment variable for {}", ssh_key_name);
                continue;
            };

            let passphrase = self
                .environment_vars
                .get(&ssh_key_name.replace(ENV_GIT_PREFIX, "GIT_SSH_PASSPHRASE"))
                .map(|val| base64::decode(val).ok())
                .flatten()
                .map(|str| String::from_utf8(str).ok())
                .flatten();

            let public_key = self
                .environment_vars
                .get(&ssh_key_name.replace(ENV_GIT_PREFIX, "GIT_SSH_PUBLIC_KEY"))
                .map(|val| base64::decode(val).ok())
                .flatten()
                .map(|str| String::from_utf8(str).ok())
                .flatten();

            ssh_keys.push(SshKey {
                private_key,
                passphrase,
                public_key,
            });
        }

        // Convert our root path to an relative path to be able to append them correctly
        let root_path = if Path::new(&self.root_path).is_absolute() {
            PathBuf::from(self.root_path.trim_start_matches('/'))
        } else {
            PathBuf::from(&self.root_path)
        };
        assert!(root_path.is_relative(), "root path is not a relative path");

        let dockerfile_path = self.dockerfile_path.as_ref().map(|path| {
            if Path::new(&path).is_absolute() {
                root_path.join(path.trim_start_matches('/'))
            } else {
                root_path.join(&path)
            }
        });

        //FIXME: Return a result the function
        let url = Url::parse(&self.git_url).unwrap_or_else(|_| Url::parse("https://invalid-git-url.com").unwrap());

        Build {
            git_repository: GitRepository {
                url,
                credentials: self.git_credentials.as_ref().map(|credentials| Credentials {
                    login: credentials.login.clone(),
                    password: credentials.access_token.clone(),
                }),
                ssh_keys,
                commit_id: self.commit_id.clone(),
                dockerfile_path,
                root_path,
                buildpack_language: self.buildpack_language.clone(),
            },
            image: self.to_image(registry_url),
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
    #[serde(default)]
    /// sticky_sessions_enabled: enables sticky session for the request to come to the same
    /// pod replica that was responding to the request before
    pub sticky_sessions_enabled: bool,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

impl Router {
    pub fn to_router_domain(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        logger: Box<dyn Logger>,
    ) -> Option<Box<dyn crate::cloud_provider::service::Router>> {
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
                let router = Box::new(RouterAws::new(
                    context.clone(),
                    self.id.as_str(),
                    self.name.as_str(),
                    self.action.to_service_action(),
                    self.default_domain.as_str(),
                    custom_domains,
                    routes,
                    self.sticky_sessions_enabled,
                    listeners,
                    logger,
                ));
                Some(router)
            }
            CPKind::Do => {
                let router = Box::new(RouterDo::new(
                    context.clone(),
                    self.id.as_str(),
                    self.name.as_str(),
                    self.action.to_service_action(),
                    self.default_domain.as_str(),
                    custom_domains,
                    routes,
                    self.sticky_sessions_enabled,
                    listeners,
                    logger,
                ));
                Some(router)
            }
            CPKind::Scw => {
                let router = Box::new(RouterScw::new(
                    context.clone(),
                    self.id.as_str(),
                    self.name.as_str(),
                    self.action.to_service_action(),
                    self.default_domain.as_str(),
                    custom_domains,
                    routes,
                    self.sticky_sessions_enabled,
                    listeners,
                    logger,
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
pub enum DatabaseMode {
    MANAGED,
    CONTAINER,
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
    pub encrypt_disk: bool,
    #[serde(default)] // => false if not present in input
    pub activate_high_availability: bool,
    #[serde(default)] // => false if not present in input
    pub activate_backups: bool,
    pub publicly_accessible: bool,
    pub mode: DatabaseMode,
}

impl Database {
    pub fn to_database_domain(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        logger: Box<dyn Logger>,
    ) -> Option<Box<dyn crate::cloud_provider::service::Database>> {
        let database_options = DatabaseOptions {
            mode: self.mode.clone(),
            login: self.username.clone(),
            password: self.password.clone(),
            host: self.fqdn.clone(),
            port: self.port,
            disk_size_in_gib: self.disk_size_in_gib,
            database_disk_type: self.database_disk_type.clone(),
            encrypt_disk: self.encrypt_disk,
            activate_high_availability: self.activate_high_availability,
            activate_backups: self.activate_backups,
            publicly_accessible: self.publicly_accessible,
        };

        let listeners = cloud_provider.listeners().clone();

        match cloud_provider.kind() {
            CPKind::Aws => match self.kind {
                DatabaseKind::Postgresql => {
                    let db = Box::new(PostgreSQLAws::new(
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
                        logger,
                    ));

                    Some(db)
                }
                DatabaseKind::Mysql => {
                    let db = Box::new(MySQLAws::new(
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
                        logger,
                    ));

                    Some(db)
                }
                DatabaseKind::Mongodb => {
                    let db = Box::new(MongoDbAws::new(
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
                        logger,
                    ));

                    Some(db)
                }
                DatabaseKind::Redis => {
                    let db = Box::new(RedisAws::new(
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
                        logger,
                    ));

                    Some(db)
                }
            },
            CPKind::Do => match self.kind {
                DatabaseKind::Postgresql => {
                    let db = Box::new(PostgresDo::new(
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
                        logger,
                    ));

                    Some(db)
                }
                DatabaseKind::Mysql => {
                    let db = Box::new(MySQLDo::new(
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
                        logger,
                    ));

                    Some(db)
                }
                DatabaseKind::Redis => {
                    let db = Box::new(RedisDo::new(
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
                        logger,
                    ));

                    Some(db)
                }
                DatabaseKind::Mongodb => {
                    let db = Box::new(MongoDo::new(
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
                        logger,
                    ));

                    Some(db)
                }
            },
            CPKind::Scw => match self.kind {
                DatabaseKind::Postgresql => match VersionsNumber::from_str(self.version.as_str()) {
                    Ok(v) => {
                        let db = Box::new(PostgresScw::new(
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
                            logger.clone(),
                        ));

                        Some(db)
                    }
                    Err(e) => {
                        error!(
                            "{}",
                            format!("error while parsing postgres version, error: {}", e.message())
                        );
                        None
                    }
                },
                DatabaseKind::Mysql => match VersionsNumber::from_str(self.version.as_str()) {
                    Ok(v) => {
                        let db = Box::new(MySQLScw::new(
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
                            logger.clone(),
                        ));

                        Some(db)
                    }
                    Err(e) => {
                        error!(
                            "{}",
                            format!("error while parsing mysql version, error: {}", e.message())
                        );
                        None
                    }
                },
                DatabaseKind::Redis => {
                    let db = Box::new(RedisScw::new(
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
                        logger.clone(),
                    ));

                    Some(db)
                }
                DatabaseKind::Mongodb => {
                    let db = Box::new(MongoDbScw::new(
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
                        logger,
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

impl DatabaseKind {
    pub fn name(&self) -> &str {
        match self {
            DatabaseKind::Mongodb => "mongodb",
            DatabaseKind::Mysql => "mysql",
            DatabaseKind::Postgresql => "postgresql",
            DatabaseKind::Redis => "redis",
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

#[derive(Clone)]
pub struct Context {
    organization_id: String,
    cluster_id: String,
    execution_id: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    test_cluster: bool,
    docker_host: Option<Url>,
    features: Vec<Features>,
    metadata: Option<Metadata>,
    pub docker: Docker,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, Eq, PartialEq)]
pub enum Features {
    LogsHistory,
    MetricsHistory,
}

// trait used to reimplement clone without same fields
// this trait is used for Context struct
pub trait CloneForTest {
    fn clone_not_same_execution_id(&self) -> Self;
}

// for test we need to clone context but to change the directory workspace used
// to to this we just have to suffix the execution id in tests
impl CloneForTest for Context {
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
        organization_id: String,
        cluster_id: String,
        execution_id: String,
        workspace_root_dir: String,
        lib_root_dir: String,
        test_cluster: bool,
        docker_host: Option<Url>,
        features: Vec<Features>,
        metadata: Option<Metadata>,
        docker: Docker,
    ) -> Self {
        Context {
            organization_id,
            cluster_id,
            execution_id,
            workspace_root_dir,
            lib_root_dir,
            test_cluster,
            docker_host,
            features,
            metadata,
            docker,
        }
    }

    pub fn organization_id(&self) -> &str {
        self.organization_id.as_str()
    }

    pub fn cluster_id(&self) -> &str {
        self.cluster_id.as_str()
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

    pub fn docker_tcp_socket(&self) -> &Option<Url> {
        &self.docker_host
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
    pub forced_upgrade: Option<bool>,
    pub disable_pleco: Option<bool>,
}

impl Metadata {
    pub fn new(
        dry_run_deploy: Option<bool>,
        resource_expiration_in_seconds: Option<u32>,
        forced_upgrade: Option<bool>,
        disable_pleco: Option<bool>,
    ) -> Self {
        Metadata {
            dry_run_deploy,
            resource_expiration_in_seconds,
            forced_upgrade,
            disable_pleco,
        }
    }
}

/// Represent a String path instead of passing a PathBuf struct
pub type StringPath = String;

pub trait ToTerraformString {
    fn to_terraform_format_string(&self) -> String;
}

pub trait ToHelmString {
    fn to_helm_format_string(&self) -> String;
}

/// Represents a domain, just plain domain, no protocol.
/// eq. `test.com`, `sub.test.com`
#[derive(Clone)]
pub struct Domain {
    raw: String,
    root_domain: String,
}

impl Domain {
    pub fn new(raw: String) -> Self {
        // TODO(benjaminch): This is very basic solution which doesn't take into account
        // some edge cases such as: "test.co.uk" domains
        let sep: &str = ".";
        let items: Vec<String> = raw.split(sep).map(|e| e.to_string()).collect();
        let items_count = raw.matches(sep).count() + 1;
        let top_domain: String = match items_count > 2 {
            true => items.iter().skip(items_count - 2).join("."),
            false => items.iter().join("."),
        };

        Domain {
            root_domain: top_domain,
            raw,
        }
    }

    pub fn new_with_subdomain(raw: String, sub_domain: String) -> Self {
        Domain::new(format!("{}.{}", sub_domain, raw))
    }

    pub fn with_sub_domain(&self, sub_domain: String) -> Domain {
        Domain::new(format!("{}.{}", sub_domain, self.raw))
    }

    pub fn root_domain(&self) -> Domain {
        Domain::new(self.root_domain.to_string())
    }

    pub fn wildcarded(&self) -> Domain {
        if self.is_wildcarded() {
            return self.clone();
        }

        match self.raw.is_empty() {
            false => Domain::new_with_subdomain(self.raw.to_string(), "*".to_string()),
            true => Domain::new("*".to_string()),
        }
    }

    fn is_wildcarded(&self) -> bool {
        self.raw.starts_with("*")
    }
}

impl Display for Domain {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw.as_str())
    }
}

impl ToTerraformString for Domain {
    fn to_terraform_format_string(&self) -> String {
        format!("{{{}}}", self.raw)
    }
}

impl ToHelmString for Domain {
    fn to_helm_format_string(&self) -> String {
        format!("{{{}}}", self.raw)
    }
}

impl ToTerraformString for Ipv4Addr {
    fn to_terraform_format_string(&self) -> String {
        format!("{{{}}}", self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{Domain, QoveryIdentifier};

    #[test]
    fn test_domain_new() {
        struct TestCase<'a> {
            input: String,
            expected_root_domain_output: String,
            expected_wildcarded_output: String,
            description: &'a str,
        }

        // setup:
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input: "".to_string(),
                expected_root_domain_output: "".to_string(),
                expected_wildcarded_output: "*".to_string(),
                description: "empty raw domain input",
            },
            TestCase {
                input: "*".to_string(),
                expected_root_domain_output: "*".to_string(),
                expected_wildcarded_output: "*".to_string(),
                description: "wildcard domain input",
            },
            TestCase {
                input: "*.test.com".to_string(),
                expected_root_domain_output: "test.com".to_string(),
                expected_wildcarded_output: "*.test.com".to_string(),
                description: "wildcarded domain input",
            },
            TestCase {
                input: "test.co.uk".to_string(),
                expected_root_domain_output: "co.uk".to_string(), // TODO(benjamin) => Should be test.co.uk in the future
                expected_wildcarded_output: "*.co.uk".to_string(),
                description: "broken edge case domain with special tld input",
            },
            TestCase {
                input: "test".to_string(),
                expected_root_domain_output: "test".to_string(),
                expected_wildcarded_output: "*.test".to_string(),
                description: "domain without tld input",
            },
            TestCase {
                input: "test.com".to_string(),
                expected_root_domain_output: "test.com".to_string(),
                expected_wildcarded_output: "*.test.com".to_string(),
                description: "simple top domain input",
            },
            TestCase {
                input: "sub.test.com".to_string(),
                expected_root_domain_output: "test.com".to_string(),
                expected_wildcarded_output: "*.sub.test.com".to_string(),
                description: "simple sub domain input",
            },
            TestCase {
                input: "yetanother.sub.test.com".to_string(),
                expected_root_domain_output: "test.com".to_string(),
                expected_wildcarded_output: "*.yetanother.sub.test.com".to_string(),
                description: "simple sub domain input",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = Domain::new(tc.input.clone());
            tc.expected_wildcarded_output; // to avoid warning

            // verify:
            assert_eq!(
                tc.expected_root_domain_output,
                result.root_domain().to_string(),
                "case {} : '{}'",
                tc.description,
                tc.input
            );
        }
    }

    #[test]
    fn test_qovery_identifier_new_from_long_id() {
        struct TestCase<'a> {
            input: String,
            expected_long_id_output: String,
            expected_short_output: String,
            description: &'a str,
        }

        // setup:
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input: "".to_string(),
                expected_long_id_output: "".to_string(),
                expected_short_output: "".to_string(),
                description: "empty raw long ID input",
            },
            TestCase {
                input: "2a365285-992f-4285-ab96-c55ac81ecde9".to_string(),
                expected_long_id_output: "2a365285-992f-4285-ab96-c55ac81ecde9".to_string(),
                expected_short_output: "2a365285".to_string(),
                description: "proper Uuid input",
            },
            TestCase {
                input: "2a365285".to_string(),
                expected_long_id_output: "2a365285".to_string(),
                expected_short_output: "2a365285".to_string(),
                description: "non standard Uuid input, length 8",
            },
            TestCase {
                input: "2a365285hebnrfvuebr".to_string(),
                expected_long_id_output: "2a365285hebnrfvuebr".to_string(),
                expected_short_output: "2a365285".to_string(),
                description: "non standard Uuid input, length longer than expected short (length 8)",
            },
            TestCase {
                input: "2a365".to_string(),
                expected_long_id_output: "2a365".to_string(),
                expected_short_output: "2a365".to_string(),
                description: "non standard Uuid input, length shorter than expected short (length 8)",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = QoveryIdentifier::new_from_long_id(tc.input.clone());

            // verify:
            assert_eq!(
                tc.expected_long_id_output, result.raw_long_id,
                "case {} : '{}'",
                tc.description, tc.input
            );
            assert_eq!(
                tc.expected_short_output, result.short,
                "case {} : '{}'",
                tc.description, tc.input
            );
        }
    }
}
