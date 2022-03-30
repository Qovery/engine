use crate::cloud_provider::service::{
    check_service_version, default_tera_context, get_tfstate_name, get_tfstate_suffix, DatabaseOptions, Service,
    ServiceVersionCheckResult,
};
use crate::cloud_provider::{service, DeploymentTarget};
use crate::cmd::kubectl;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::models::aws::database_utils::{
    get_managed_mongodb_version, get_managed_mysql_version, get_managed_postgres_version, get_managed_redis_version,
};
use crate::models::database::{
    Container, Database, DatabaseMode, DatabaseType, Managed, MongoDB, MySQL, PostgresSQL, Redis,
};
use crate::models::database_utils::{
    get_self_hosted_mongodb_version, get_self_hosted_mysql_version, get_self_hosted_postgres_version,
    get_self_hosted_redis_version,
};
use crate::models::types::{ToTeraContext, AWS};
use tera::Context as TeraContext;

/////////////////////////////////////////////////////////////////
// CONTAINER
impl DatabaseType<AWS, Container> for PostgresSQL {
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

impl DatabaseType<AWS, Container> for MySQL {
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

impl DatabaseType<AWS, Container> for Redis {
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

impl DatabaseType<AWS, Container> for MongoDB {
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
impl DatabaseType<AWS, Managed> for PostgresSQL {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "Postgres RDS"
    }
    fn lib_directory_name() -> &'static str {
        "postgresql"
    }

    fn db_type() -> service::DatabaseType {
        service::DatabaseType::PostgreSQL
    }
}

impl DatabaseType<AWS, Managed> for MySQL {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "MySQL RDS"
    }
    fn lib_directory_name() -> &'static str {
        "mysql"
    }

    fn db_type() -> service::DatabaseType {
        service::DatabaseType::MySQL
    }
}

impl DatabaseType<AWS, Managed> for Redis {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "ElasticCache"
    }
    fn lib_directory_name() -> &'static str {
        "redis"
    }

    fn db_type() -> service::DatabaseType {
        service::DatabaseType::Redis
    }
}

impl DatabaseType<AWS, Managed> for MongoDB {
    type DatabaseOptions = DatabaseOptions;

    fn short_name() -> &'static str {
        "DocumentDB"
    }

    fn lib_directory_name() -> &'static str {
        "mongodb"
    }

    fn db_type() -> service::DatabaseType {
        service::DatabaseType::MongoDB
    }
}

impl<M: DatabaseMode, T: DatabaseType<AWS, M>> Database<AWS, M, T> {
    fn to_aws_tera_context(
        &self,
        target: &DeploymentTarget,
        options: &DatabaseOptions,
        get_version: &dyn Fn(EventDetails) -> Result<ServiceVersionCheckResult, EngineError>,
    ) -> Result<TeraContext, EngineError>
    where
        Database<AWS, M, T>: Service,
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

        let version = get_version(event_details)?.matched_version().to_string();
        context.insert("version", &version);

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
        context.insert("encrypt_disk", &options.encrypt_disk);
        context.insert("database_ram_size_in_mib", &self.total_ram_in_mib);
        context.insert("database_total_cpus", &self.total_cpus);
        context.insert("database_fqdn", &options.host.as_str());
        context.insert("database_id", &self.id());
        context.insert("tfstate_suffix_name", &get_tfstate_suffix(self));
        context.insert("tfstate_name", &get_tfstate_name(self));
        context.insert("skip_final_snapshot", &false);
        context.insert("final_snapshot_name", &format!("qovery-{}-final-snap", self.id));
        context.insert("delete_automated_backups", &self.context().is_test_cluster());
        context.insert("publicly_accessible", &options.publicly_accessible);

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert("resource_expiration_in_seconds", &self.context.resource_expiration_in_seconds())
        }

        Ok(context)
    }
}

////////////////////////////////////////////////////////////////////////:
// POSTGRES SQL
impl ToTeraContext for Database<AWS, Managed, PostgresSQL>
where
    PostgresSQL: DatabaseType<AWS, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                get_managed_postgres_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_aws_tera_context(target, &self.options, &check_version)
    }
}
impl ToTeraContext for Database<AWS, Container, PostgresSQL>
where
    PostgresSQL: DatabaseType<AWS, Container>,
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
        self.to_aws_tera_context(target, &self.options, &check_version)
    }
}

////////////////////////////////////////////////////////////////////////:
// MySQL
impl ToTeraContext for Database<AWS, Managed, MySQL>
where
    MySQL: DatabaseType<AWS, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                get_managed_mysql_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_aws_tera_context(target, &self.options, &check_version)
    }
}
impl ToTeraContext for Database<AWS, Container, MySQL>
where
    MySQL: DatabaseType<AWS, Container>,
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
        self.to_aws_tera_context(target, &self.options, &check_version)
    }
}

impl ToTeraContext for Database<AWS, Managed, MongoDB>
where
    MongoDB: DatabaseType<AWS, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                get_managed_mongodb_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_aws_tera_context(target, &self.options, &check_version)
    }
}
impl ToTeraContext for Database<AWS, Container, MongoDB>
where
    MongoDB: DatabaseType<AWS, Container>,
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

        self.to_aws_tera_context(target, &self.options, &check_version)
    }
}

impl ToTeraContext for Database<AWS, Managed, Redis>
where
    Redis: DatabaseType<AWS, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let check_version = |event_details| {
            check_service_version(
                get_managed_redis_version(self.version.to_string()),
                self,
                event_details,
                self.logger(),
            )
        };
        self.to_aws_tera_context(target, &self.options, &check_version)
    }
}
impl ToTeraContext for Database<AWS, Container, Redis>
where
    Redis: DatabaseType<AWS, Container>,
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
        self.to_aws_tera_context(target, &self.options, &check_version)
    }
}
