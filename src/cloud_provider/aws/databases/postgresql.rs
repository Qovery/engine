use std::collections::HashMap;

use tera::Context as TeraContext;

use crate::cloud_provider::aws::databases::utilities;
use crate::cloud_provider::aws::databases::utilities::{
    generate_supported_version, get_tfstate_name, get_tfstate_suffix,
};
use crate::cloud_provider::environment::{Environment, Kind};
use crate::cloud_provider::kubernetes::{do_stateless_service_cleanup, Kubernetes};
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
    ) -> Self {
        PostgreSQL {
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
        crate::string::cut(format!("postgresql-{}", self.id()), 50)
    }

    fn tera_context(
        &self,
        kubernetes: &dyn Kubernetes,
        environment: &Environment,
    ) -> Result<TeraContext, EngineError> {
        let mut context = self.default_tera_context(kubernetes, environment);

        let is_managed_services = match environment.kind {
            Kind::Production => true,
            Kind::Development => false,
        };

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

        context.insert("namespace", environment.namespace());

        let version = self.matching_correct_version(is_managed_services)?;
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

        context.insert(
            "delete_automated_backups",
            &self.context().is_test_cluster(),
        );
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }

        Ok(context)
    }

    fn matching_correct_version(&self, is_managed_services: bool) -> Result<String, EngineError> {
        match get_postgres_version(self.version(), is_managed_services) {
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
                        "The provided PostgreSQL version is not supported, please refer to the \
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
                        format!("{}/aws/services/postgresql", self.context.lib_root_dir()).as_str(),
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
                    crate::template::generate_and_copy_all_files_into_dir(
                        format!(
                            "{}/aws/charts/external-name-svc",
                            self.context.lib_root_dir()
                        )
                        .as_str(),
                        workspace_dir.as_str(),
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

impl Database for PostgreSQL {}

impl Create for PostgreSQL {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.PostgreSQL.on_create() called for {}", self.name());

        let workspace_dir = self.workspace_directory();

        match target {
            DeploymentTarget::ManagedServices(kubernetes, environment) => {
                // use terraform
                info!("deploy postgresql on AWS RDS for {}", self.name());
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
                        format!("{}/aws/services/postgresql", self.context.lib_root_dir()).as_str(),
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
                info!("deploy PostgreSQL on Kubernetes for {}", self.name());

                let context = self.tera_context(*kubernetes, *environment)?;

                let kubernetes_config_file_path = kubernetes.config_file_path()?;

                let from_dir =
                    format!("{}/common/services/postgresql", self.context.lib_root_dir());

                let chart_values = format!(
                    "{}/common/chart_values/postgresql",
                    &self.context.lib_root_dir()
                );

                // default chart
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
                        "PostgreSQL database fails to be deployed (before start)".into(),
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
                                "PostgreSQL database {} with id {} failed to start after several retries",
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
        Ok(())
    }

    fn on_create_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "AWS.PostgreSQL.on_create_error() called for {}",
            self.name()
        );

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
        self.delete(target)
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "AWS.PostgreSQL.on_create_error() called for {}",
            self.name()
        );

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

fn get_postgres_version(
    requested_version: &str,
    is_managed_service: bool,
) -> Result<String, StringError> {
    let mut supported_postgres_versions = HashMap::new();

    if is_managed_service {
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
    } else {
        // https://hub.docker.com/r/bitnami/postgresql/tags?page=1&ordering=last_updated

        // v10
        let v10 = generate_supported_version(10, 1, 14, Some(0), Some(0), None);
        supported_postgres_versions.extend(v10);

        // v11
        let v11 = generate_supported_version(11, 1, 9, Some(0), Some(0), None);
        supported_postgres_versions.extend(v11);

        // v12
        let v12 = generate_supported_version(12, 2, 4, Some(0), Some(0), None);
        supported_postgres_versions.extend(v12);
    }

    utilities::get_supported_version_to_use(
        "Postgresql",
        supported_postgres_versions,
        requested_version,
    )
}

#[cfg(test)]
mod tests_postgres {
    use std::collections::HashMap;

    use crate::cloud_provider::aws::databases::postgresql::get_postgres_version;

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
}
