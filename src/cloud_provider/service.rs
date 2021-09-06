use std::net::TcpStream;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::Duration;

use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::utilities::check_domain_for;
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl::ScalingKind::Statefulset;
use crate::cmd::kubectl::{kubectl_exec_delete_secret, kubectl_exec_scale_replicas_by_selector, ScalingKind};
use crate::cmd::structs::LabelsContent;
use crate::error::{cast_simple_error_to_engine_error, StringError};
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::models::ProgressLevel::Info;
use crate::models::{Context, Listen, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope};

pub trait Service {
    fn context(&self) -> &Context;
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn sanitized_name(&self) -> String;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn workspace_directory(&self) -> String {
        let dir_root = match self.service_type() {
            ServiceType::Application => "applications",
            ServiceType::Database(_) => "databases",
            ServiceType::Router => "routers",
        };

        crate::fs::workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("{}/{}", dir_root, self.name()),
        )
        .unwrap()
    }
    fn version(&self) -> &str;
    fn action(&self) -> &Action;
    fn private_port(&self) -> Option<u16>;
    fn start_timeout(&self) -> Timeout<u32>;
    fn total_cpus(&self) -> String;
    fn cpu_burst(&self) -> String;
    fn total_ram_in_mib(&self) -> u32;
    fn total_instances(&self) -> u16;
    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError>;
    // used to retrieve logs by using Kubernetes labels (selector)
    fn selector(&self) -> String;
    fn debug_logs(&self, deployment_target: &DeploymentTarget) -> Vec<String> {
        debug_logs(self, deployment_target)
    }
    fn is_listening(&self, ip: &str) -> bool {
        let private_port = match self.private_port() {
            Some(private_port) => private_port,
            _ => return false,
        };

        TcpStream::connect(format!("{}:{}", ip, private_port)).is_ok()
    }
    fn engine_error_scope(&self) -> EngineErrorScope;
    fn engine_error(&self, cause: EngineErrorCause, message: String) -> EngineError {
        EngineError::new(
            cause,
            self.engine_error_scope(),
            self.context().execution_id(),
            Some(message),
        )
    }
    fn is_valid(&self) -> Result<(), EngineError> {
        let binaries = ["kubectl", "helm", "terraform", "aws-iam-authenticator"];

        for binary in binaries.iter() {
            if !crate::cmd::utilities::does_binary_exist(binary) {
                let err = format!("{} binary not found", binary);

                return Err(EngineError::new(
                    EngineErrorCause::Internal,
                    EngineErrorScope::Engine,
                    self.id(),
                    Some(err),
                ));
            }
        }

        // TODO check lib directories available

        Ok(())
    }

    fn progress_scope(&self) -> ProgressScope {
        let id = self.id().to_string();

        match self.service_type() {
            ServiceType::Application => ProgressScope::Application { id },
            ServiceType::Database(_) => ProgressScope::Database { id },
            ServiceType::Router => ProgressScope::Router { id },
        }
    }
}

pub trait StatelessService: Service + Create + Pause + Delete {
    fn exec_action(&self, deployment_target: &DeploymentTarget) -> Result<(), EngineError> {
        match self.action() {
            crate::cloud_provider::service::Action::Create => self.on_create(deployment_target),
            crate::cloud_provider::service::Action::Delete => self.on_delete(deployment_target),
            crate::cloud_provider::service::Action::Pause => self.on_pause(deployment_target),
            crate::cloud_provider::service::Action::Nothing => Ok(()),
        }
    }
}

pub trait StatefulService: Service + Create + Pause + Delete + Backup + Clone + Upgrade + Downgrade {
    fn exec_action(&self, deployment_target: &DeploymentTarget) -> Result<(), EngineError> {
        match self.action() {
            crate::cloud_provider::service::Action::Create => self.on_create(deployment_target),
            crate::cloud_provider::service::Action::Delete => self.on_delete(deployment_target),
            crate::cloud_provider::service::Action::Pause => self.on_pause(deployment_target),
            crate::cloud_provider::service::Action::Nothing => Ok(()),
        }
    }
}

pub trait Application: StatelessService {
    fn image(&self) -> &Image;
    fn set_image(&mut self, image: Image);
}

pub trait Router: StatelessService + Listen + Helm {
    fn domains(&self) -> Vec<&str>;
    fn has_custom_domains(&self) -> bool;
    fn check_domains(&self) -> Result<(), EngineError> {
        check_domain_for(
            ListenersHelper::new(self.listeners()),
            self.domains(),
            self.id(),
            self.context().execution_id(),
        )?;
        Ok(())
    }
}

pub trait Database: StatefulService {
    fn check_domains(&self, listeners: Listeners, domains: Vec<&str>) -> Result<(), EngineError> {
        check_domain_for(
            ListenersHelper::new(&listeners),
            domains,
            self.id(),
            self.context().execution_id(),
        )?;
        Ok(())
    }
}

pub trait Create {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_create_check(&self) -> Result<(), EngineError>;
    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
}

pub trait Pause {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_pause_check(&self) -> Result<(), EngineError>;
    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
}

pub trait Delete {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_delete_check(&self) -> Result<(), EngineError>;
    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
}

pub trait Backup {
    fn on_backup(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_backup_check(&self) -> Result<(), EngineError>;
    fn on_backup_error(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_restore(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_restore_check(&self) -> Result<(), EngineError>;
    fn on_restore_error(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
}

pub trait Clone {
    fn on_clone(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_clone_check(&self) -> Result<(), EngineError>;
    fn on_clone_error(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
}

pub trait Upgrade {
    fn on_upgrade(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_upgrade_check(&self) -> Result<(), EngineError>;
    fn on_upgrade_error(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
}

pub trait Downgrade {
    fn on_downgrade(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_downgrade_check(&self) -> Result<(), EngineError>;
    fn on_downgrade_error(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
}

pub trait Terraform {
    fn terraform_common_resource_dir_path(&self) -> String;
    fn terraform_resource_dir_path(&self) -> String;
}

pub trait Helm {
    fn helm_release_name(&self) -> String;
    fn helm_chart_dir(&self) -> String;
    fn helm_chart_values_dir(&self) -> String;
    fn helm_chart_external_name_service_dir(&self) -> String;
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum Action {
    Create,
    Pause,
    Delete,
    Nothing,
}

#[derive(Eq, PartialEq)]
pub struct DatabaseOptions {
    pub login: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub disk_size_in_gib: u32,
    pub database_disk_type: String,
}

#[derive(Eq, PartialEq)]
pub enum DatabaseType<'a> {
    PostgreSQL(&'a DatabaseOptions),
    MongoDB(&'a DatabaseOptions),
    MySQL(&'a DatabaseOptions),
    Redis(&'a DatabaseOptions),
}

#[derive(Eq, PartialEq)]
pub struct ServiceVersion {
    major: u32,
    minor: u32,
    update: u32,
    suffix: Option<&str>,
}

impl ServiceVersion {
    fn new(major: u32, minor: u32, update: u32, suffix: Option<&str>) {
        ServiceVersion {
            major,
            minor,
            update,
            suffix,
        }
    }

    fn to_string(&self) -> String {
        match self.suffix {
            Some(s) => format!("{}.{}.{}{}", self.major, self.minor, self.update, s),
            None => format!("{}.{}.{}", self.major, self.minor, self.update),
        }
    }

    fn to_major_string(&self) -> String {
        format!("{}", self.major),
    }

    fn to_major_minor_string(&self) -> String {
        format!("{}.{}", self.major, self.minor),
    }
}

#[derive(Eq, PartialEq)]
pub enum ServiceType<'a> {
    Application,
    Database(DatabaseType<'a>),
    Router,
}

impl<'a> ServiceType<'a> {
    pub fn name(&self) -> &str {
        match self {
            ServiceType::Application => "Application",
            ServiceType::Database(db_type) => match db_type {
                DatabaseType::PostgreSQL(_) => "PostgreSQL database",
                DatabaseType::MongoDB(_) => "MongoDB database",
                DatabaseType::MySQL(_) => "MySQL database",
                DatabaseType::Redis(_) => "Redis database",
            },
            ServiceType::Router => "Router",
        }
    }
}

pub fn debug_logs<T>(service: &T, deployment_target: &DeploymentTarget) -> Vec<String>
where
    T: Service + ?Sized,
{
    match deployment_target {
        DeploymentTarget::ManagedServices(_, _) => Vec::new(), // TODO retrieve logs from managed service?
        DeploymentTarget::SelfHosted(kubernetes, environment) => {
            match get_stateless_resource_information_for_user(*kubernetes, *environment, service) {
                Ok(lines) => lines,
                Err(err) => {
                    error!(
                        "error while retrieving debug logs from {} {}; error: {:?}",
                        service.service_type().name(),
                        service.name_with_id(),
                        err
                    );
                    Vec::new()
                }
            }
        }
    }
}

pub fn default_tera_context(
    service: &dyn Service,
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
) -> TeraContext {
    let mut context = TeraContext::new();

    context.insert("id", service.id());
    context.insert("owner_id", environment.owner_id.as_str());
    context.insert("project_id", environment.project_id.as_str());
    context.insert("organization_id", environment.organization_id.as_str());
    context.insert("environment_id", environment.id.as_str());
    context.insert("region", kubernetes.region());
    context.insert("zone", kubernetes.zone());
    context.insert("name", service.name());
    context.insert("sanitized_name", &service.sanitized_name());
    context.insert("namespace", environment.namespace());
    context.insert("cluster_name", kubernetes.name());
    context.insert("total_cpus", &service.total_cpus());
    context.insert("total_ram_in_mib", &service.total_ram_in_mib());
    context.insert("total_instances", &service.total_instances());

    context.insert("is_private_port", &service.private_port().is_some());
    if service.private_port().is_some() {
        context.insert("private_port", &service.private_port().unwrap());
    }

    context.insert("version", service.version());

    context
}

/// deploy a stateless service created by the user (E.g: App or External Service)
/// the difference with `deploy_service(..)` is that this function provides the thrown error in case of failure
pub fn deploy_user_stateless_service<T>(target: &DeploymentTarget, service: &T) -> Result<(), EngineError>
where
    T: Service + Helm,
{
    deploy_stateless_service(
        target,
        service,
        service.engine_error(
            EngineErrorCause::User(
                "Your application has failed to start. \
                Ensure you can run it without issues with `qovery run` and check its logs from the web interface or the CLI with `qovery log`. \
                This issue often occurs due to ports misconfiguration. Make sure you exposed the correct port (using EXPOSE statement in Dockerfile or via Qovery configuration).",
            ),
            format!(
                "{} {} has failed to start ⤬",
                service.service_type().name(),
                service.name_with_id()
            ),
        ),
    )
}

/// deploy a stateless service (app, router, database...) on Kubernetes
pub fn deploy_stateless_service<T>(
    target: &DeploymentTarget,
    service: &T,
    thrown_error: EngineError,
) -> Result<(), EngineError>
where
    T: Service + Helm,
{
    let (kubernetes, environment) = match target {
        DeploymentTarget::ManagedServices(k, env) => (*k, *env),
        DeploymentTarget::SelfHosted(k, env) => (*k, *env),
    };

    let workspace_dir = service.workspace_directory();
    let tera_context = service.tera_context(target)?;

    let _ = cast_simple_error_to_engine_error(
        service.engine_error_scope(),
        service.context().execution_id(),
        crate::template::generate_and_copy_all_files_into_dir(
            service.helm_chart_dir(),
            workspace_dir.as_str(),
            &tera_context,
        ),
    )?;

    let helm_release_name = service.helm_release_name();
    let kubernetes_config_file_path = kubernetes.config_file_path()?;

    // define labels to add to namespace
    let namespace_labels = service.context().resource_expiration_in_seconds().map(|_| {
        vec![
            (LabelsContent {
                name: "ttl".to_string(),
                value: format! {"{}", service.context().resource_expiration_in_seconds().unwrap()},
            }),
        ]
    });

    // create a namespace with labels if do not exists
    let _ = cast_simple_error_to_engine_error(
        service.engine_error_scope(),
        service.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_create_namespace(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            namespace_labels,
            kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    )?;

    // do exec helm upgrade and return the last deployment status
    let helm_history_row = cast_simple_error_to_engine_error(
        service.engine_error_scope(),
        service.context().execution_id(),
        crate::cmd::helm::helm_exec_with_upgrade_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            workspace_dir.as_str(),
            service.start_timeout(),
            kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    )?;

    // check deployment status
    if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
        return Err(thrown_error);
    }

    let _ = cast_simple_error_to_engine_error(
        service.engine_error_scope(),
        service.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_is_pod_ready_with_retry(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            service.selector().as_str(),
            kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    )?;

    Ok(())
}

/// do specific operations on a stateless service deployment error
pub fn deploy_stateless_service_error<T>(target: &DeploymentTarget, service: &T) -> Result<(), EngineError>
where
    T: Service + Helm,
{
    let (kubernetes, environment) = match target {
        DeploymentTarget::ManagedServices(k, env) => (*k, *env),
        DeploymentTarget::SelfHosted(k, env) => (*k, *env),
    };

    let kubernetes_config_file_path = kubernetes.config_file_path()?;
    let helm_release_name = service.helm_release_name();

    let history_rows = cast_simple_error_to_engine_error(
        service.engine_error_scope(),
        service.context().execution_id(),
        crate::cmd::helm::helm_exec_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            &kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    )?;

    if history_rows.len() == 1 {
        cast_simple_error_to_engine_error(
            service.engine_error_scope(),
            service.context().execution_id(),
            crate::cmd::helm::helm_exec_uninstall(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name.as_str(),
                kubernetes.cloud_provider().credentials_environment_variables(),
            ),
        )?;
    }

    Ok(())
}

pub fn scale_down_database(
    target: &DeploymentTarget,
    service: &impl Database,
    replicas_count: usize,
) -> Result<(), EngineError> {
    let (kubernetes, environment) = match target {
        DeploymentTarget::ManagedServices(_, _) => {
            info!("Doing nothing for pause database as it is a managed service");
            return Ok(());
        }
        DeploymentTarget::SelfHosted(k, env) => (*k, *env),
    };

    let scaledown_ret = kubectl_exec_scale_replicas_by_selector(
        kubernetes.config_file_path()?,
        kubernetes.cloud_provider().credentials_environment_variables(),
        environment.namespace(),
        Statefulset,
        format!("databaseId={}", service.id()).as_str(),
        replicas_count as u32,
    );

    cast_simple_error_to_engine_error(
        service.engine_error_scope(),
        service.context().execution_id(),
        scaledown_ret,
    )
}

pub fn scale_down_application(
    target: &DeploymentTarget,
    service: &impl StatelessService,
    replicas_count: usize,
    scaling_kind: ScalingKind,
) -> Result<(), EngineError> {
    let (kubernetes, environment) = match target {
        DeploymentTarget::ManagedServices(_, _) => {
            return Err(EngineError {
                cause: EngineErrorCause::Internal,
                scope: EngineErrorScope::Engine,
                execution_id: service.context().execution_id().to_string(),
                message: Some(format!("Cannot scale down managed service: {}", service.name_with_id())),
            })
        }
        DeploymentTarget::SelfHosted(k, env) => (*k, *env),
    };
    let scaledown_ret = kubectl_exec_scale_replicas_by_selector(
        kubernetes.config_file_path()?,
        kubernetes.cloud_provider().credentials_environment_variables(),
        environment.namespace(),
        scaling_kind,
        format!("appId={}", service.id()).as_str(),
        replicas_count as u32,
    );

    cast_simple_error_to_engine_error(
        service.engine_error_scope(),
        service.context().execution_id(),
        scaledown_ret,
    )
}

pub fn delete_router<T>(target: &DeploymentTarget, service: &T, is_error: bool) -> Result<(), EngineError>
where
    T: Router,
{
    send_progress_on_long_task(service, crate::cloud_provider::service::Action::Delete, || {
        delete_stateless_service(target, service, is_error)
    })
}

pub fn delete_stateless_service<T>(target: &DeploymentTarget, service: &T, is_error: bool) -> Result<(), EngineError>
where
    T: Service + Helm,
{
    let (kubernetes, environment) = match target {
        DeploymentTarget::ManagedServices(k, env) => (*k, *env),
        DeploymentTarget::SelfHosted(k, env) => (*k, *env),
    };

    let helm_release_name = service.helm_release_name();

    if is_error {
        let _ = get_stateless_resource_information(kubernetes, environment, service.selector().as_str())?;
    }

    // clean the resource
    let _ = helm_uninstall_release(kubernetes, environment, helm_release_name.as_str())?;

    Ok(())
}

pub fn deploy_stateful_service<T>(target: &DeploymentTarget, service: &T) -> Result<(), EngineError>
where
    T: StatefulService + Helm + Terraform,
{
    let workspace_dir = service.workspace_directory();

    match target {
        DeploymentTarget::ManagedServices(kubernetes, _) => {
            // use terraform
            info!(
                "deploy {} with name {} on {}",
                service.service_type().name(),
                service.name_with_id(),
                kubernetes.cloud_provider().kind().name()
            );

            let context = service.tera_context(target)?;

            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.terraform_common_resource_dir_path(),
                    &workspace_dir,
                    &context,
                ),
            )?;

            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.terraform_resource_dir_path(),
                    workspace_dir.as_str(),
                    &context,
                ),
            )?;

            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.helm_chart_external_name_service_dir(),
                    format!("{}/{}", workspace_dir, "external-name-svc"),
                    &context,
                ),
            )?;

            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::cmd::terraform::terraform_init_validate_plan_apply(
                    workspace_dir.as_str(),
                    service.context().is_dry_run_deploy(),
                ),
            )?;
        }
        DeploymentTarget::SelfHosted(kubernetes, environment) => {
            // use helm
            info!(
                "deploy {} with name {} on {:?} Kubernetes cluster id {}",
                service.service_type().name(),
                service.name_with_id(),
                kubernetes.cloud_provider().kind().name(),
                kubernetes.id()
            );

            let context = service.tera_context(target)?;
            let kubernetes_config_file_path = kubernetes.config_file_path()?;

            // default chart
            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.helm_chart_dir(),
                    workspace_dir.as_str(),
                    &context,
                ),
            )?;

            // overwrite with our chart values
            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.helm_chart_values_dir(),
                    workspace_dir.as_str(),
                    &context,
                ),
            )?;

            // define labels to add to namespace
            let namespace_labels = service.context().resource_expiration_in_seconds().map(|_| {
                vec![
                    (LabelsContent {
                        name: "ttl".into(),
                        value: format!("{}", service.context().resource_expiration_in_seconds().unwrap()),
                    }),
                ]
            });

            // create a namespace with labels if it does not exist
            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::cmd::kubectl::kubectl_exec_create_namespace(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    namespace_labels,
                    kubernetes.cloud_provider().credentials_environment_variables(),
                ),
            )?;

            // do exec helm upgrade and return the last deployment status
            let helm_history_row = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::cmd::helm::helm_exec_with_upgrade_history(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    service.helm_release_name().as_str(),
                    workspace_dir.as_str(),
                    service.start_timeout(),
                    kubernetes.cloud_provider().credentials_environment_variables(),
                ),
            )?;

            // check deployment status
            if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
                return Err(service.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "{} service fails to be deployed (before start)",
                        service.service_type().name()
                    ),
                ));
            }

            // check app status
            match crate::cmd::kubectl::kubectl_exec_is_pod_ready_with_retry(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                service.selector().as_str(),
                kubernetes.cloud_provider().credentials_environment_variables(),
            ) {
                Ok(Some(true)) => {}
                _ => {
                    return Err(service.engine_error(
                        EngineErrorCause::Internal,
                        format!(
                            "{} database {} failed to start after several retries",
                            service.service_type().name(),
                            service.name_with_id()
                        ),
                    ));
                }
            }
        }
    }

    Ok(())
}

pub fn delete_stateful_service<T>(target: &DeploymentTarget, service: &T) -> Result<(), EngineError>
where
    T: StatefulService + Helm + Terraform,
{
    match target {
        DeploymentTarget::ManagedServices(kubernetes, environment) => {
            let workspace_dir = service.workspace_directory();
            let tera_context = service.tera_context(target)?;

            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.terraform_common_resource_dir_path(),
                    workspace_dir.as_str(),
                    &tera_context,
                ),
            )?;

            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.terraform_resource_dir_path(),
                    workspace_dir.as_str(),
                    &tera_context,
                ),
            )?;

            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.helm_chart_external_name_service_dir(),
                    format!("{}/{}", workspace_dir, "external-name-svc"),
                    &tera_context,
                ),
            )?;

            let _ = cast_simple_error_to_engine_error(
                service.engine_error_scope(),
                service.context().execution_id(),
                crate::template::generate_and_copy_all_files_into_dir(
                    service.helm_chart_external_name_service_dir(),
                    workspace_dir.as_str(),
                    &tera_context,
                ),
            )?;

            match crate::cmd::terraform::terraform_init_validate_destroy(workspace_dir.as_str(), true) {
                Ok(_) => {
                    info!("deleting secret containing tfstates");
                    let _ = delete_terraform_tfstate_secret(
                        *kubernetes,
                        environment.namespace(),
                        &get_tfstate_name(service),
                    );
                }
                Err(e) => {
                    let message = format!("{:?}", e);
                    error!("{}", message);

                    return Err(service.engine_error(EngineErrorCause::Internal, message));
                }
            }
        }
        DeploymentTarget::SelfHosted(kubernetes, environment) => {
            let helm_release_name = service.helm_release_name();

            // clean the resource
            let _ = helm_uninstall_release(*kubernetes, *environment, helm_release_name.as_str())?;
        }
    }

    Ok(())
}

pub fn check_service_version<T>(result: Result<String, StringError>, service: &T) -> Result<String, EngineError>
where
    T: Service + Listen,
{
    let listeners_helper = ListenersHelper::new(service.listeners());

    match result {
        Ok(version) => {
            if service.version() != version.as_str() {
                let message = format!(
                    "{} version {} has been requested by the user; but matching version is {}",
                    service.service_type().name(),
                    service.version(),
                    version.as_str()
                );

                info!("{}", message.as_str());

                let progress_info = ProgressInfo::new(
                    service.progress_scope(),
                    ProgressLevel::Info,
                    Some(message),
                    service.context().execution_id(),
                );

                listeners_helper.deployment_in_progress(progress_info);
            }

            Ok(version)
        }
        Err(err) => {
            let message = format!(
                "{} version {} is not supported!",
                service.service_type().name(),
                service.version(),
            );

            error!("{}", message.as_str());

            let progress_info = ProgressInfo::new(
                service.progress_scope(),
                ProgressLevel::Error,
                Some(message),
                service.context().execution_id(),
            );

            listeners_helper.deployment_error(progress_info);

            error!("{}", err);

            Err(service.engine_error(
                EngineErrorCause::User(
                    "The provided database version is not supported, please refer to the \
                documentation https://docs.qovery.com",
                ),
                err,
            ))
        }
    }
}

fn delete_terraform_tfstate_secret(
    kubernetes: &dyn Kubernetes,
    namespace: &str,
    secret_name: &str,
) -> Result<(), EngineError> {
    let config_file_path = kubernetes.config_file_path()?;

    //create the namespace to insert the tfstate in secrets
    let _ = kubectl_exec_delete_secret(
        config_file_path,
        namespace,
        secret_name,
        kubernetes.cloud_provider().credentials_environment_variables(),
    );

    Ok(())
}

pub enum CheckAction {
    Deploy,
    Pause,
    Delete,
}

pub fn check_kubernetes_service_error<T>(
    result: Result<(), EngineError>,
    kubernetes: &dyn Kubernetes,
    service: &Box<T>,
    deployment_target: &DeploymentTarget,
    listeners_helper: &ListenersHelper,
    action_verb: &str,
    action: CheckAction,
) -> Result<(), EngineError>
where
    T: Service + ?Sized,
{
    let progress_info = ProgressInfo::new(
        service.progress_scope(),
        ProgressLevel::Info,
        Some(format!(
            "{} {} {}",
            action_verb,
            service.service_type().name().to_lowercase(),
            service.name()
        )),
        kubernetes.context().execution_id(),
    );

    match action {
        CheckAction::Deploy => listeners_helper.deployment_in_progress(progress_info),
        CheckAction::Pause => listeners_helper.pause_in_progress(progress_info),
        CheckAction::Delete => listeners_helper.delete_in_progress(progress_info),
    }

    match result {
        Err(err) => {
            error!(
                "{} error with {} {} , id: {} => {:?}",
                action_verb,
                service.service_type().name(),
                service.name(),
                service.id(),
                err
            );

            let progress_info = ProgressInfo::new(
                service.progress_scope(),
                ProgressLevel::Error,
                Some(format!(
                    "{} error {} {} : error => {:?}",
                    action_verb,
                    service.service_type().name().to_lowercase(),
                    service.name(),
                    err
                )),
                kubernetes.context().execution_id(),
            );

            match action {
                CheckAction::Deploy => listeners_helper.deployment_error(progress_info),
                CheckAction::Pause => listeners_helper.pause_error(progress_info),
                CheckAction::Delete => listeners_helper.delete_error(progress_info),
            }

            let debug_logs = service.debug_logs(deployment_target);
            let debug_logs_string = if !debug_logs.is_empty() {
                debug_logs.join("\n")
            } else {
                String::from("<no debug logs>")
            };

            info!("{}", debug_logs_string);

            let progress_info = ProgressInfo::new(
                service.progress_scope(),
                ProgressLevel::Debug,
                Some(debug_logs_string),
                kubernetes.context().execution_id(),
            );

            match action {
                CheckAction::Deploy => listeners_helper.deployment_error(progress_info),
                CheckAction::Pause => listeners_helper.pause_error(progress_info),
                CheckAction::Delete => listeners_helper.delete_error(progress_info),
            }

            return Err(err);
        }
        _ => {
            let progress_info = ProgressInfo::new(
                service.progress_scope(),
                ProgressLevel::Info,
                Some(format!(
                    "{} succeeded for {} {}",
                    action_verb,
                    service.service_type().name().to_lowercase(),
                    service.name()
                )),
                kubernetes.context().execution_id(),
            );

            match action {
                CheckAction::Deploy => listeners_helper.deployment_in_progress(progress_info),
                CheckAction::Pause => listeners_helper.pause_in_progress(progress_info),
                CheckAction::Delete => listeners_helper.delete_in_progress(progress_info),
            }

            Ok(())
        }
    }
}

pub type Logs = String;
pub type Describe = String;

/// return debug information line by line to help the user to understand what's going on,
/// and why its app does not start
pub fn get_stateless_resource_information_for_user<T>(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    service: &T,
) -> Result<Vec<String>, EngineError>
where
    T: Service + ?Sized,
{
    let selector = service.selector();
    let kubernetes_config_file_path = kubernetes.config_file_path()?;
    let mut result = Vec::with_capacity(50);

    // get logs
    let logs = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_logs(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector.as_str(),
            kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    )
    .unwrap_or_else(|_| vec![format!("Unable to retrieve logs for pod: {}", selector)]);

    let _ = result.extend(logs);

    // get pod state
    let pods = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_get_pod(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector.as_str(),
            kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    )
    .map_or_else(|_| vec![], |pods| pods.items);

    for pod in pods {
        for container_condition in pod.status.conditions {
            if container_condition.status.to_ascii_lowercase() == "false" {
                result.push(format!(
                    "Condition not met to start the container: {} -> {}: {}",
                    container_condition.typee,
                    container_condition.reason.unwrap_or_default(),
                    container_condition.message.unwrap_or_default()
                ))
            }
        }
        for container_status in pod.status.container_statuses.unwrap_or_default() {
            if let Some(last_state) = container_status.last_state {
                if let Some(terminated) = last_state.terminated {
                    if let Some(message) = terminated.message {
                        result.push(format!("terminated state message: {}", message));
                    }

                    result.push(format!("terminated state exit code: {}", terminated.exit_code));
                }

                if let Some(waiting) = last_state.waiting {
                    if let Some(message) = waiting.message {
                        result.push(format!("waiting state message: {}", message));
                    }
                }
            }
        }
    }

    // get pod events
    let events = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_get_json_events(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    )
    .map_or_else(|_| vec![], |events| events.items);

    for event in events {
        if event.type_.to_lowercase() != "normal" {
            if let Some(message) = event.message {
                result.push(format!(
                    "{} {} {}: {}",
                    event.last_timestamp.unwrap_or_default(),
                    event.type_,
                    event.reason,
                    message
                ));
            }
        }
    }

    Ok(result)
}

/// show different output (kubectl describe, log..) for debug purpose
pub fn get_stateless_resource_information(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    selector: &str,
) -> Result<(Describe, Logs), EngineError> {
    let kubernetes_config_file_path = kubernetes.config_file_path()?;

    // exec describe pod...
    let describe = match cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_describe_pod(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector,
            kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    ) {
        Ok(output) => {
            info!("{}", output);
            output
        }
        Err(err) => {
            error!("{:?}", err);
            return Err(err);
        }
    };

    // exec logs...
    let logs = match cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_logs(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector,
            kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    ) {
        Ok(output) => {
            info!("{:?}", output);
            output.join("\n")
        }
        Err(err) => {
            error!("{:?}", err);
            return Err(err);
        }
    };

    Ok((describe, logs))
}

pub fn helm_uninstall_release(
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
    helm_release_name: &str,
) -> Result<(), EngineError> {
    let kubernetes_config_file_path = kubernetes.config_file_path()?;

    let history_rows = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::helm::helm_exec_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name,
            &kubernetes.cloud_provider().credentials_environment_variables(),
        ),
    )?;

    // if there is no valid history - then delete the helm chart
    let first_valid_history_row = history_rows.iter().find(|x| x.is_successfully_deployed());

    if first_valid_history_row.is_some() {
        cast_simple_error_to_engine_error(
            kubernetes.engine_error_scope(),
            kubernetes.context().execution_id(),
            crate::cmd::helm::helm_exec_uninstall(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name,
                kubernetes.cloud_provider().credentials_environment_variables(),
            ),
        )?;
    }

    Ok(())
}

/// This function call (start|pause|delete)_in_progress function every 10 seconds when a
/// long blocking task is running.
pub fn send_progress_on_long_task<S, R, F>(service: &S, action: Action, long_task: F) -> R
where
    S: Service + Listen,
    F: Fn() -> R,
{
    let waiting_message = match action {
        Action::Create => Some(format!(
            "{} '{}' deployment is in progress...",
            service.service_type().name(),
            service.name_with_id()
        )),
        Action::Pause => Some(format!(
            "{} '{}' pause is in progress...",
            service.service_type().name(),
            service.name_with_id()
        )),
        Action::Delete => Some(format!(
            "{} '{}' deletion is in progress...",
            service.service_type().name(),
            service.name_with_id()
        )),
        Action::Nothing => None,
    };

    send_progress_on_long_task_with_message(service, waiting_message, action, long_task)
}

/// This function call (start|pause|delete)_in_progress function every 10 seconds when a
/// long blocking task is running.
pub fn send_progress_on_long_task_with_message<S, M, R, F>(
    service: &S,
    waiting_message: Option<M>,
    action: Action,
    long_task: F,
) -> R
where
    S: Service + Listen,
    M: Into<String>,
    F: Fn() -> R,
{
    let listeners = std::clone::Clone::clone(service.listeners());

    let progress_info = ProgressInfo::new(
        service.progress_scope(),
        Info,
        waiting_message.map(|message| message.into()),
        service.context().execution_id(),
    );

    let (tx, rx) = mpsc::channel();

    // monitor thread to notify user while the blocking task is executed
    let _ = std::thread::Builder::new()
        .name("task-monitor".to_string())
        .spawn(move || {
            // stop the thread when the blocking task is done
            let listeners_helper = ListenersHelper::new(&listeners);
            let action = action;
            let progress_info = progress_info;

            loop {
                // do notify users here
                let progress_info = std::clone::Clone::clone(&progress_info);

                match action {
                    Action::Create => listeners_helper.deployment_in_progress(progress_info),
                    Action::Pause => listeners_helper.pause_in_progress(progress_info),
                    Action::Delete => listeners_helper.delete_in_progress(progress_info),
                    Action::Nothing => {} // should not happens
                };

                thread::sleep(Duration::from_secs(10));

                // watch for thread termination
                match rx.try_recv() {
                    Ok(_) | Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {}
                }
            }
        });

    let blocking_task_result = long_task();
    let _ = tx.send(());

    blocking_task_result
}

pub fn get_tfstate_suffix(service: &dyn Service) -> String {
    service.id().to_string()
}

// Name generated from TF secret suffix
// https://www.terraform.io/docs/backends/types/kubernetes.html#secret_suffix
// As mention the doc: Secrets will be named in the format: tfstate-{workspace}-{secret_suffix}.
pub fn get_tfstate_name(service: &dyn Service) -> String {
    format!("tfstate-default-{}", service.id())
}
