use crate::cloud_provider::service::{
    check_service_version, default_tera_context, get_tfstate_name, get_tfstate_suffix, Service,
    ServiceVersionCheckResult,
};
use crate::cloud_provider::{service, DeploymentTarget};
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::models::database::{Container, Database, DatabaseType, Managed, MongoDB, MySQL, PostgresSQL, Redis};

use crate::io_models::database::DatabaseOptions;
use crate::models::aws_ec2::database_utils::{
    is_allowed_managed_mongodb_version, is_allowed_managed_mysql_version, is_allowed_managed_postgres_version,
    is_allowed_managed_redis_version,
};
use crate::models::types::{AWSEc2, ToTeraContext};
use crate::unit_conversion::cpu_string_to_float;
use tera::Context as TeraContext;

/////////////////////////////////////////////////////////////////
// CONTAINER
impl DatabaseType<AWSEc2, Container> for PostgresSQL {
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

impl DatabaseType<AWSEc2, Container> for MySQL {
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

impl DatabaseType<AWSEc2, Container> for Redis {
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

impl DatabaseType<AWSEc2, Container> for MongoDB {
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

/////////////////////////////////////////////////////////////////
// MANAGED
impl DatabaseType<AWSEc2, Managed> for PostgresSQL {
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

impl DatabaseType<AWSEc2, Managed> for MySQL {
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

impl DatabaseType<AWSEc2, Managed> for Redis {
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

impl DatabaseType<AWSEc2, Managed> for MongoDB {
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

impl<T: DatabaseType<AWSEc2, Managed>> Database<AWSEc2, Managed, T>
where
    Database<AWSEc2, Managed, T>: Service,
{
    fn get_version_aws_managed(
        &self,
        event_details: EventDetails,
    ) -> Result<ServiceVersionCheckResult, Box<EngineError>> {
        let fn_version = match T::db_type() {
            service::DatabaseType::PostgreSQL => is_allowed_managed_postgres_version,
            service::DatabaseType::MongoDB => is_allowed_managed_mongodb_version,
            service::DatabaseType::MySQL => is_allowed_managed_mysql_version,
            service::DatabaseType::Redis => is_allowed_managed_redis_version,
        };

        check_service_version(
            fn_version(&self.version)
                .map(|_| self.version.to_string())
                .map_err(CommandError::from),
            self,
            event_details,
        )
    }

    fn to_tera_context_for_aws_managed(
        &self,
        target: &DeploymentTarget,
        options: &DatabaseOptions,
    ) -> Result<TeraContext, Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);

        // we need the kubernetes config file to store tfstates file in kube secrets
        let kube_config_file_path = kubernetes.get_kubeconfig_file_path()?;
        context.insert("kubeconfig_path", &kube_config_file_path);
        context.insert("namespace", environment.namespace());

        let version = self
            .get_version_aws_managed(event_details)?
            .matched_version()
            .to_string();
        context.insert("version", &version);

        // Specific to mysql
        if T::db_type() == service::DatabaseType::MySQL {
            context.insert(
                "parameter_group_family",
                &format!(
                    "mysql{}.{}",
                    self.version.major,
                    self.version.minor.as_deref().unwrap_or_default()
                ),
            );
        }

        // Specific for redis
        if T::db_type() == service::DatabaseType::Redis {
            let parameter_group_name = if self.version.major == "5" {
                "default.redis5.0"
            } else if self.version.major == "6" {
                "default.redis6.x"
            } else if self.version.major == "7" {
                "default.redis7"
            } else {
                "redis.unknown"
            };

            context.insert("database_elasticache_parameter_group_name", parameter_group_name);
            context.insert("database_elasticache_instances_number", &1);
        }

        for (k, v) in kubernetes.cloud_provider().tera_context_environment_variables() {
            context.insert(k, v);
        }

        context.insert("kubernetes_cluster_id", kubernetes.id());
        context.insert("kubernetes_cluster_name", kubernetes.name());
        context.insert("fqdn_id", self.fqdn_id.as_str());
        context.insert("fqdn", self.fqdn(target, &self.fqdn).as_str());
        context.insert("service_name", self.fqdn_id.as_str());
        context.insert("database_name", self.sanitized_name().as_str());
        context.insert("database_db_name", self.name());
        context.insert("database_login", options.login.as_str());
        context.insert("database_password", options.password.as_str());
        context.insert("database_port", &self.private_port);
        context.insert("database_disk_size_in_gib", &options.disk_size_in_gib);
        if let Some(i) = &self.database_instance_type {
            context.insert("database_instance_type", i.to_cloud_provider_format().as_str());
        }
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
        context.insert("delete_automated_backups", &target.kubernetes.context().is_test_cluster());
        context.insert("publicly_accessible", &options.publicly_accessible);

        context.insert(
            "resource_expiration_in_seconds",
            &kubernetes.advanced_settings().pleco_resources_ttl,
        );

        Ok(context)
    }
}

////////////////////////////////////////////////////////////////////////:
// POSTGRES SQL
impl ToTeraContext for Database<AWSEc2, Managed, PostgresSQL>
where
    PostgresSQL: DatabaseType<AWSEc2, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_aws_managed(target, &self.options)
    }
}

impl ToTeraContext for Database<AWSEc2, Container, PostgresSQL>
where
    PostgresSQL: DatabaseType<AWSEc2, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// MySQL
impl ToTeraContext for Database<AWSEc2, Managed, MySQL>
where
    MySQL: DatabaseType<AWSEc2, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_aws_managed(target, &self.options)
    }
}

impl ToTeraContext for Database<AWSEc2, Container, MySQL>
where
    MySQL: DatabaseType<AWSEc2, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// MongoDB
impl ToTeraContext for Database<AWSEc2, Managed, MongoDB>
where
    MongoDB: DatabaseType<AWSEc2, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_aws_managed(target, &self.options)
    }
}

impl ToTeraContext for Database<AWSEc2, Container, MongoDB>
where
    MongoDB: DatabaseType<AWSEc2, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_container(target, &self.options)
    }
}

////////////////////////////////////////////////////////////////////////:
// Redis
impl ToTeraContext for Database<AWSEc2, Managed, Redis>
where
    Redis: DatabaseType<AWSEc2, Managed>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_aws_managed(target, &self.options)
    }
}

impl ToTeraContext for Database<AWSEc2, Container, Redis>
where
    Redis: DatabaseType<AWSEc2, Container>,
{
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.to_tera_context_for_container(target, &self.options)
    }
}
