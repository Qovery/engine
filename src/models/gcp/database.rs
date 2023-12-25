#![allow(clippy::redundant_closure)]

use crate::cloud_provider::{service, DeploymentTarget};
use crate::errors::EngineError;
use crate::models::database::{Container, Database, DatabaseType, MongoDB, MySQL, PostgresSQL, Redis};

use crate::io_models::database::DatabaseOptions;
use crate::models::types::{ToTeraContext, GCP};
use crate::unit_conversion::cpu_string_to_float;
use tera::Context as TeraContext;

/////////////////////////////////////////////////////////////////
// CONTAINER
impl DatabaseType<GCP, Container> for PostgresSQL {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "PostgresSQL"
    }
    fn lib_directory_name() -> &'static str {
        "postgresql"
    }
    fn db_type() -> service::DatabaseType {
        service::DatabaseType::PostgreSQL
    }

    fn cpu_validate(desired_cpu: String) -> String {
        // todo: update core side to avoid passing String and keep u32 #ENG-1277
        let cpu_size = cpu_string_to_float(desired_cpu.clone());
        if cpu_size < 0.25 {
            // todo: return an error instead?
            "250m".to_string()
        } else {
            desired_cpu
        }
    }

    fn memory_validate(desired_memory: u32) -> u32 {
        if desired_memory < 100 {
            // todo: return an error instead?
            100
        } else {
            desired_memory
        }
    }
}

impl DatabaseType<GCP, Container> for MySQL {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "MySQL"
    }
    fn lib_directory_name() -> &'static str {
        "mysql"
    }
    fn db_type() -> service::DatabaseType {
        service::DatabaseType::MySQL
    }

    fn cpu_validate(desired_cpu: String) -> String {
        // todo: update core side to avoid passing String and keep u32 #ENG-1277
        let cpu_size = cpu_string_to_float(desired_cpu.clone());
        if cpu_size < 0.25 {
            // todo: return an error instead?
            "250m".to_string()
        } else {
            desired_cpu
        }
    }

    // lower than 500m, it's too long to start and fails. Better to allow cpu overcommit than growing init boot value
    fn cpu_burst_value(desired_cpu: String) -> String {
        // todo: update core side to avoid passing String and keep u32 #ENG-1277
        let cpu_size = cpu_string_to_float(desired_cpu.clone());
        if cpu_size < 0.5 {
            // todo: return an error instead?
            "500m".to_string()
        } else {
            desired_cpu
        }
    }

    fn memory_validate(desired_memory: u32) -> u32 {
        if desired_memory < 100 {
            // todo: return an error instead?
            100
        } else {
            desired_memory
        }
    }
}

impl DatabaseType<GCP, Container> for Redis {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "Redis"
    }
    fn lib_directory_name() -> &'static str {
        "redis"
    }
    fn db_type() -> service::DatabaseType {
        service::DatabaseType::Redis
    }

    fn cpu_validate(desired_cpu: String) -> String {
        // todo: update core side to avoid passing String and keep u32 #ENG-1277
        let cpu_size = cpu_string_to_float(desired_cpu.clone());
        if cpu_size < 0.25 {
            // todo: return an error instead?
            "250m".to_string()
        } else {
            desired_cpu
        }
    }

    fn memory_validate(desired_memory: u32) -> u32 {
        if desired_memory < 100 {
            // todo: return an error instead?
            100
        } else {
            desired_memory
        }
    }
}

impl DatabaseType<GCP, Container> for MongoDB {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "MongoDb"
    }
    fn lib_directory_name() -> &'static str {
        "mongodb"
    }
    fn db_type() -> service::DatabaseType {
        service::DatabaseType::MongoDB
    }

    fn cpu_validate(desired_cpu: String) -> String {
        // todo: update core side to avoid passing String and keep u32 #ENG-1277
        let cpu_size = cpu_string_to_float(desired_cpu.clone());
        if cpu_size < 0.25 {
            // todo: return an error instead?
            "250m".to_string()
        } else {
            desired_cpu
        }
    }

    fn memory_validate(desired_memory: u32) -> u32 {
        if desired_memory < 256 {
            // todo: return an error instead?
            256
        } else {
            desired_memory
        }
    }
}

////////////////////////////////////////////////////////////////////////:
// POSTGRES SQL
impl ToTeraContext for Database<GCP, Container, PostgresSQL>
where
    PostgresSQL: DatabaseType<GCP, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// MySQL
impl ToTeraContext for Database<GCP, Container, MySQL>
where
    MySQL: DatabaseType<GCP, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// MongoDB
impl ToTeraContext for Database<GCP, Container, MongoDB>
where
    MongoDB: DatabaseType<GCP, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// Redis
impl ToTeraContext for Database<GCP, Container, Redis>
where
    Redis: DatabaseType<GCP, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_container(target, &self.options)
    }
}
