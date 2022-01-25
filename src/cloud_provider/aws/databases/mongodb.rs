use std::collections::HashMap;

use crate::cloud_provider::aws::databases::utilities::aws_final_snapshot_name;
use tera::Context as TeraContext;

use crate::cloud_provider::service::{
    check_service_version, default_tera_context, delete_stateful_service, deploy_stateful_service, get_tfstate_name,
    get_tfstate_suffix, scale_down_database, send_progress_on_long_task, Action, Create, Database, DatabaseOptions,
    DatabaseType, Delete, Helm, Pause, Service, ServiceType, StatefulService, Terraform,
};
use crate::cloud_provider::utilities::{
    generate_supported_version, get_self_hosted_mongodb_version, get_supported_version_to_use, print_action,
    sanitize_db_name,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl;
use crate::error::{EngineError, EngineErrorScope, StringError};
use crate::events::{ToTransmitter, Transmitter};
use crate::models::DatabaseMode::MANAGED;
use crate::models::{Context, Listen, Listener, Listeners};
use ::function_name::named;

pub struct MongoDB {
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

impl MongoDB {
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
        MongoDB {
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
        check_service_version(get_mongodb_version(self.version(), is_managed_services), self)
    }

    fn cloud_provider_name(&self) -> &str {
        "aws"
    }

    fn struct_name(&self) -> &str {
        "mongodb"
    }
}

impl StatefulService for MongoDB {
    fn is_managed_service(&self) -> bool {
        self.options.mode == MANAGED
    }
}

impl Service for MongoDB {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Database(DatabaseType::MongoDB(&self.options))
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn sanitized_name(&self) -> String {
        sanitize_db_name("mongodb", self.id())
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
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, target.kubernetes, target.environment);

        // we need the kubernetes config file to store tfstates file in kube secrets
        let kube_config_file_path = match kubernetes.get_kubeconfig_file_path() {
            Ok(path) => path,
            Err(e) => {
                return Err(e.to_legacy_engine_error());
            }
        };
        context.insert("kubeconfig_path", &kube_config_file_path);

        kubectl::kubectl_exec_create_namespace_without_labels(
            &environment.namespace(),
            kube_config_file_path.as_str(),
            kubernetes.cloud_provider().credentials_environment_variables(),
        );

        context.insert("namespace", environment.namespace());

        let version = self.matching_correct_version(self.is_managed_service())?;
        context.insert("version", &version);

        for (k, v) in kubernetes.cloud_provider().tera_context_environment_variables() {
            context.insert(k, v);
        }

        context.insert("kubernetes_cluster_id", kubernetes.id());
        context.insert("kubernetes_cluster_name", kubernetes.name());

        context.insert("fqdn_id", self.fqdn_id.as_str());
        context.insert("fqdn", self.fqdn(&self.fqdn, self.is_managed_service()).as_str());
        context.insert("sanitized_name", self.sanitized_name().as_str());
        context.insert("database_db_name", self.name.as_str());
        context.insert("database_login", self.options.login.as_str());
        context.insert("database_password", self.options.password.as_str());
        context.insert("database_port", &self.private_port());
        context.insert("database_disk_size_in_gib", &self.options.disk_size_in_gib);
        context.insert("database_instance_type", &self.database_instance_type);
        context.insert("database_disk_type", &self.options.database_disk_type);
        context.insert("encrypt_disk", &self.options.encrypt_disk);
        context.insert("database_ram_size_in_mib", &self.total_ram_in_mib);
        context.insert("database_total_cpus", &self.total_cpus);
        context.insert("database_fqdn", &self.options.host.as_str());
        context.insert("database_id", &self.id());
        context.insert("tfstate_suffix_name", &get_tfstate_suffix(self));
        context.insert("tfstate_name", &get_tfstate_name(self));

        context.insert("publicly_accessible", &self.options.publicly_accessible);
        context.insert("skip_final_snapshot", &false);
        context.insert("final_snapshot_name", &aws_final_snapshot_name(self.id()));
        context.insert("delete_automated_backups", &self.context().is_test_cluster());
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

impl Database for MongoDB {}

impl ToTransmitter for MongoDB {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::Database(
            self.id().to_string(),
            self.service_type().to_string(),
            self.name().to_string(),
        )
    }
}

impl Helm for MongoDB {
    fn helm_selector(&self) -> Option<String> {
        self.selector()
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("mongodb-{}", self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/common/services/mongodb", self.context.lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        format!("{}/aws/chart_values/mongodb", self.context.lib_root_dir()) // FIXME replace `chart_values` by `charts_values`
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        format!("{}/common/charts/external-name-svc", self.context.lib_root_dir())
    }
}

impl Terraform for MongoDB {
    fn terraform_common_resource_dir_path(&self) -> String {
        format!("{}/aws/services/common", self.context.lib_root_dir())
    }

    fn terraform_resource_dir_path(&self) -> String {
        format!("{}/aws/services/mongodb", self.context.lib_root_dir())
    }
}

impl Create for MongoDB {
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

impl Pause for MongoDB {
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

impl Delete for MongoDB {
    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Pause, || {
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

impl Listen for MongoDB {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

fn get_mongodb_version(requested_version: String, is_managed_service: bool) -> Result<String, StringError> {
    if is_managed_service {
        get_managed_mongodb_version(requested_version)
    } else {
        get_self_hosted_mongodb_version(requested_version)
    }
}

fn get_managed_mongodb_version(requested_version: String) -> Result<String, StringError> {
    let mut supported_mongodb_versions = HashMap::new();

    // v3.6.0
    let mongo_version = generate_supported_version(3, 6, 6, Some(0), Some(0), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.0.0
    let mongo_version = generate_supported_version(4, 0, 0, Some(0), Some(0), None);
    supported_mongodb_versions.extend(mongo_version);

    get_supported_version_to_use("DocumentDB", supported_mongodb_versions, requested_version)
}

#[cfg(test)]
mod tests_mongodb {
    use crate::cloud_provider::aws::databases::mongodb::{get_mongodb_version, MongoDB};
    use crate::cloud_provider::service::{Action, DatabaseOptions, Service};
    use crate::models::{Context, DatabaseMode};

    #[test]
    fn check_mongodb_version() {
        // managed version
        assert_eq!(get_mongodb_version("4".to_string(), true).unwrap(), "4.0.0");
        assert_eq!(get_mongodb_version("4.0".to_string(), true).unwrap(), "4.0.0");
        assert_eq!(
            get_mongodb_version("4.4".to_string(), true).unwrap_err().as_str(),
            "DocumentDB 4.4 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_mongodb_version("4".to_string(), false).unwrap(), "4.4.4");
        assert_eq!(get_mongodb_version("4.2".to_string(), false).unwrap(), "4.2.12");
        assert_eq!(
            get_mongodb_version("3.4".to_string(), false).unwrap_err().as_str(),
            "MongoDB 3.4 version is not supported"
        );
    }

    #[test]
    fn mongo_name_sanitizer() {
        let db_id = "dbid";
        let db_expected_name = "dbid-mongodb";

        let database = MongoDB::new(
            Context::new(
                "".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                false,
                None,
                vec![],
                None,
            ),
            db_id.clone(),
            Action::Create,
            "mongo-db",
            "8",
            "mongotest.qovery.io",
            "pgid",
            "1".to_string(),
            512,
            "db.t2.micro",
            DatabaseOptions {
                login: "".to_string(),
                password: "".to_string(),
                host: "".to_string(),
                port: 5432,
                mode: DatabaseMode::CONTAINER,
                disk_size_in_gib: 10,
                database_disk_type: "gp2".to_string(),
                encrypt_disk: false,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
            },
            vec![],
        );
        assert_eq!(database.sanitized_name(), db_expected_name);
    }
}
