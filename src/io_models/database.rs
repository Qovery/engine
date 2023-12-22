use crate::cloud_provider::aws::database_instance_type::AwsDatabaseInstanceType;
use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::scaleway::database_instance_type::ScwDatabaseInstanceType;
use crate::cloud_provider::{service, CloudProvider, Kind as CPKind, Kind};
use crate::io_models::context::Context;
use crate::io_models::Action;
use crate::models;
use crate::models::database::{
    Container, DatabaseError, DatabaseInstanceType, DatabaseService, Managed, MongoDB, MySQL, PostgresSQL, Redis,
};
use crate::models::types::{AWSEc2, VersionsNumber, AWS, SCW};
use crate::models::types::{CloudProvider as CloudProviderTrait, GCP};
use chrono::{DateTime, Utc};
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
    pub kube_name: String,
    pub version: String,
    pub created_at: DateTime<Utc>,
    pub fqdn_id: String,
    pub fqdn: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub total_cpus: String,
    pub total_ram_in_mib: u32,
    pub disk_size_in_gib: u32,
    pub database_instance_type: Option<String>,
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

        let version = VersionsNumber::from_str(self.version.as_str())
            .map_err(|_| DatabaseError::InvalidConfig(format!("Bad version number: {}", self.version)))?;

        // Trying to pick database instance type for managed DB building based on cloud provider
        // Container DB instance type to be set to None as it's not needed
        let database_instance_type: Option<Box<dyn DatabaseInstanceType>> = match &self.database_instance_type {
            None => None,
            Some(database_instance_type_raw_str) => match cloud_provider.kind() {
                Kind::Aws => match AwsDatabaseInstanceType::from_str(database_instance_type_raw_str) {
                    Ok(t) => Some(Box::new(t)),
                    Err(e) => return Err(e),
                },
                Kind::Scw => match ScwDatabaseInstanceType::from_str(database_instance_type_raw_str) {
                    Ok(t) => Some(Box::new(t)),
                    Err(e) => return Err(e),
                },
                Kind::Gcp => todo!(), // TODO(benjaminch): GKE integration
            },
        };

        match (cloud_provider.kind(), &self.kind, &self.mode) {
            (CPKind::Aws, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Managed, PostgresSQL>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        database_instance_type,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Managed, PostgresSQL>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        database_instance_type,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Container, PostgresSQL>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        None,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Container, PostgresSQL>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        None,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }

            (CPKind::Aws, DatabaseKind::Mysql, DatabaseMode::MANAGED) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Managed, MySQL>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        database_instance_type,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Managed, MySQL>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        database_instance_type,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Container, MySQL>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        None,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Container, MySQL>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        None,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Redis, DatabaseMode::MANAGED) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Managed, Redis>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        database_instance_type,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Managed, Redis>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        database_instance_type,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Container, Redis>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        None,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Container, Redis>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        None,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Mongodb, DatabaseMode::MANAGED) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Managed, MongoDB>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        database_instance_type,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Managed, MongoDB>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        database_instance_type,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }
            (CPKind::Aws, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::database::Database::<AWS, Container, MongoDB>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        None,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::database::Database::<AWSEc2, Container, MongoDB>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name.clone(),
                        version,
                        self.created_at,
                        self.fqdn.as_str(),
                        self.fqdn_id.as_str(),
                        self.total_cpus.clone(),
                        self.total_ram_in_mib,
                        database_options.disk_size_in_gib,
                        None,
                        database_options.publicly_accessible,
                        database_options.port,
                        database_options,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }

            (CPKind::Scw, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => {
                let db = models::database::Database::<SCW, Managed, PostgresSQL>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    database_instance_type,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<SCW, Container, PostgresSQL>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Mysql, DatabaseMode::MANAGED) => {
                let db = models::database::Database::<SCW, Managed, MySQL>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    database_instance_type,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<SCW, Container, MySQL>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<SCW, Container, Redis>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Scw, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<SCW, Container, MongoDB>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
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

            (CPKind::Gcp, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<GCP, Container, PostgresSQL>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Gcp, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<GCP, Container, MySQL>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Gcp, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<GCP, Container, Redis>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Gcp, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<GCP, Container, MongoDB>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.total_cpus.clone(),
                    self.total_ram_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                )?;

                Ok(Box::new(db))
            }
            (CPKind::Gcp, DatabaseKind::Mysql, DatabaseMode::MANAGED) => Err(DatabaseError::UnsupportedManagedMode(
                service::DatabaseType::MySQL,
                GCP::full_name().to_string(),
            )),
            (CPKind::Gcp, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => Err(
                DatabaseError::UnsupportedManagedMode(service::DatabaseType::PostgreSQL, GCP::full_name().to_string()),
            ),
            (CPKind::Gcp, DatabaseKind::Redis, DatabaseMode::MANAGED) => Err(DatabaseError::UnsupportedManagedMode(
                service::DatabaseType::Redis,
                GCP::full_name().to_string(),
            )),
            (CPKind::Gcp, DatabaseKind::Mongodb, DatabaseMode::MANAGED) => Err(DatabaseError::UnsupportedManagedMode(
                service::DatabaseType::MongoDB,
                GCP::full_name().to_string(),
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
