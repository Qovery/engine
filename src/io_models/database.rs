use crate::environment::models;
use crate::environment::models::database::{
    Container, DatabaseError, DatabaseInstanceType, DatabaseService, Managed, MongoDB, MySQL, PostgresSQL, Redis,
};
use crate::environment::models::types::{AWS, OnPremise, SCW, VersionsNumber};
use crate::environment::models::types::{CloudProvider as CloudProviderTrait, GCP};
use crate::infrastructure::models::cloud_provider::aws::database_instance_type::AwsDatabaseInstanceType;
use crate::infrastructure::models::cloud_provider::scaleway::database_instance_type::ScwDatabaseInstanceType;
use crate::infrastructure::models::cloud_provider::{CloudProvider, Kind as CPKind, Kind, service};
use crate::io_models::Action;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use chrono::{DateTime, Utc};
use core::result::Result;
use core::result::Result::{Err, Ok};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use uuid::Uuid;

use super::annotations_group::Annotation;

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
    pub cpu_request_in_milli: u32,
    pub cpu_limit_in_milli: u32,
    pub ram_request_in_mib: u32,
    pub ram_limit_in_mib: u32,
    pub disk_size_in_gib: u32,
    pub database_instance_type: Option<String>,
    pub database_disk_type: String,
    #[serde(default)] // => None if not present in input
    pub database_disk_iops: Option<u32>,
    pub encrypt_disk: bool,
    #[serde(default)] // => false if not present in input
    pub activate_high_availability: bool,
    #[serde(default)] // => false if not present in input
    pub activate_backups: bool,
    pub publicly_accessible: bool,
    pub mode: DatabaseMode,
    #[serde(default)]
    pub annotations_group_ids: BTreeSet<Uuid>,
    #[serde(default)]
    pub labels_group_ids: BTreeSet<Uuid>,
}

impl Database {
    pub fn to_database_domain(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        annotations_group: &BTreeMap<Uuid, AnnotationsGroup>,
        labels_group: &BTreeMap<Uuid, LabelsGroup>,
    ) -> Result<Box<dyn DatabaseService>, DatabaseError> {
        let database_options = DatabaseOptions {
            mode: self.mode.clone(),
            login: self.username.clone(),
            password: self.password.clone(),
            host: self.fqdn.clone(),
            port: self.port,
            disk_size_in_gib: self.disk_size_in_gib,
            database_disk_type: self.database_disk_type.clone(),
            database_disk_iops: self
                .database_disk_iops
                .map(DiskIOPS::Provisioned)
                .unwrap_or(DiskIOPS::Default),
            encrypt_disk: self.encrypt_disk,
            activate_high_availability: self.activate_high_availability,
            activate_backups: self.activate_backups,
            publicly_accessible: self.publicly_accessible,
        };

        let annotations_groups = self
            .annotations_group_ids
            .iter()
            .flat_map(|annotations_group_id| annotations_group.get(annotations_group_id))
            .cloned()
            .collect_vec();
        let labels_groups = self
            .labels_group_ids
            .iter()
            .flat_map(|labels_group_id| labels_group.get(labels_group_id))
            .cloned()
            .collect_vec();
        let mut additional_annotations = Vec::new();
        if let (CPKind::Aws, DatabaseMode::CONTAINER) = (cloud_provider.kind(), &self.mode) {
            // alb annotations
            additional_annotations.push(Annotation {
                key: "service.beta.kubernetes.io/aws-load-balancer-additional-resource-tags".to_string(),
                value: format!(
                    "OrganizationLongId={},OrganizationId={},ClusterLongId={},ClusterId={},QoveryName={}",
                    context.organization_long_id(),
                    context.organization_short_id(),
                    context.cluster_long_id(),
                    context.cluster_short_id(),
                    self.kube_name.clone()
                ),
            });
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
                Kind::Azure => todo!(),
                Kind::Scw => match ScwDatabaseInstanceType::from_str(database_instance_type_raw_str) {
                    Ok(t) => Some(Box::new(t)),
                    Err(e) => return Err(e),
                },
                Kind::Gcp => todo!(), // TODO(benjaminch): GKE integration
                Kind::OnPremise => None,
            },
        };

        match (cloud_provider.kind(), &self.kind, &self.mode) {
            (CPKind::Aws, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => {
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    database_instance_type,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?))
            }
            (CPKind::Aws, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?))
            }

            (CPKind::Aws, DatabaseKind::Mysql, DatabaseMode::MANAGED) => {
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    database_instance_type,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?))
            }
            (CPKind::Aws, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?))
            }
            (CPKind::Aws, DatabaseKind::Redis, DatabaseMode::MANAGED) => {
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    database_instance_type,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?))
            }
            (CPKind::Aws, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?))
            }
            (CPKind::Aws, DatabaseKind::Mongodb, DatabaseMode::MANAGED) => {
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    database_instance_type,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?))
            }
            (CPKind::Aws, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?))
            }
            (CPKind::Azure, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => {
                todo!()
            }
            (CPKind::Azure, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
                todo!()
            }
            (CPKind::Azure, DatabaseKind::Mysql, DatabaseMode::MANAGED) => {
                todo!()
            }
            (CPKind::Azure, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
                todo!()
            }
            (CPKind::Azure, DatabaseKind::Mongodb, DatabaseMode::MANAGED) => {
                todo!()
            }
            (CPKind::Azure, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
                todo!()
            }
            (CPKind::Azure, DatabaseKind::Redis, DatabaseMode::MANAGED) => {
                todo!()
            }
            (CPKind::Azure, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
                todo!()
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    database_instance_type,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    database_instance_type,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
            (CPKind::OnPremise, DatabaseKind::Postgresql, DatabaseMode::MANAGED) => {
                Err(DatabaseError::UnsupportedManagedMode(
                    service::DatabaseType::PostgreSQL,
                    OnPremise::full_name().to_string(),
                ))
            }
            (CPKind::OnPremise, DatabaseKind::Postgresql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<OnPremise, Container, PostgresSQL>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::OnPremise, DatabaseKind::Mysql, DatabaseMode::MANAGED) => Err(
                DatabaseError::UnsupportedManagedMode(service::DatabaseType::MySQL, OnPremise::full_name().to_string()),
            ),
            (CPKind::OnPremise, DatabaseKind::Mysql, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<OnPremise, Container, MySQL>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::OnPremise, DatabaseKind::Mongodb, DatabaseMode::MANAGED) => {
                Err(DatabaseError::UnsupportedManagedMode(
                    service::DatabaseType::MongoDB,
                    OnPremise::full_name().to_string(),
                ))
            }
            (CPKind::OnPremise, DatabaseKind::Mongodb, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<OnPremise, Container, MongoDB>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?;

                Ok(Box::new(db))
            }
            (CPKind::OnPremise, DatabaseKind::Redis, DatabaseMode::MANAGED) => Err(
                DatabaseError::UnsupportedManagedMode(service::DatabaseType::Redis, OnPremise::full_name().to_string()),
            ),
            (CPKind::OnPremise, DatabaseKind::Redis, DatabaseMode::CONTAINER) => {
                let db = models::database::Database::<OnPremise, Container, Redis>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name.clone(),
                    version,
                    self.created_at,
                    self.fqdn.as_str(),
                    self.fqdn_id.as_str(),
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
                )?;

                Ok(Box::new(db))
            }
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    database_options.disk_size_in_gib,
                    None,
                    database_options.publicly_accessible,
                    database_options.port,
                    database_options,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    additional_annotations,
                    labels_groups,
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

#[derive(Clone, Eq, PartialEq)]
pub enum DiskIOPS {
    Default,
    Provisioned(u32),
}

impl DiskIOPS {
    pub fn value(&self) -> u32 {
        match self {
            DiskIOPS::Default => 0,
            DiskIOPS::Provisioned(iops) => *iops,
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
    pub database_disk_iops: DiskIOPS,
    pub encrypt_disk: bool,
    pub activate_high_availability: bool,
    pub activate_backups: bool,
    pub publicly_accessible: bool,
}
