use tera::Context as TeraContext;

use crate::cloud_provider::service::{
    check_service_version, default_tera_context, delete_stateful_service, deploy_stateful_service, get_tfstate_name,
    get_tfstate_suffix, scale_down_database, send_progress_on_long_task, Action, Create, Database, DatabaseOptions,
    DatabaseType, Delete, Helm, Pause, Service, ServiceType, StatefulService, Terraform,
};
use crate::cloud_provider::utilities::{
    get_self_hosted_mysql_version, get_supported_version_to_use, print_action, sanitize_name, VersionsNumber,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl;
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope, StringError};
use crate::models::DatabaseMode::MANAGED;
use crate::models::{Context, Listen, Listener, Listeners};
use ::function_name::named;
use std::collections::HashMap;
use std::str::FromStr;

pub struct MySQL {
    context: Context,
    id: String,
    action: Action,
    name: String,
    version: VersionsNumber,
    fqdn: String,
    fqdn_id: String,
    total_cpus: String,
    total_ram_in_mib: u32,
    database_instance_type: String,
    options: DatabaseOptions,
    listeners: Listeners,
}

impl MySQL {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        version: VersionsNumber,
        fqdn: &str,
        fqdn_id: &str,
        total_cpus: String,
        total_ram_in_mib: u32,
        database_instance_type: &str,
        options: DatabaseOptions,
        listeners: Listeners,
    ) -> Self {
        Self {
            context,
            action,
            id: id.to_string(),
            name: name.to_string(),
            version,
            fqdn: fqdn.to_string(),
            fqdn_id: fqdn_id.to_string(),
            total_cpus,
            total_ram_in_mib,
            database_instance_type: database_instance_type.to_string(),
            options,
            listeners,
        }
    }

    fn matching_correct_version(&self, is_managed_services: bool) -> Result<VersionsNumber, EngineError> {
        let version = check_service_version(Self::pick_mysql_version(self.version(), is_managed_services), self)?;
        match VersionsNumber::from_str(version.as_str()) {
            Ok(res) => Ok(res),
            Err(e) => Err(self.engine_error(
                EngineErrorCause::Internal,
                format!("cannot parse database version, err: {}", e),
            )),
        }
    }

    fn pick_mysql_version(requested_version: String, is_managed_service: bool) -> Result<String, StringError> {
        if is_managed_service {
            Self::pick_managed_mysql_version(requested_version)
        } else {
            get_self_hosted_mysql_version(requested_version)
        }
    }

    fn pick_managed_mysql_version(requested_version: String) -> Result<String, StringError> {
        // Scaleway supported MySQL versions
        // https://api.scaleway.com/rdb/v1/regions/fr-par/database-engines
        let mut supported_mysql_versions = HashMap::new();

        // {"name": "MySQL", "version":"8","end_of_life":"2026-04-01T00:00:00Z"}
        supported_mysql_versions.insert("8".to_string(), "8".to_string());
        supported_mysql_versions.insert("8.0".to_string(), "8.0".to_string());

        get_supported_version_to_use("RDB MySQL", supported_mysql_versions, requested_version)
    }

    fn cloud_provider_name(&self) -> &str {
        "scaleway"
    }

    fn struct_name(&self) -> &str {
        "mysql"
    }
}

impl StatefulService for MySQL {
    fn is_managed_service(&self) -> bool {
        self.options.mode == MANAGED
    }
}

impl Service for MySQL {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Database(DatabaseType::MySQL(&self.options))
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn sanitized_name(&self) -> String {
        sanitize_name("mysql", self.name())
    }

    fn version(&self) -> String {
        self.version.to_string()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn private_port(&self) -> Option<u16> {
        Some(self.options.port)
    }

    fn start_timeout(&self) -> Timeout<u32> {
        Timeout::Value(600)
    }

    fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    fn cpu_burst(&self) -> String {
        unimplemented!()
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
        self.options.publicly_accessible
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let kubernetes = target.kubernetes;
        let environment = target.environment;

        let mut context = default_tera_context(self, kubernetes, environment);

        // we need the kubernetes config file to store tfstates file in kube secrets
        let kube_config_file_path = kubernetes.config_file_path()?;
        context.insert("kubeconfig_path", &kube_config_file_path);

        kubectl::kubectl_exec_create_namespace_without_labels(
            &environment.namespace(),
            kube_config_file_path.as_str(),
            kubernetes.cloud_provider().credentials_environment_variables(),
        );

        context.insert("namespace", environment.namespace());

        let version = &self.matching_correct_version(self.is_managed_service())?;
        context.insert("version_major", &version.to_major_version_string());
        context.insert("version", &version.to_string()); // Scaleway needs to have major version only

        for (k, v) in kubernetes.cloud_provider().tera_context_environment_variables() {
            context.insert(k, v);
        }

        context.insert("kubernetes_cluster_id", kubernetes.id());
        context.insert("kubernetes_cluster_name", kubernetes.name());

        context.insert("fqdn_id", self.fqdn_id.as_str());
        context.insert(
            "fqdn",
            self.fqdn(target, &self.fqdn, self.is_managed_service()).as_str(),
        );
        context.insert("database_login", self.options.login.as_str());
        context.insert("database_password", self.options.password.as_str());
        context.insert("database_port", &self.private_port());
        context.insert("database_disk_size_in_gib", &self.options.disk_size_in_gib);
        context.insert("database_instance_type", &self.database_instance_type);
        context.insert("database_disk_type", &self.options.database_disk_type);
        context.insert("database_name", &self.sanitized_name());
        context.insert("database_ram_size_in_mib", &self.total_ram_in_mib);
        context.insert("database_total_cpus", &self.total_cpus);
        context.insert("database_fqdn", &self.options.host.as_str());
        context.insert("database_id", &self.id());
        context.insert("tfstate_suffix_name", &get_tfstate_suffix(self));
        context.insert("tfstate_name", &get_tfstate_name(self));

        context.insert("publicly_accessible", &self.options.publicly_accessible);
        context.insert("activate_high_availability", &self.options.activate_high_availability);
        context.insert("activate_backups", &self.options.activate_backups);

        context.insert("delete_automated_backups", &self.context().is_test_cluster());
        context.insert("skip_final_snapshot", &self.context().is_test_cluster());
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }

        Ok(context)
    }

    fn selector(&self) -> Option<String> {
        Some(format!("app={}", self.sanitized_name()))
    }

    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Database(
            self.id().to_string(),
            self.service_type().name().to_string(),
            self.name().to_string(),
        )
    }
}

impl Database for MySQL {}

impl Helm for MySQL {
    fn helm_selector(&self) -> Option<String> {
        self.selector()
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("mysql-{}", self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/common/services/mysql", self.context.lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        format!("{}/scaleway/chart_values/mysql", self.context.lib_root_dir())
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        format!("{}/common/charts/external-name-svc", self.context.lib_root_dir())
    }
}

impl Terraform for MySQL {
    fn terraform_common_resource_dir_path(&self) -> String {
        format!("{}/scaleway/services/common", self.context.lib_root_dir())
    }

    fn terraform_resource_dir_path(&self) -> String {
        format!("{}/scaleway/services/mysql", self.context.lib_root_dir())
    }
}

impl Create for MySQL {
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_stateful_service(target, self)
        })
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        self.check_domains(self.listeners.clone(), vec![self.fqdn.as_str()])
    }

    #[named]
    fn on_create_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        Ok(())
    }
}

impl Pause for MySQL {
    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Pause, || {
            scale_down_database(target, self, 0)
        })
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        Ok(())
    }
}

impl Delete for MySQL {
    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
            delete_stateful_service(target, self)
        })
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_delete_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );
        Ok(())
    }
}

impl Listen for MySQL {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
