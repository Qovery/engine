use std::collections::HashMap;

use tera::Context as TeraContext;

use crate::cloud_provider::aws::databases::utilities;
use crate::cloud_provider::aws::databases::utilities::{get_tfstate_name, get_tfstate_suffix};
use crate::cloud_provider::common::kubernetes::do_stateless_service_cleanup;
use crate::cloud_provider::environment::{Environment, Kind};
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Action, Backup, Create, Database, DatabaseOptions, DatabaseType, Delete, Downgrade, Pause,
    Service, ServiceType, StatefulService, Upgrade,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl;
use crate::cmd::structs::LabelsContent;
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause, StringError};
use crate::models::Context;

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
        }
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("redis-{}", self.id()), 50)
    }

    fn tera_context(
        &self,
        kubernetes: &dyn Kubernetes,
        environment: &Environment,
    ) -> Result<TeraContext, EngineError> {
        let mut context = self.default_tera_context(kubernetes, environment);

        // we need the kubernetes config file to store tfstates file in kube secrets
        let kubernetes_config_file_path = kubernetes.config_file_path();

        match kubernetes_config_file_path {
            Ok(kube_config) => {
                context.insert("kubeconfig_path", &kube_config.as_str());

                kubectl::kubectl_exec_create_namespace_without_labels(
                    &environment.namespace(),
                    kube_config.as_str(),
                    kubernetes
                        .cloud_provider()
                        .credentials_environment_variables(),
                );
            }
            Err(e) => error!(
                "Failed to generate the kubernetes config file path: {:?}",
                e
            ),
        }

        let is_managed_services = match environment.kind {
            Kind::Production => true,
            Kind::Development => false,
        };

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

        context.insert(
            "database_elasticache_parameter_group_name",
            parameter_group_name,
        );

        context.insert("namespace", environment.namespace());
        context.insert("version", &version);

        for (k, v) in kubernetes
            .cloud_provider()
            .tera_context_environment_variables()
        {
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
        context.insert("tfstate_suffix_name", &get_tfstate_suffix(&self.id()));
        context.insert("tfstate_name", &get_tfstate_name(&self.id()));

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }

        Ok(context)
    }

    fn matching_correct_version(&self, is_managed_services: bool) -> Result<String, EngineError> {
        match get_redis_version(self.version(), is_managed_services) {
            Ok(version) => {
                info!(
                    "version {} has been requested by the user; but matching version is {}",
                    self.version(),
                    version
                );

                Ok(version)
            }
            Err(err) => {
                error!("{}", err);
                warn!(
                    "fallback on the version {} provided by the user",
                    self.version()
                );

                Err(self.engine_error(
                    EngineErrorCause::User(
                        "The provided Redis version is not supported, please refer to the \
                documentation https://docs.qovery.com",
                    ),
                    err,
                ))
            }
        }
    }

    fn delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let workspace_dir = self.workspace_directory();

        match target {
            DeploymentTarget::ManagedServices(kubernetes, environment) => {
                let context = self.tera_context(*kubernetes, *environment)?;

                // deploy before destroy to avoid missing elements
                self.on_create(target)?;

                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::template::generate_and_copy_all_files_into_dir(
                        format!("{}/aws/services/common", self.context.lib_root_dir()).as_str(),
                        &workspace_dir,
                        &context,
                    ),
                )?;

                match crate::cmd::terraform::terraform_exec_with_init_plan_apply_destroy(
                    workspace_dir.as_str(),
                ) {
                    Ok(_) => {
                        info!("Deleting secrets containing tfstates");
                        let _ = utilities::delete_terraform_tfstate_secret(
                            *kubernetes,
                            &get_tfstate_name(&self.id()),
                        );
                    }
                    Err(e) => {
                        let message = format!(
                            "Error while destroying infrastructure {}",
                            e.message.unwrap_or("".into())
                        );

                        error!("{}", message);

                        return Err(self.engine_error(EngineErrorCause::Internal, message));
                    }
                }
            }
            DeploymentTarget::SelfHosted(kubernetes, environment) => {
                let helm_release_name = self.helm_release_name();

                // clean the resource
                let _ = do_stateless_service_cleanup(
                    *kubernetes,
                    *environment,
                    helm_release_name.as_str(),
                )?;
            }
        }

        Ok(())
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

    fn version(&self) -> &str {
        self.version.as_str()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn private_port(&self) -> Option<u16> {
        Some(self.options.port)
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
}

impl Database for Redis {}

impl Create for Redis {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        match target {
            DeploymentTarget::ManagedServices(kubernetes, environment) => {
                // use terraform
                info!("deploy Redis on AWS Elasticache for {}", self.name());
                let context = self.tera_context(*kubernetes, *environment)?;

                let workspace_dir = self.workspace_directory();

                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::template::generate_and_copy_all_files_into_dir(
                        format!("{}/aws/services/common", self.context.lib_root_dir()).as_str(),
                        &workspace_dir,
                        &context,
                    ),
                )?;

                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::template::generate_and_copy_all_files_into_dir(
                        format!("{}/aws/services/redis", self.context.lib_root_dir()).as_str(),
                        workspace_dir.as_str(),
                        &context,
                    ),
                )?;

                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::template::generate_and_copy_all_files_into_dir(
                        format!(
                            "{}/aws/charts/external-name-svc",
                            self.context.lib_root_dir()
                        )
                        .as_str(),
                        format!("{}/{}", workspace_dir, "external-name-svc").as_str(),
                        &context,
                    ),
                )?;

                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::cmd::terraform::terraform_exec_with_init_validate_plan_apply(
                        workspace_dir.as_str(),
                        self.context.is_dry_run_deploy(),
                    ),
                )?;
            }
            DeploymentTarget::SelfHosted(kubernetes, environment) => {
                // use helm
                info!("deploy Redis on Kubernetes for {}", self.name());

                let context = self.tera_context(*kubernetes, *environment)?;
                let workspace_dir = self.workspace_directory();

                let kubernetes_config_file_path = kubernetes.config_file_path()?;

                // default chart
                let from_dir = format!("{}/common/services/redis", self.context.lib_root_dir());

                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::template::generate_and_copy_all_files_into_dir(
                        from_dir.as_str(),
                        workspace_dir.as_str(),
                        &context,
                    ),
                )?;

                // overwrite with our chart values
                let chart_values =
                    format!("{}/common/chart_values/redis", &self.context.lib_root_dir());

                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::template::generate_and_copy_all_files_into_dir(
                        chart_values.as_str(),
                        workspace_dir.as_str(),
                        &context,
                    ),
                )?;

                let helm_release_name = self.helm_release_name();

                // define labels to add to namespace
                let namespace_labels = match self.context.resource_expiration_in_seconds() {
                    Some(_) => Some(vec![
                        (LabelsContent {
                            name: "ttl".to_string(),
                            value: format! {"{}", self.context.resource_expiration_in_seconds().unwrap()},
                        }),
                    ]),
                    None => None,
                };

                // create a namespace with labels if do not exists
                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::cmd::kubectl::kubectl_exec_create_namespace(
                        kubernetes_config_file_path.as_str(),
                        environment.namespace(),
                        namespace_labels,
                        kubernetes
                            .cloud_provider()
                            .credentials_environment_variables(),
                    ),
                )?;

                // do exec helm upgrade and return the last deployment status
                let helm_history_row = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    crate::cmd::helm::helm_exec_with_upgrade_history(
                        kubernetes_config_file_path.as_str(),
                        environment.namespace(),
                        helm_release_name.as_str(),
                        workspace_dir.as_str(),
                        Timeout::Default,
                        kubernetes
                            .cloud_provider()
                            .credentials_environment_variables(),
                    ),
                )?;

                // check deployment status
                if helm_history_row.is_none()
                    || !helm_history_row.unwrap().is_successfully_deployed()
                {
                    return Err(self.engine_error(
                        EngineErrorCause::Internal,
                        "Redis database fails to be deployed (before start)".into(),
                    ));
                }

                // check app status
                let selector = format!("app={}", self.name());

                match crate::cmd::kubectl::kubectl_exec_is_pod_ready_with_retry(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    selector.as_str(),
                    kubernetes
                        .cloud_provider()
                        .credentials_environment_variables(),
                ) {
                    Ok(Some(true)) => {}
                    _ => {
                        return Err(self.engine_error(
                            EngineErrorCause::Internal,
                            format!(
                                "Redis database {} with id {} failed to start after several retries",
                                self.name(),
                                self.id()
                            ),
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        //FIXME : perform an actual check
        Ok(())
    }

    fn on_create_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.Redis.on_create_error() called for {}", self.name());
        Ok(())
    }
}

impl Pause for Redis {
    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.Redis.on_pause() called for {}", self.name());

        // TODO how to pause production? - the goal is to reduce cost, but it is possible to pause a production env?
        // TODO how to pause development? - the goal is also to reduce cost, we can set the number of instances to 0, which will avoid to delete data :)

        Ok(())
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.Redis.on_pause_error() called for {}", self.name());

        // TODO what to do if there is a pause error?

        Ok(())
    }
}

impl Delete for Redis {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.Redis.on_delete() called for {}", self.name());
        self.delete(target)
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

fn get_redis_version(
    requested_version: &str,
    is_managed_service: bool,
) -> Result<String, StringError> {
    let mut supported_redis_versions = HashMap::with_capacity(2);
    let mut database_name = "Redis";

    if is_managed_service {
        // https://docs.aws.amazon.com/AmazonElastiCache/latest/red-ug/supported-engine-versions.html
        database_name = "Elasticache";

        supported_redis_versions.insert("6".to_string(), "6.x".to_string());
        supported_redis_versions.insert("5".to_string(), "5.0.6".to_string());
    } else {
        // https://hub.docker.com/r/bitnami/redis/tags?page=1&ordering=last_updated
        supported_redis_versions.insert("6".to_string(), "6.0.9".to_string());
        supported_redis_versions.insert("6.0".to_string(), "6.0.9".to_string());
        supported_redis_versions.insert("5".to_string(), "5.0.10".to_string());
        supported_redis_versions.insert("5.0".to_string(), "5.0.10".to_string());
    }

    utilities::get_supported_version_to_use(
        database_name,
        supported_redis_versions,
        requested_version,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::cloud_provider::aws::databases::redis::get_redis_version;

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
}
