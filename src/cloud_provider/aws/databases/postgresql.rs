use std::collections::HashMap;

use tera::Context as TeraContext;

use crate::cloud_provider::aws::databases::utilities::rds_name_sanitizer;
use crate::cloud_provider::environment::Kind;
use crate::cloud_provider::service::{
    check_service_version, default_tera_context, delete_stateful_service, deploy_stateful_service, get_tfstate_name,
    get_tfstate_suffix, send_progress_on_long_task, Action, Backup, Create, Database, DatabaseOptions, DatabaseType,
    Delete, Downgrade, Helm, Pause, Service, ServiceType, StatefulService, Terraform, Upgrade,
};
use crate::cloud_provider::utilities::{
    generate_supported_version, get_self_hosted_postgres_version, get_supported_version_to_use,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl;
use crate::error::{EngineError, EngineErrorScope, StringError};
use crate::models::{Context, Listen, Listener, Listeners};

pub struct PostgreSQL {
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

impl PostgreSQL {
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
        PostgreSQL {
            context,
            action,
            id: id.to_string(),
            name: Self::sanitize_name("postgresql", name),
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
        check_service_version(get_postgres_version(self.version(), is_managed_services), self)
    }

    fn sanitize_name(prefix: &str, name: &str) -> String {
        // https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_Limits.html#RDS_Limits.Constraints
        let max_size = 63 - 3; // max RDS - k8s statefulset chars
        rds_name_sanitizer(max_size, prefix, name)
    }
}

impl StatefulService for PostgreSQL {}

impl Service for PostgreSQL {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Database(DatabaseType::PostgreSQL(&self.options))
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.version.as_str()
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

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    fn total_instances(&self) -> u16 {
        1
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let is_managed_services = match environment.kind {
            Kind::Production => true,
            Kind::Development => false,
        };

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

        let version = self.matching_correct_version(is_managed_services)?;
        context.insert("version", &version);

        for (k, v) in kubernetes.cloud_provider().tera_context_environment_variables() {
            context.insert(k, v);
        }

        context.insert("kubernetes_cluster_id", kubernetes.id());
        context.insert("kubernetes_cluster_name", kubernetes.name());

        context.insert("fqdn_id", self.fqdn_id.as_str());
        context.insert("fqdn", self.fqdn.as_str());

        context.insert("database_name", self.name.as_str());
        context.insert("database_login", self.options.login.as_str());
        context.insert("database_password", self.options.password.as_str());
        context.insert("database_port", &self.private_port());
        context.insert("database_disk_size_in_gib", &self.options.disk_size_in_gib);
        context.insert("database_instance_type", &self.database_instance_type);
        context.insert("database_disk_type", &self.options.database_disk_type);
        context.insert("database_ram_size_in_mib", &self.total_ram_in_mib);
        context.insert("database_total_cpus", &self.total_cpus);
        context.insert("database_fqdn", &self.options.host.as_str());
        context.insert("database_id", &self.id());
        context.insert("tfstate_suffix_name", &get_tfstate_suffix(self));
        context.insert("tfstate_name", &get_tfstate_name(self));

        context.insert("delete_automated_backups", &self.context().is_test_cluster());

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }

        Ok(context)
    }

    fn selector(&self) -> String {
        format!("app={}", self.name())
    }

    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Database(
            self.id().to_string(),
            self.service_type().name().to_string(),
            self.name().to_string(),
        )
    }
}

impl Database for PostgreSQL {}

impl Helm for PostgreSQL {
    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("postgresql-{}", self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/common/services/postgresql", self.context.lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        format!("{}/aws/chart_values/postgresql", self.context.lib_root_dir())
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        format!("{}/common/charts/external-name-svc", self.context.lib_root_dir())
    }
}

impl Terraform for PostgreSQL {
    fn terraform_common_resource_dir_path(&self) -> String {
        format!("{}/aws/services/common", self.context.lib_root_dir())
    }

    fn terraform_resource_dir_path(&self) -> String {
        format!("{}/aws/services/postgresql", self.context.lib_root_dir())
    }
}

impl Create for PostgreSQL {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.PostgreSQL.on_create() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Create,
            Box::new(|| deploy_stateful_service(target, self)),
        )
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        self.check_domains(self.listeners.clone(), vec![self.fqdn.as_str()])
    }

    fn on_create_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.PostgreSQL.on_create_error() called for {}", self.name());

        Ok(())
    }
}

impl Pause for PostgreSQL {
    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.PostgreSQL.on_pause() called for {}", self.name());

        // TODO how to pause production? - the goal is to reduce cost, but it is possible to pause a production env?
        // TODO how to pause development? - the goal is also to reduce cost, we can set the number of instances to 0, which will avoid to delete data :)

        Ok(())
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.PostgreSQL.on_pause_error() called for {}", self.name());

        // TODO what to do if there is a pause error?

        Ok(())
    }
}

impl Delete for PostgreSQL {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.PostgreSQL.on_delete() called for {}", self.name());

        send_progress_on_long_task(
            self,
            crate::cloud_provider::service::Action::Delete,
            Box::new(|| delete_stateful_service(target, self)),
        )
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.PostgreSQL.on_create_error() called for {}", self.name());

        Ok(())
    }
}

impl crate::cloud_provider::service::Clone for PostgreSQL {
    fn on_clone(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_clone_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_clone_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}

impl Upgrade for PostgreSQL {
    fn on_upgrade(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_upgrade_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_upgrade_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}

impl Downgrade for PostgreSQL {
    fn on_downgrade(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_downgrade_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_downgrade_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}

impl Backup for PostgreSQL {
    fn on_backup(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_backup_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_backup_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_restore(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_restore_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_restore_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}

impl Listen for PostgreSQL {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

fn get_postgres_version(requested_version: &str, is_managed_service: bool) -> Result<String, StringError> {
    if is_managed_service {
        get_managed_postgres_version(requested_version)
    } else {
        get_self_hosted_postgres_version(requested_version)
    }
}

fn get_managed_postgres_version(requested_version: &str) -> Result<String, StringError> {
    let mut supported_postgres_versions = HashMap::new();

    // https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_PostgreSQL.html#PostgreSQL.Concepts

    // v10
    let mut v10 = generate_supported_version(10, 1, 14, None, None, None);
    v10.remove("10.2"); // non supported version by AWS
    v10.remove("10.8"); // non supported version by AWS
    supported_postgres_versions.extend(v10);

    // v11
    let mut v11 = generate_supported_version(11, 1, 9, None, None, None);
    v11.remove("11.3"); // non supported version by AWS
    supported_postgres_versions.extend(v11);

    // v12
    let v12 = generate_supported_version(12, 2, 4, None, None, None);
    supported_postgres_versions.extend(v12);

    get_supported_version_to_use("Postgresql", supported_postgres_versions, requested_version)
}

#[cfg(test)]
mod tests_postgres {
    use crate::cloud_provider::aws::databases::postgresql::{get_postgres_version, PostgreSQL};
    use crate::cloud_provider::service::{Action, DatabaseOptions};
    use crate::models::Context;

    #[test]
    fn check_postgres_version() {
        // managed version
        assert_eq!(get_postgres_version("12", true).unwrap(), "12.4");
        assert_eq!(get_postgres_version("12.3", true).unwrap(), "12.3");
        assert_eq!(
            get_postgres_version("12.3.0", true).unwrap_err().as_str(),
            "Postgresql 12.3.0 version is not supported"
        );
        assert_eq!(
            get_postgres_version("11.3", true).unwrap_err().as_str(),
            "Postgresql 11.3 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_postgres_version("12", false).unwrap(), "12.4.0");
        assert_eq!(get_postgres_version("12.3", false).unwrap(), "12.3.0");
        assert_eq!(get_postgres_version("12.3.0", false).unwrap(), "12.3.0");
        assert_eq!(
            get_postgres_version("1.0", false).unwrap_err().as_str(),
            "Postgresql 1.0 version is not supported"
        );
    }

    #[test]
    fn postgres_name_sanitizer() {
        let db_input_name = "test-name_sanitizer-with-too-many-chars-not-allowed-which_will-be-shrinked-at-the-end";
        let db_expected_name = "postgresqltestnamesanitizerwithtoomanycharsnotallowedwhichwi";

        let database = PostgreSQL::new(
            Context::new("".to_string(), "".to_string(), "".to_string(), None, None),
            "pgid",
            Action::Create,
            db_input_name,
            "8",
            "pgtest.qovery.io",
            "pgid",
            "1".to_string(),
            512,
            "db.t2.micro",
            DatabaseOptions {
                login: "".to_string(),
                password: "".to_string(),
                host: "".to_string(),
                port: 5432,
                disk_size_in_gib: 10,
                database_disk_type: "gp2".to_string(),
            },
            vec![],
        );
        assert_eq!(database.name, db_expected_name);
    }
}
