use crate::cloud_provider::service::{
    check_service_version, default_tera_context, get_tfstate_name, get_tfstate_suffix, DatabaseOptions, Service,
    ServiceVersionCheckResult,
};
use crate::cloud_provider::{service, DeploymentTarget};
use crate::cmd::kubectl;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::models::database::{
    Container, Database, DatabaseMode, DatabaseType, Managed, MongoDB, MySQL, PostgresSQL, Redis,
};
use crate::models::database_utils::{
    get_self_hosted_mongodb_version, get_self_hosted_mysql_version, get_self_hosted_postgres_version,
    get_self_hosted_redis_version,
};
use crate::models::scaleway::database_utils::{pick_managed_mysql_version, pick_managed_postgres_version};
use crate::models::types::{ToTeraContext, SCW};
use tera::Context as TeraContext;

/////////////////////////////////////////////////////////////////
// CONTAINER
impl DatabaseType<SCW, Container> for PostgresSQL {
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

impl DatabaseType<SCW, Container> for MySQL {
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

impl DatabaseType<SCW, Container> for Redis {
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

impl DatabaseType<SCW, Container> for MongoDB {
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
impl DatabaseType<SCW, Managed> for PostgresSQL {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "Postgres Managed"
    }
    fn lib_directory_name() -> &'static str {
        "postgresql"
    }

    fn db_type() -> service::DatabaseType {
        service::DatabaseType::PostgreSQL
    }
}

impl DatabaseType<SCW, Managed> for MySQL {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "MySQL Managed"
    }
    fn lib_directory_name() -> &'static str {
        "mysql"
    }

    fn db_type() -> service::DatabaseType {
        service::DatabaseType::MySQL
    }
}

// Redis and MongoDB are not supported managed db yet

impl<M: DatabaseMode, T: DatabaseType<SCW, M>> Database<SCW, M, T> {
    fn to_scaleway_tera_context(
        &self,
        target: &DeploymentTarget,
        options: &DatabaseOptions,
        get_version: &dyn Fn(EventDetails) -> Result<ServiceVersionCheckResult, EngineError>,
    ) -> Result<TeraContext, EngineError>
    where
        Database<SCW, M, T>: Service,
    {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));
        let kubernetes = target.kubernetes;
        let environment = target.environment;

        let mut context = default_tera_context(self, kubernetes, environment);

        // we need the kubernetes config file to store tfstates file in kube secrets
        let kube_config_file_path = kubernetes.get_kubeconfig_file_path()?;
        context.insert("kubeconfig_path", &kube_config_file_path);

        kubectl::kubectl_exec_create_namespace_without_labels(
            environment.namespace(),
            kube_config_file_path.as_str(),
            kubernetes.cloud_provider().credentials_environment_variables(),
        );

        context.insert("namespace", environment.namespace());

        let version = get_version(event_details)?.matched_version();
        context.insert("version_major", &version.to_major_version_string());
        context.insert("version", &version.to_string()); // Scaleway needs to have major version only

        for (k, v) in kubernetes.cloud_provider().tera_context_environment_variables() {
            context.insert(k, v);
        }

        context.insert("kubernetes_cluster_id", kubernetes.id());
        context.insert("kubernetes_cluster_name", kubernetes.name());

        context.insert("fqdn_id", self.fqdn_id.as_str());
        context.insert("fqdn", self.fqdn(target, &self.fqdn, M::is_managed()).as_str());
        context.insert("service_name", self.fqdn_id.as_str());
        context.insert("database_name", self.sanitized_name().as_str());
        context.insert("database_db_name", self.name());
        context.insert("database_login", options.login.as_str());
        context.insert("database_password", options.password.as_str());
        context.insert("database_port", &self.private_port());
        context.insert("database_disk_size_in_gib", &options.disk_size_in_gib);
        context.insert("database_instance_type", &self.database_instance_type);
        context.insert("database_disk_type", &options.database_disk_type);
        context.insert("database_ram_size_in_mib", &self.total_ram_in_mib);
        context.insert("database_total_cpus", &self.total_cpus);
        context.insert("database_fqdn", &options.host.as_str());
        context.insert("database_id", &self.id());
        context.insert("tfstate_suffix_name", &get_tfstate_suffix(self));
        context.insert("tfstate_name", &get_tfstate_name(self));

        context.insert("publicly_accessible", &options.publicly_accessible);
        context.insert("activate_high_availability", &options.activate_high_availability);
        context.insert("activate_backups", &options.activate_backups);
        context.insert("delete_automated_backups", &self.context().is_test_cluster());
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert("resource_expiration_in_seconds", &self.context.resource_expiration_in_seconds())
        }

        Ok(context)
    }
}

////////////////////////////////////////////////////////////////////////:
// POSTGRES SQL
impl ToTeraContext for Database<SCW, Managed, PostgresSQL>
where
    PostgresSQL: DatabaseType<SCW, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                pick_managed_postgres_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_scaleway_tera_context(target, &self.options, &check_version)
    }
}

impl ToTeraContext for Database<SCW, Container, PostgresSQL>
where
    PostgresSQL: DatabaseType<SCW, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                get_self_hosted_postgres_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_scaleway_tera_context(target, &self.options, &check_version)
    }
}

////////////////////////////////////////////////////////////////////////:
// MySQL
impl ToTeraContext for Database<SCW, Managed, MySQL>
where
    MySQL: DatabaseType<SCW, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                pick_managed_mysql_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_scaleway_tera_context(target, &self.options, &check_version)
    }
}

impl ToTeraContext for Database<SCW, Container, MySQL>
where
    MySQL: DatabaseType<SCW, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                get_self_hosted_mysql_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_scaleway_tera_context(target, &self.options, &check_version)
    }
}

////////////////////////////////////////////////////////////////////////:
// MongoDB
impl ToTeraContext for Database<SCW, Container, MongoDB>
where
    MongoDB: DatabaseType<SCW, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                get_self_hosted_mongodb_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };

        self.to_scaleway_tera_context(target, &self.options, &check_version)
    }
}

////////////////////////////////////////////////////////////////////////:
// Redis
impl ToTeraContext for Database<SCW, Container, Redis>
where
    Redis: DatabaseType<SCW, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                get_self_hosted_redis_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_scaleway_tera_context(target, &self.options, &check_version)
    }
}
