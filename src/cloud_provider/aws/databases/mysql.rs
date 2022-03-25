use std::collections::HashMap;

use tera::Context as TeraContext;

use crate::cloud_provider::aws::databases::utilities::{aws_final_snapshot_name, get_parameter_group_from_version};
use crate::cloud_provider::service::{
    check_service_version, default_tera_context, delete_stateful_service, deploy_stateful_service, get_tfstate_name,
    get_tfstate_suffix, scale_down_database, send_progress_on_long_task, Action, Create, Database, DatabaseOptions,
    DatabaseType, Delete, Helm, Pause, Service, ServiceType, ServiceVersionCheckResult, StatefulService, Terraform,
};
use crate::cloud_provider::utilities::{
    generate_supported_version, get_self_hosted_mysql_version, get_supported_version_to_use, managed_db_name_sanitizer,
    print_action,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter, Transmitter};
use crate::logger::Logger;
use crate::models::DatabaseMode::MANAGED;
use crate::models::{Context, DatabaseKind, Listen, Listener, Listeners};
use ::function_name::named;

pub struct MySQLAws {
    context: Context,
    id: String,
    action: Action,
    name: String,
    version: String,
    fqdn: String,
    fqdn_id: String,
    total_cpus: String,
    total_ram_in_mib: u32,
    database_instance_type: String,
    options: DatabaseOptions,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl MySQLAws {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        version: &str,
        fqdn: &str,
        fqdn_id: &str,
        total_cpus: String,
        total_ram_in_mib: u32,
        database_instance_type: &str,
        options: DatabaseOptions,
        listeners: Listeners,
        logger: Box<dyn Logger>,
    ) -> Self {
        Self {
            context,
            action,
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            fqdn: fqdn.to_string(),
            fqdn_id: fqdn_id.to_string(),
            total_cpus,
            total_ram_in_mib,
            database_instance_type: database_instance_type.to_string(),
            options,
            listeners,
            logger,
        }
    }

    fn matching_correct_version(
        &self,
        is_managed_services: bool,
        event_details: EventDetails,
    ) -> Result<ServiceVersionCheckResult, EngineError> {
        check_service_version(
            get_mysql_version(self.version(), is_managed_services),
            self,
            event_details,
            self.logger(),
        )
    }

    fn cloud_provider_name(&self) -> &str {
        "aws"
    }

    fn struct_name(&self) -> &str {
        "mysql"
    }
}

impl StatefulService for MySQLAws {
    fn as_stateful_service(&self) -> &dyn StatefulService {
        self
    }

    fn is_managed_service(&self) -> bool {
        self.options.mode == MANAGED
    }
}

impl ToTransmitter for MySQLAws {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::Database(self.id().to_string(), self.service_type().to_string(), self.name().to_string())
    }
}

impl Service for MySQLAws {
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
        // https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_Limits.html#RDS_Limits.Constraints
        let prefix = "mysql";
        let max_size = 63 - 3; // max RDS - k8s statefulset chars
        managed_db_name_sanitizer(max_size, prefix, self.name())
    }

    fn version(&self) -> String {
        self.version.clone()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn private_port(&self) -> Option<u16> {
        Some(self.options.port)
    }

    fn start_timeout(&self) -> Timeout<u32> {
        Timeout::Default
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

        let version = &self.matching_correct_version(self.is_managed_service(), event_details.clone())?;
        context.insert("version", &version.matched_version().to_string());

        if self.is_managed_service() {
            let parameter_group_family =
                match get_parameter_group_from_version(version.matched_version(), DatabaseKind::Mysql) {
                    Ok(v) => v,
                    Err(e) => {
                        return Err(EngineError::new_terraform_unsupported_context_parameter_value(
                            event_details,
                            "MySQL".to_string(),
                            "parameter_group_family".to_string(),
                            version.matched_version().to_string(),
                            Some(e),
                        ))
                    }
                };
            context.insert("parameter_group_family", &parameter_group_family);
        };

        for (k, v) in kubernetes.cloud_provider().tera_context_environment_variables() {
            context.insert(k, v);
        }

        context.insert("kubernetes_cluster_id", kubernetes.id());
        context.insert("kubernetes_cluster_name", kubernetes.name());

        context.insert("fqdn_id", self.fqdn_id.as_str());
        context.insert("fqdn", self.fqdn(target, &self.fqdn, self.is_managed_service()).as_str());
        context.insert("service_name", self.fqdn_id.as_str());
        context.insert("database_login", self.options.login.as_str());
        context.insert("database_password", self.options.password.as_str());
        context.insert("database_port", &self.private_port());
        context.insert("database_disk_size_in_gib", &self.options.disk_size_in_gib);
        context.insert("database_instance_type", &self.database_instance_type);
        context.insert("database_disk_type", &self.options.database_disk_type);
        context.insert("encrypt_disk", &self.options.encrypt_disk);
        context.insert("database_name", &self.sanitized_name());
        context.insert("database_ram_size_in_mib", &self.total_ram_in_mib);
        context.insert("database_total_cpus", &self.total_cpus);
        context.insert("database_fqdn", &self.options.host.as_str());
        context.insert("database_id", &self.id());
        context.insert("tfstate_suffix_name", &get_tfstate_suffix(self));
        context.insert("tfstate_name", &get_tfstate_name(self));

        context.insert("skip_final_snapshot", &false);
        context.insert("final_snapshot_name", &aws_final_snapshot_name(self.id()));
        context.insert("delete_automated_backups", &self.context().is_test_cluster());
        context.insert("publicly_accessible", &self.options.publicly_accessible);
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert("resource_expiration_in_seconds", &self.context.resource_expiration_in_seconds())
        }

        Ok(context)
    }

    fn logger(&self) -> &dyn Logger {
        &*self.logger
    }

    fn selector(&self) -> Option<String> {
        Some(format!("app={}", self.sanitized_name()))
    }
}

impl Database for MySQLAws {}

impl Helm for MySQLAws {
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
        format!("{}/aws/chart_values/mysql", self.context.lib_root_dir()) // FIXME replace `chart_values` by `charts_values`
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        format!("{}/common/charts/external-name-svc", self.context.lib_root_dir())
    }
}

impl Terraform for MySQLAws {
    fn terraform_common_resource_dir_path(&self) -> String {
        format!("{}/aws/services/common", self.context.lib_root_dir())
    }

    fn terraform_resource_dir_path(&self) -> String {
        format!("{}/aws/services/mysql", self.context.lib_root_dir())
    }
}

impl Create for MySQLAws {
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_stateful_service(target, self, event_details.clone(), self.logger())
        })
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        self.check_domains(self.listeners.clone(), vec![self.fqdn.as_str()], event_details, self.logger())
    }

    #[named]
    fn on_create_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        Ok(())
    }
}

impl Pause for MySQLAws {
    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
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
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        Ok(())
    }
}

impl Delete for MySQLAws {
    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
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
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        Ok(())
    }
}

impl Listen for MySQLAws {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

fn get_mysql_version(requested_version: String, is_managed_service: bool) -> Result<String, CommandError> {
    if is_managed_service {
        get_managed_mysql_version(requested_version)
    } else {
        get_self_hosted_mysql_version(requested_version)
    }
}

fn get_managed_mysql_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_mysql_versions = HashMap::new();
    // https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_MySQL.html#MySQL.Concepts.VersionMgmt

    // v5.7
    let mut v57 = generate_supported_version(5, 7, 7, Some(16), Some(34), None);
    v57.remove("5.7.32");
    v57.remove("5.7.29");
    v57.remove("5.7.27");
    v57.remove("5.7.20");
    v57.remove("5.7.18");
    supported_mysql_versions.extend(v57);

    // v8
    let mut v8 = generate_supported_version(8, 0, 0, Some(11), Some(26), None);
    v8.remove("8.0.24");
    v8.remove("8.0.22");
    v8.remove("8.0.18");
    v8.remove("8.0.14");
    v8.remove("8.0.12");
    supported_mysql_versions.extend(v8);

    get_supported_version_to_use("RDS MySQL", supported_mysql_versions, requested_version)
}

#[cfg(test)]
mod tests_mysql {
    use crate::cloud_provider::aws::databases::mysql::get_mysql_version;

    #[test]
    fn check_mysql_version() {
        // managed version
        assert_eq!(get_mysql_version("8".to_string(), true).unwrap(), "8.0.26");
        assert_eq!(get_mysql_version("8.0".to_string(), true).unwrap(), "8.0.26");
        assert_eq!(get_mysql_version("8.0.16".to_string(), true).unwrap(), "8.0.16");
        assert_eq!(
            get_mysql_version("8.0.18".to_string(), true)
                .unwrap_err()
                .message()
                .as_str(),
            "RDS MySQL 8.0.18 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_mysql_version("5".to_string(), false).unwrap(), "5.7.34");
        assert_eq!(get_mysql_version("5.7".to_string(), false).unwrap(), "5.7.34");
        assert_eq!(get_mysql_version("5.7.31".to_string(), false).unwrap(), "5.7.31");
        assert_eq!(
            get_mysql_version("1.0".to_string(), false)
                .unwrap_err()
                .message()
                .as_str(),
            "MySQL 1.0 version is not supported"
        );
    }
}
