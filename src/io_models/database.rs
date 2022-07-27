use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::{service, CloudProvider, Kind as CPKind};
use crate::io_models::context::Context;
use crate::io_models::Action;
use crate::logger::Logger;
use crate::models;
use crate::models::database::{Container, DatabaseError, DatabaseService, Managed, MongoDB, MySQL, PostgresSQL, Redis};
use crate::models::types::CloudProvider as CloudProviderTrait;
use crate::models::types::{AWSEc2, VersionsNumber, AWS, DO, SCW};
use core::result::Result;
use core::result::Result::{Err, Ok};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum DatabaseMode {
    MANAGED,
    CONTAINER,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Database {
    pub kind: DatabaseKind,
    pub action: Action,
    pub long_id: Uuid,
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
    ) -> Result<Box<dyn DatabaseService>, DatabaseError> {
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
        let version = VersionsNumber::from_str(self.version.as_str())
            .map_err(|_| DatabaseError::InvalidConfig(format!("Bad version number: {}", self.version)))?;

        match (cloud_provider.kind(), &self.kind, &self.mode) {
            (CPKind::Aws, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Managed, PostgresSQL>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Managed, PostgresSQL>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Container, PostgresSQL>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Container, PostgresSQL>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                }
            }

            (CPKind::Aws, DatabaseKind::Mysql, DatabaseMode::MANAGED) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Managed, MySQL>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Managed, MySQL>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Container, MySQL>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Container, MySQL>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Redis, DatabaseMode::MANAGED) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Managed, Redis>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Managed, Redis>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Container, Redis>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Container, Redis>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Mongodb, DatabaseMode::MANAGED) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Managed, MongoDB>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Managed, MongoDB>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Container, MongoDB>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Container, MongoDB>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        version,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        self.database_instance_type.as_str(),
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        listeners,
                        logger,
                    )?))
                }
            }

            (CPKind::Do, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<DO, Container, PostgresSQL>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Do, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<DO, Container, MySQL>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Do, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<DO, Container, Redis>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Do, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<DO, Container, MongoDB>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Do, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => Err(
                DatabaseError::UnsupportedManagedMode(service::DatabaseType::PostgreSQL, DO::full_name().to_string()),
            ),
            (CPKind::Do, DatabaseKind::Mysql, DatabaseMode::MANAGED) => Err(DatabaseError::UnsupportedManagedMode(
                service::DatabaseType::MySQL,
                DO::full_name().to_string(),
            )),
            (CPKind::Do, DatabaseKind::Redis, DatabaseMode::MANAGED) => Err(DatabaseError::UnsupportedManagedMode(
                service::DatabaseType::Redis,
                DO::full_name().to_string(),
            )),
            (CPKind::Do, DatabaseKind::Mongodb, DatabaseMode::MANAGED) => Err(DatabaseError::UnsupportedManagedMode(
                service::DatabaseType::MongoDB,
                DO::full_name().to_string(),
            )),

            (CPKind::Scw, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => {
                let db = models::database::Database::<SCW, Managed, PostgresSQL>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<SCW, Container, PostgresSQL>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Mysql, DatabaseMode::MANAGED) => {
                let db = models::database::Database::<SCW, Managed, MySQL>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<SCW, Container, MySQL>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<SCW, Container, Redis>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<SCW, Container, MongoDB>::new(
                    context.clone(),
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    version,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    self.database_instance_type.as_str(),
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    listeners,
                    logger,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Redis, DatabaseMode::MANAGED) => Err(DatabaseError::UnsupportedManagedMode(
                service::DatabaseType::Redis,
                SCW::full_name().to_string(),
            )),
            (CPKind::Scw, DatabaseKind::Mongodb, DatabaseMode::MANAGED) => Err(DatabaseError::UnsupportedManagedMode(
                service::DatabaseType::MongoDB,
                SCW::full_name().to_string(),
            )),
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

#[derive(Eq, PartialEq)]
pub struct DatabaseOptions {
    pub login: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub mode: DatabaseMode,
    pub disk_size_in_gib: u32,
    pub database_disk_type: String,
    pub encrypt_disk: bool,
    pub activate_high_availability: bool,
    pub activate_backups: bool,
    pub publicly_accessible: bool,
}
