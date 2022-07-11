use crate::cloud_provider::service::{
    check_service_version, default_tera_context, delete_stateful_service, deploy_database_service, get_tfstate_name,
    get_tfstate_suffix, scale_down_database, Action, Create, DatabaseOptions, DatabaseService, Delete, Helm, Pause,
    Service, ServiceType, ServiceVersionCheckResult, Terraform,
};
use crate::cloud_provider::utilities::{check_domain_for, managed_db_name_sanitizer, print_action};
use crate::cloud_provider::{service, DeploymentTarget};
use crate::deployment_report::database::reporter::DatabaseDeploymentReporter;
use crate::deployment_report::execute_long_deployment;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter, Transmitter};
use crate::io_models::{ApplicationAdvancedSettings, Context, Listen, Listener, Listeners, ListenersHelper};
use crate::logger::Logger;
use crate::models::database_utils::{
    get_self_hosted_mongodb_version, get_self_hosted_mysql_version, get_self_hosted_postgres_version,
    get_self_hosted_redis_version,
};
use crate::models::types::{CloudProvider, ToTeraContext, VersionsNumber};
use crate::utilities::to_short_id;
use function_name::named;
use std::borrow::Borrow;
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
    pub(super) context: Context,
    pub(super) id: String,
    pub(super) long_id: Uuid,
    pub(super) action: Action,
    pub(super) name: String,
    pub(super) version: VersionsNumber,
    pub(super) fqdn: String,
    pub(super) fqdn_id: String,
    pub(super) total_cpus: String,
    pub(super) total_ram_in_mib: u32,
    pub(super) database_instance_type: String,
    pub(super) publicly_accessible: bool,
    pub(super) private_port: u16,
    pub(super) options: T::DatabaseOptions,
    pub(super) listeners: Listeners,
    pub(super) logger: Box<dyn Logger>,
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Database<C, M, T> {
    pub fn new(
        context: Context,
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
        listeners: Listeners,
        logger: Box<dyn Logger>,
    ) -> Result<Self, DatabaseError> {
        // TODO: Implement domain constraint logic

        Ok(Self {
            _marker: PhantomData,
            context,
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
            listeners,
            logger,
        })
    }

    fn selector(&self) -> String {
        format!("databaseId={}", self.id)
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Terraform for Database<C, M, T> {
    fn terraform_common_resource_dir_path(&self) -> String {
        format!("{}/{}/services/common", self.context.lib_root_dir(), C::lib_directory_name())
    }

    fn terraform_resource_dir_path(&self) -> String {
        format!(
            "{}/{}/services/{}",
            self.context.lib_root_dir(),
            C::lib_directory_name(),
            T::lib_directory_name()
        )
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Listen for Database<C, M, T> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> ToTransmitter for Database<C, M, T> {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::Database(self.id.to_string(), T::short_name().to_string(), self.name.to_string())
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Service for Database<C, M, T>
where
    Database<C, M, T>: ToTeraContext,
{
    fn context(&self) -> &Context {
        &self.context
    }

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

    fn application_advanced_settings(&self) -> Option<ApplicationAdvancedSettings> {
        None
    }

    fn version(&self) -> String {
        self.version.to_string()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn private_port(&self) -> Option<u16> {
        Some(self.private_port)
    }

    fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    fn cpu_burst(&self) -> String {
        self.total_cpus.to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    fn min_instances(&self) -> u32 {
        1
    }

    fn max_instances(&self) -> u32 {
        1
    }

    fn publicly_accessible(&self) -> bool {
        self.publicly_accessible
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        self.to_tera_context(target)
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn selector(&self) -> Option<String> {
        Some(self.selector())
    }
}

impl<Cloud: CloudProvider, M: DatabaseMode, DbType: DatabaseType<Cloud, M>> Helm for Database<Cloud, M, DbType> {
    fn helm_selector(&self) -> Option<String> {
        Some(self.selector())
    }

    fn helm_release_name(&self) -> String {
        format!("{}-{}", DbType::lib_directory_name(), self.id)
    }

    fn helm_chart_dir(&self) -> String {
        format!(
            "{}/common/services/{}",
            self.context.lib_root_dir(),
            DbType::lib_directory_name()
        )
    }

    fn helm_chart_values_dir(&self) -> String {
        format!(
            "{}/{}/chart_values/{}",
            self.context.lib_root_dir(),
            Cloud::lib_directory_name(),
            DbType::lib_directory_name()
        )
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        format!("{}/common/charts/external-name-svc", self.context.lib_root_dir())
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Create for Database<C, M, T>
where
    Database<C, M, T>: ToTeraContext,
{
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Create), || {
            deploy_database_service(target, self, event_details.clone(), self.logger())
        })
    }

    #[named]
    fn on_create_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        if self.publicly_accessible {
            check_domain_for(
                ListenersHelper::new(&self.listeners),
                vec![&self.fqdn],
                self.context.execution_id(),
                self.context.execution_id(),
                event_details,
                self.logger(),
            )?;
        }
        Ok(())
    }

    #[named]
    fn on_create_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        Ok(())
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Pause for Database<C, M, T>
where
    Database<C, M, T>: ToTeraContext,
{
    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Pause), || {
            scale_down_database(target, self, 0)
        })
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        Ok(())
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Delete for Database<C, M, T>
where
    Database<C, M, T>: ToTeraContext,
{
    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Delete), || {
            delete_stateful_service(target, self, event_details.clone(), self.logger())
        })
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_delete_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        Ok(())
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> DatabaseService for Database<C, M, T>
where
    Database<C, M, T>: ToTeraContext,
{
    fn is_managed_service(&self) -> bool {
        M::is_managed()
    }

    fn db_type(&self) -> service::DatabaseType {
        T::db_type()
    }

    fn as_service(&self) -> &dyn Service {
        self
    }
}

impl<C: CloudProvider, M: DatabaseMode, T: DatabaseType<C, M>> Database<C, M, T>
where
    Database<C, M, T>: Service,
{
    fn get_version(&self, event_details: EventDetails) -> Result<ServiceVersionCheckResult, EngineError> {
        let fn_version = match T::db_type() {
            service::DatabaseType::PostgreSQL => get_self_hosted_postgres_version,
            service::DatabaseType::MongoDB => get_self_hosted_mongodb_version,
            service::DatabaseType::MySQL => get_self_hosted_mysql_version,
            service::DatabaseType::Redis => get_self_hosted_redis_version,
        };

        check_service_version(fn_version(self.version.to_string()), self, event_details, self.logger())
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
        context.insert("fqdn", self.fqdn(target, &self.fqdn, M::is_managed()).as_str());
        context.insert("service_name", self.fqdn_id.as_str());
        context.insert("database_db_name", self.name());
        context.insert("database_login", options.login.as_str());
        context.insert("database_password", options.password.as_str());
        context.insert("database_port", &self.private_port());
        context.insert("database_disk_size_in_gib", &options.disk_size_in_gib);
        context.insert("database_instance_type", &self.database_instance_type);
        context.insert("database_disk_type", &options.database_disk_type);
        context.insert("database_ram_size_in_mib", &self.total_ram_in_mib);
        context.insert("database_total_cpus", &self.total_cpus);
        context.insert("database_total_cpus_burst", &T::cpu_burst_value(self.total_cpus.clone()));
        context.insert("database_fqdn", &options.host.as_str());
        context.insert("database_id", &self.id());
        context.insert("tfstate_suffix_name", &get_tfstate_suffix(self));
        context.insert("tfstate_name", &get_tfstate_name(self));
        context.insert("publicly_accessible", &self.publicly_accessible);

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert("resource_expiration_in_seconds", &self.context.resource_expiration_in_seconds())
        }

        Ok(context)
    }
}
