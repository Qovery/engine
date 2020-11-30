use tera::Context as TeraContext;

use crate::cloud_provider::aws::databases::utilities;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Action, Backup, Create, Database, DatabaseOptions, DatabaseType, Delete, Downgrade, Pause,
    Service, ServiceType, StatefulService, Upgrade,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause};
use crate::models::Context;
use std::collections::HashMap;

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

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("databases/{}", self.name()),
        )
    }

    fn tera_context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> TeraContext {
        let mut context = self.default_tera_context(kubernetes, environment);

        // FIXME: is there an other way than downcast a pointer?
        let cp = kubernetes
            .cloud_provider()
            .as_any()
            .downcast_ref::<AWS>()
            .expect("Could not downcast kubernetes.cloud_provider() to AWS");

        // we need the kubernetes config file to store tfstates file in kube secrets
        let kubernetes_config_file_path = utilities::get_kubernetes_config_path(
            self.workspace_directory().as_str(),
            kubernetes,
            environment,
        );

        match kubernetes_config_file_path {
            Ok(kube_config) => {
                context.insert("kubeconfig_path", &kube_config.as_str());
                let aws = kubernetes
                    .cloud_provider()
                    .as_any()
                    .downcast_ref::<AWS>()
                    .unwrap();

                utilities::create_namespace(&environment.namespace(), kube_config.as_str(), aws);
            }
            Err(e) => error!(
                "Failed to generate the kubernetes config file path: {:?}",
                e
            ),
        }

        context.insert("namespace", environment.namespace());

        context.insert("aws_access_key", &cp.access_key_id);
        context.insert("aws_secret_key", &cp.secret_access_key);
        context.insert("eks_cluster_id", kubernetes.id());
        context.insert("eks_cluster_name", kubernetes.name());

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

        context.insert(
            "resource_expiration_in_seconds",
            &self.context.resource_expiration_in_seconds(),
        );

        context
    }

    fn delete(&self, target: &DeploymentTarget, is_error: bool) -> Result<(), EngineError> {
        let workspace_dir = self.workspace_directory();

        match target {
            DeploymentTarget::ManagedServices(kubernetes, environment) => {
                if is_error {
                    // do not delete if it is an error
                    return Ok(());
                }

                let context = self.tera_context(*kubernetes, *environment);

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
                        utilities::delete_terraform_tfstate_secret(
                            *kubernetes,
                            environment,
                            self.workspace_directory().as_str(),
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
                let selector = format!("app={}", self.name());

                if is_error {
                    let _ = cast_simple_error_to_engine_error(
                        self.engine_error_scope(),
                        self.context.execution_id(),
                        common::get_stateless_resource_information(
                            *kubernetes,
                            *environment,
                            workspace_dir.as_str(),
                            selector.as_str(),
                        ),
                    )?;
                }

                // clean the resource
                let _ = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    common::do_stateless_service_cleanup(
                        *kubernetes,
                        *environment,
                        workspace_dir.as_str(),
                        helm_release_name.as_str(),
                    ),
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
        let mut redis_managed_versions = HashMap::with_capacity(2);
        redis_managed_versions.insert(5, "5.0.6");
        redis_managed_versions.insert(6, "6.x");

        // todo: return Result instead to better handle not supported version and avoid trying to deploy a non supported version
        match utilities::check_version(redis_managed_versions, self.version.as_str()) {
            Ok(v) => v,
            Err(e) => e,
        }
        .as_ref()
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
                info!("deploy Redis on AWS Elasticcache for {}", self.name());
                let context = self.tera_context(*kubernetes, *environment);

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

                let context = self.tera_context(*kubernetes, *environment);
                let workspace_dir = self.workspace_directory();

                let aws = kubernetes
                    .cloud_provider()
                    .as_any()
                    .downcast_ref::<AWS>()
                    .unwrap();

                let kubernetes_config_file_path = cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    common::kubernetes_config_path(
                        workspace_dir.as_str(),
                        environment.organization_id.as_str(),
                        kubernetes.id(),
                        aws.access_key_id.as_str(),
                        aws.secret_access_key.as_str(),
                        kubernetes.region(),
                    ),
                )?;
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
                let aws_credentials_envs = vec![
                    (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
                    (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
                ];

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
                        aws_credentials_envs.clone(),
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
                    aws_credentials_envs,
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

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.Redis.on_create_error() called for {}", self.name());

        self.delete(target, true)
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
        self.delete(target, false)
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!("AWS.Redis.on_create_error() called for {}", self.name());
        self.delete(target, true)
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
