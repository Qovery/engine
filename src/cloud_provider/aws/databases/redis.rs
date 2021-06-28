use std::collections::HashMap;

use tera::Context as TeraContext;

use crate::cloud_provider::environment::Kind;
use crate::cloud_provider::service::{
    check_service_version, default_tera_context, delete_stateful_service, deploy_stateful_service, get_tfstate_name,
    get_tfstate_suffix, scale_down_database, send_progress_on_long_task, Action, Backup, Create, Database,
    DatabaseOptions, DatabaseType, Delete, Downgrade, Helm, Pause, Service, ServiceType, StatefulService, Terraform,
    Upgrade,
};
use crate::cloud_provider::utilities::{get_self_hosted_redis_version, get_supported_version_to_use};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl;
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope, StringError};
use crate::models::{Context, Listen, Listener, Listeners};

pub struct Redis {
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

impl Redis {
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
        check_service_version(get_redis_version(self.version(), is_managed_services), self)
    }
}

impl StatefulService for Redis {}

impl Service for Redis {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Database(DatabaseType::Redis(&self.options))
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn sanitized_name(&self) -> String {
        // https://aws.amazon.com/about-aws/whats-new/2019/08/elasticache_supports_50_chars_cluster_name
        let prefix = "redis";
        let max_size = 47 - prefix.len(); // 50 (max Elasticache ) - 3 (k8s statefulset chars)
        let mut new_name = self.name().replace("_", "").replace("-", "");

        if new_name.chars().count() > max_size {
            new_name = new_name[..max_size].to_string();
        }

        format!("{}{}", prefix, new_name)
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

    fn cpu_burst(&self) -> String {
        unimplemented!()
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

        let version = self.matching_correct_version(is_managed_services)?;

        let parameter_group_name = if version.starts_with("5.") {
            "default.redis5.0"
        } else if version.starts_with("6.") {
            "default.redis6.x"
        } else {
            return Err(self.engine_error(
                EngineErrorCause::Internal,
                "Elasticache parameter group name unknown".to_string(),
            ));
        };

        context.insert("database_elasticache_parameter_group_name", parameter_group_name);

        context.insert("namespace", environment.namespace());
        context.insert("version", &version);

        for (k, v) in kubernetes.cloud_provider().tera_context_environment_variables() {
            context.insert(k, v);
        }

        context.insert("kubernetes_cluster_id", kubernetes.id());
        context.insert("kubernetes_cluster_name", kubernetes.name());

        context.insert("fqdn_id", self.fqdn_id.as_str());
        context.insert("fqdn", self.fqdn.as_str());

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

        context.insert("skip_final_snapshot", &self.context().is_test_cluster());
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

impl Database for Redis {}

impl Helm for Redis {
    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("redis-{}", self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/common/services/redis", self.context.lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        format!("{}/aws/chart_values/redis", self.context.lib_root_dir())
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        format!("{}/common/charts/external-name-svc", self.context.lib_root_dir())
    }
}

impl Terraform for Redis {
    fn terraform_common_resource_dir_path(&self) -> String {
        format!("{}/aws/services/common", self.context.lib_root_dir())
    }

    fn terraform_resource_dir_path(&self) -> String {
        format!("{}/aws/services/redis", self.context.lib_root_dir())
    }
}

impl Create for Redis {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.Redis.on_create() called for {}", self.name());

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_stateful_service(target, self)
        })
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        self.check_domains(self.listeners.clone(), vec![self.fqdn.as_str()])
    }

    fn on_create_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.Redis.on_create_error() called for {}", self.name());
        Ok(())
    }
}

impl Pause for Redis {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.Redis.on_pause() called for {}", self.name());

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Pause, || {
            scale_down_database(target, self, 0)
        })
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.Redis.on_pause_error() called for {}", self.name());

        Ok(())
    }
}

impl Delete for Redis {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.Redis.on_delete() called for {}", self.name());

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
            delete_stateful_service(target, self)
        })
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.Redis.on_create_error() called for {}", self.name());
        Ok(())
    }
}

impl crate::cloud_provider::service::Clone for Redis {
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

impl Upgrade for Redis {
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

impl Downgrade for Redis {
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

impl Backup for Redis {
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

impl Listen for Redis {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

fn get_redis_version(requested_version: &str, is_managed_service: bool) -> Result<String, StringError> {
    if is_managed_service {
        get_managed_redis_version(requested_version)
    } else {
        get_self_hosted_redis_version(requested_version)
    }
}

fn get_managed_redis_version(requested_version: &str) -> Result<String, StringError> {
    let mut supported_redis_versions = HashMap::with_capacity(2);
    // https://docs.aws.amazon.com/AmazonElastiCache/latest/red-ug/supported-engine-versions.html

    supported_redis_versions.insert("6".to_string(), "6.x".to_string());
    supported_redis_versions.insert("5".to_string(), "5.0.6".to_string());

    get_supported_version_to_use("Elasticache", supported_redis_versions, requested_version)
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::databases::redis::{get_redis_version, Redis};
    use crate::cloud_provider::service::{Action, DatabaseOptions, Service};
    use crate::models::Context;

    #[test]
    fn check_redis_version() {
        // managed version
        assert_eq!(get_redis_version("6", true).unwrap(), "6.x");
        assert_eq!(get_redis_version("5", true).unwrap(), "5.0.6");
        assert_eq!(
            get_redis_version("1.0", true).unwrap_err().as_str(),
            "Elasticache 1.0 version is not supported"
        );

        // self-hosted version
        assert_eq!(get_redis_version("6", false).unwrap(), "6.0.9");
        assert_eq!(get_redis_version("6.0", false).unwrap(), "6.0.9");
        assert_eq!(
            get_redis_version("1.0", false).unwrap_err().as_str(),
            "Redis 1.0 version is not supported"
        );
    }

    #[test]
    fn redis_name_sanitizer() {
        let db_input_name = "test-name_sanitizer-with-too-many-chars-not-allowed-which_will-be-shrinked-at-the-end";
        let db_expected_name = "redistestnamesanitizerwithtoomanycharsnotallowe";

        let database = Redis::new(
            Context::new(
                "".to_string(),
                "".to_string(),
                "".to_string(),
                false,
                None,
                vec![],
                None,
            ),
            "pgid",
            Action::Create,
            db_input_name,
            "8",
            "redistest.qovery.io",
            "redisid",
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
        assert_eq!(database.sanitized_name(), db_expected_name);
    }
}
