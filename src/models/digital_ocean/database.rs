use crate::cloud_provider::service::{check_service_version, Service};
use crate::cloud_provider::{service, DeploymentTarget};
use crate::errors::EngineError;
use crate::io_models::database::DatabaseOptions;
use crate::models::database::{Container, Database, DatabaseType, MongoDB, MySQL, PostgresSQL, Redis};
use crate::models::database_utils::{
    get_self_hosted_mongodb_version, get_self_hosted_mysql_version, get_self_hosted_postgres_version,
    get_self_hosted_redis_version,
};
use crate::models::types::{ToTeraContext, DO};
use tera::Context as TeraContext;

/////////////////////////////////////////////////////////////////
// CONTAINER
impl DatabaseType<DO, Container> for PostgresSQL {
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
}

impl DatabaseType<DO, Container> for MySQL {
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
}

impl DatabaseType<DO, Container> for Redis {
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
}

impl DatabaseType<DO, Container> for MongoDB {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "Redis"
    }

    fn lib_directory_name() -> &'static str {
        "mongodb"
    }

    fn db_type() -> service::DatabaseType {
        service::DatabaseType::MongoDB
    }
}

/////////////////////////////////////////////////////////////////
// MANAGED
// DO don't support managed databases for now

////////////////////////////////////////////////////////////////////////:
// POSTGRES SQL
impl ToTeraContext for Database<DO, Container, PostgresSQL>
where
    PostgresSQL: DatabaseType<DO, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let _check_version = |event_details| {
            check_service_version(
                get_self_hosted_postgres_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// MySQL
impl ToTeraContext for Database<DO, Container, MySQL>
where
    MySQL: DatabaseType<DO, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let _check_version = |event_details| {
            check_service_version(
                get_self_hosted_mysql_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// MongoDB
impl ToTeraContext for Database<DO, Container, MongoDB>
where
    MongoDB: DatabaseType<DO, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let _check_version = |event_details| {
            check_service_version(
                get_self_hosted_mongodb_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };

        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// Redis
impl ToTeraContext for Database<DO, Container, Redis>
where
    Redis: DatabaseType<DO, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let _check_version = |event_details| {
            check_service_version(
                get_self_hosted_redis_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_tera_context_for_container(target, &self.options)
    }
}
