use crate::cloud_provider::service::{
    check_service_version, default_tera_context, Action, Service, ServiceType, ServiceVersionCheckResult,
};
use crate::cloud_provider::utilities::managed_db_name_sanitizer;
use crate::cloud_provider::{service, DeploymentTarget};
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::database::DatabaseOptions;
use crate::models::database_utils::{
    get_self_hosted_mongodb_version, get_self_hosted_mysql_version, get_self_hosted_postgres_version,
    get_self_hosted_redis_version,
};
use crate::models::types::{CloudProvider, ToTeraContext, VersionsNumber};
use crate::utilities::to_short_id;
use std::marker::PhantomData;
use tera::Context as TeraContext;
use uuid::Uuid;

/////////////////////////////////////////////////////////////////
// Database mode
pub struct Managed {}
pub struct Container {}
pub trait DatabaseMode {
    fn is_managed() -> bool;
    fn is_container() -> bool {
        !Self::is_managed()
    }
}

impl DatabaseMode for Managed {
    fn is_managed() -> bool {
        true
    }
}

impl DatabaseMode for Container {
    fn is_managed() -> bool {
        false
    }
}

/////////////////////////////////////////////////////////////////
// Database types, will be only used as a marker
pub struct PostgresSQL {}
pub struct MySQL {}
pub struct MongoDB {}
pub struct Redis {}

pub trait DatabaseType<T: CloudProvider, M: DatabaseMode> {
    type DatabaseOptions;

    fn short_name() -> &'static str;
    fn lib_directory_name() -> &'static str;
    fn db_type() -> service::DatabaseType;
    // autocorrect resources if needed
    fn cpu_validate(desired_cpu: String) -> String {
        desired_cpu
    }
    fn cpu_burst_value(desired_cpu: String) -> String {
        desired_cpu
    }
    fn memory_validate(desired_memory: u32) -> u32 {
        desired_memory
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DatabaseError {
    #[error("Database invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Managed database for {0:?} is not supported (yet) by provider {1}")]
    UnsupportedManagedMode(service::DatabaseType, String),
}

pub struct Database<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> {
    _marker: PhantomData<(C, M, T)>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails>,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) action: Action,
    pub(crate) name: String,
    pub(crate) version: VersionsNumber,
    pub(crate) fqdn: String,
    pub(crate) fqdn_id: String,
    pub(crate) total_cpus: String,
    pub(crate) total_ram_in_mib: u32,
    pub(crate) database_instance_type: String,
    pub(crate) publicly_accessible: bool,
    pub(crate) private_port: u16,
    pub(crate) options: T::DatabaseOptions,
    pub(crate) workspace_directory: String,
    pub(crate) lib_root_directory: String,
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Database<C, M, T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        action: Action,
        name: &str,
        version: VersionsNumber,
        fqdn: &str,
        fqdn_id: &str,
        total_cpus: String,
        total_ram_in_mib: u32,
        database_instance_type: &str,
        publicly_accessible: bool,
        private_port: u16,
        options: T::DatabaseOptions,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, DatabaseError> {
        // TODO: Implement domain constraint logic

        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("databases/{}", long_id),
        )
        .map_err(|_| DatabaseError::InvalidConfig("Can't create workspace directory".to_string()))?;

        let event_details = mk_event_details(Transmitter::Database(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            action,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            version,
            fqdn: fqdn.to_string(),
            fqdn_id: fqdn_id.to_string(),
            total_cpus: T::cpu_validate(total_cpus),
            total_ram_in_mib: T::memory_validate(total_ram_in_mib),
            database_instance_type: database_instance_type.to_string(),
            publicly_accessible,
            private_port,
            options,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
    }

    pub fn selector(&self) -> String {
        format!("databaseId={}", self.id)
    }

    pub fn workspace_directory(&self) -> &str {
        &self.workspace_directory
    }

    pub(super) fn fqdn(&self, target: &DeploymentTarget, fqdn: &str) -> String {
        match &self.publicly_accessible {
            true => fqdn.to_string(),
            false => match M::is_managed() {
                true => format!("{}-dns.{}.svc.cluster.local", self.id(), target.environment.namespace()),
                false => format!("{}.{}.svc.cluster.local", self.sanitized_name(), target.environment.namespace()),
            },
        }
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Service for Database<C, M, T> {
    fn service_type(&self) -> ServiceType {
        ServiceType::Database(T::db_type())
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn sanitized_name(&self) -> String {
        // FIXME: specific case only for aws ;'(
        // This is sad, but can't change that as it would break/wipe all container db for users
        // AWS and AWS-EC2
        if C::lib_directory_name().starts_with("aws") {
            managed_db_name_sanitizer(60, T::lib_directory_name(), &self.id)
        } else {
            format!("{}-{}", T::lib_directory_name(), &self.id)
        }
    }

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn selector(&self) -> Option<String> {
        Some(self.selector())
    }

    fn as_service(&self) -> &dyn Service {
        self
    }
}

// Mzthod Only For all container database
impl<C: CloudProvider, T: DatabaseType<C, Container>> Database<C, Container, T> {
    pub fn helm_release_name(&self) -> String {
        format!("{}-{}", T::lib_directory_name(), self.id)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/services/{}", self.lib_root_directory, T::lib_directory_name())
    }

    pub fn helm_chart_values_dir(&self) -> String {
        format!(
            "{}/{}/chart_values/{}",
            self.lib_root_directory,
            C::lib_directory_name(),
            T::lib_directory_name()
        )
    }

    pub(super) fn to_tera_context_for_container(
        &self,
        target: &DeploymentTarget,
        options: &DatabaseOptions,
    ) -> Result<TeraContext, EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);

        // we need the kubernetes config file to store tfstates file in kube secrets
        let kube_config_file_path = kubernetes.get_kubeconfig_file_path()?;
        context.insert("kubeconfig_path", &kube_config_file_path);
        context.insert("namespace", environment.namespace());

        let version = self.get_version(event_details)?.matched_version().to_string();
        context.insert("version", &version);

        for (k, v) in kubernetes.cloud_provider().tera_context_environment_variables() {
            context.insert(k, v);
        }

        context.insert("kubernetes_cluster_id", kubernetes.id());
        context.insert("kubernetes_cluster_name", kubernetes.name());

        context.insert("fqdn_id", self.fqdn_id.as_str());
        context.insert("fqdn", self.fqdn(target, &self.fqdn).as_str());
        context.insert("service_name", self.fqdn_id.as_str());
        context.insert("database_db_name", &self.name);
        context.insert("database_login", options.login.as_str());
        context.insert("database_password", options.password.as_str());
        context.insert("database_port", &self.private_port);
        context.insert("database_disk_size_in_gib", &options.disk_size_in_gib);
        context.insert("database_instance_type", &self.database_instance_type);
        context.insert("database_disk_type", &options.database_disk_type);
        context.insert("database_ram_size_in_mib", &self.total_ram_in_mib);
        context.insert("database_total_cpus", &self.total_cpus);
        context.insert("database_total_cpus_burst", &T::cpu_burst_value(self.total_cpus.clone()));
        context.insert("database_fqdn", &options.host.as_str());
        context.insert("database_id", &self.id());
        context.insert("publicly_accessible", &self.publicly_accessible);

        context.insert(
            "resource_expiration_in_seconds",
            &kubernetes.advanced_settings().pleco_resources_ttl,
        );

        Ok(context)
    }

    fn get_version(&self, event_details: EventDetails) -> Result<ServiceVersionCheckResult, EngineError> {
        let fn_version = match T::db_type() {
            service::DatabaseType::PostgreSQL => get_self_hosted_postgres_version,
            service::DatabaseType::MongoDB => get_self_hosted_mongodb_version,
            service::DatabaseType::MySQL => get_self_hosted_mysql_version,
            service::DatabaseType::Redis => get_self_hosted_redis_version,
        };

        check_service_version(fn_version(self.version.to_string()), self, event_details)
    }
}

// methods for all Managed databases
impl<C: CloudProvider, T: DatabaseType<C, Managed>> Database<C, Managed, T> {
    pub fn helm_chart_external_name_service_dir(&self) -> String {
        format!("{}/common/charts/external-name-svc", self.lib_root_directory)
    }

    pub fn terraform_common_resource_dir_path(&self) -> String {
        format!("{}/{}/services/common", self.lib_root_directory, C::lib_directory_name())
    }

    pub fn terraform_resource_dir_path(&self) -> String {
        format!(
            "{}/{}/services/{}",
            self.lib_root_directory,
            C::lib_directory_name(),
            T::lib_directory_name()
        )
    }
}

pub trait DatabaseService: Service + DeploymentAction + ToTeraContext {
    fn is_managed_service(&self) -> bool;

    fn db_type(&self) -> service::DatabaseType;

    fn version(&self) -> String;

    fn as_deployment_action(&self) -> &dyn DeploymentAction;
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> DatabaseService for Database<C, M, T>
where
    Database<C, M, T>: Service + DeploymentAction + ToTeraContext,
{
    fn is_managed_service(&self) -> bool {
        M::is_managed()
    }

    fn db_type(&self) -> service::DatabaseType {
        T::db_type()
    }

    fn version(&self) -> String {
        self.version.to_string()
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }
}
