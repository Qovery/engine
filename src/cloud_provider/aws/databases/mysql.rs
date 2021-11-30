use std::collections::HashMap;

use tera::Context as TeraContext;

use crate::cloud_provider::aws::databases::utilities::{aws_final_snapshot_name, get_parameter_group_from_version};
use crate::cloud_provider::service::{
    check_service_version, default_tera_context, delete_stateful_service, deploy_stateful_service, get_tfstate_name,
    get_tfstate_suffix, scale_down_database, send_progress_on_long_task, Action, Create, Database, DatabaseOptions,
    DatabaseType, Delete, Helm, Pause, Service, ServiceType, StatefulService, Terraform,
};
use crate::cloud_provider::utilities::{
    generate_supported_version, get_self_hosted_mysql_version, get_supported_version_to_use, managed_db_name_sanitizer,
    print_action,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl;
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope, StringError};
use crate::models::DatabaseMode::MANAGED;
use crate::models::{Context, DatabaseKind, Listen, Listener, Listeners};
use ::function_name::named;

pub struct MySQL {
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
}

impl MySQL {
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
        }
    }

    fn matching_correct_version(&self, is_managed_services: bool) -> Result<String, EngineError> {
        check_service_version(get_mysql_version(self.version(), is_managed_services), self)
    }

    fn cloud_provider_name(&self) -> &str {
        "aws"
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

    fn total_instances(&self) -> u16 {
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
        context.insert("version", &version);

        if self.is_managed_service() {
            let parameter_group_family = match get_parameter_group_from_version(&version, DatabaseKind::Mysql) {
                Ok(v) => v,
                Err(e) => {
                    return Err(EngineError {
                        cause: EngineErrorCause::Internal,
                        scope: EngineErrorScope::Engine,
                        execution_id: (&self.context.execution_id()).to_string(),
                        message: Some(e),
                    })
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

        context.insert("skip_final_snapshot", &false);
        context.insert("final_snapshot_name", &aws_final_snapshot_name(self.id()));
        context.insert("delete_automated_backups", &self.context().is_test_cluster());
        context.insert("publicly_accessible", &self.options.publicly_accessible);
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }

        Ok(context)
    }

    fn selector(&self) -> String {
        format!("app={}", self.sanitized_name())
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

impl Terraform for MySQL {
    fn terraform_common_resource_dir_path(&self) -> String {
        format!("{}/aws/services/common", self.context.lib_root_dir())
    }

    fn terraform_resource_dir_path(&self) -> String {
        format!("{}/aws/services/mysql", self.context.lib_root_dir())
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

fn get_mysql_version(requested_version: String, is_managed_service: bool) -> Result<String, StringError> {
    if is_managed_service {
        get_managed_mysql_version(requested_version)
    } else {
        get_self_hosted_mysql_version(requested_version)
    }
}

fn get_managed_mysql_version(requested_version: String) -> Result<String, StringError> {
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
    let mut v8 = generate_supported_version(8, 0, 0, Some(11), Some(25), None);
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
    use crate::cloud_provider::aws::databases::mysql::{get_mysql_version, MySQL};
    use crate::cloud_provider::service::{Action, DatabaseOptions, Service};
    use crate::models::{Context, DatabaseMode};

    #[test]
    fn check_mysql_version() {
        // managed version
        assert_eq!(get_mysql_version("8".to_string(), true).unwrap(), "8.0.25");
        assert_eq!(get_mysql_version("8.0".to_string(), true).unwrap(), "8.0.25");
        assert_eq!(get_mysql_version("8.0.16".to_string(), true).unwrap(), "8.0.16");
        assert_eq!(
            get_mysql_version("8.0.18".to_string(), true).unwrap_err().as_str(),
            "RDS MySQL 8.0.18 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_mysql_version("5".to_string(), false).unwrap(), "5.7.34");
        assert_eq!(get_mysql_version("5.7".to_string(), false).unwrap(), "5.7.34");
        assert_eq!(get_mysql_version("5.7.31".to_string(), false).unwrap(), "5.7.31");
        assert_eq!(
            get_mysql_version("1.0".to_string(), false).unwrap_err().as_str(),
            "MySQL 1.0 version is not supported"
        );
    }

    #[test]
    fn mysql_name_sanitizer() {
        let db_input_name = "test-name_sanitizer-with-too-many-chars-not-allowed-which_will-be-shrinked-at-the-end";
        let db_expected_name = "mysqltestnamesanitizerwithtoomanycharsnotallowedwhichwi";

        let database = MySQL::new(
            Context::new(
                "".to_string(),
                "".to_string(),
                "".to_string(),
                false,
                None,
                vec![],
                None,
            ),
            "mysqlid",
            Action::Create,
            db_input_name,
            "8",
            "mysqltest.qovery.io",
            "mysqlid",
            "1".to_string(),
            512,
            "db.t2.micro",
            DatabaseOptions {
                login: "".to_string(),
                password: "".to_string(),
                host: "".to_string(),
                port: 3306,
                mode: DatabaseMode::MANAGED,
                disk_size_in_gib: 10,
                database_disk_type: "gp2".to_string(),
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
            },
            vec![],
        );
        assert_eq!(database.sanitized_name(), db_expected_name);
    }
}
