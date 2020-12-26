use std::net::TcpStream;

use retry::delay::Fixed;
use retry::OperationResult;
use tera::Context as TeraContext;
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::Resolver;

use crate::build_platform::Image;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd::helm::Timeout;
use crate::cmd::structs::LabelsContent;
use crate::error::cast_simple_error_to_engine_error;
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::models::{Context, Listen, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope};

pub trait Service {
    fn context(&self) -> &Context;
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn workspace_directory(&self) -> String {
        let dir_root = match self.service_type() {
            ServiceType::Application => "applications",
            ServiceType::ExternalService => "external-services",
            ServiceType::Database(_) => "databases",
            ServiceType::Router => "routers",
        };

        crate::fs::workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("{}/{}", dir_root, self.name()),
        )
    }
    fn version(&self) -> &str;
    fn action(&self) -> &Action;
    fn private_port(&self) -> Option<u16>;
    fn total_cpus(&self) -> String;
    fn total_ram_in_mib(&self) -> u32;
    fn total_instances(&self) -> u16;
    fn debug_logs(&self, deployment_target: &DeploymentTarget) -> Vec<String> {
        debug_logs(self, deployment_target)
    }
    fn is_listening(&self, ip: &str) -> bool {
        let private_port = match self.private_port() {
            Some(private_port) => private_port,
            _ => return false,
        };

        match TcpStream::connect(format!("{}:{}", ip, private_port)) {
            Ok(_) => true,
            Err(_) => false,
        }
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

    fn default_tera_context(
        &self,
        kubernetes: &dyn Kubernetes,
        environment: &Environment,
    ) -> TeraContext {
        let mut context = TeraContext::new();

        context.insert("id", self.id());
        context.insert("owner_id", environment.owner_id.as_str());
        context.insert("project_id", environment.project_id.as_str());
        context.insert("organization_id", environment.organization_id.as_str());
        context.insert("environment_id", environment.id.as_str());
        context.insert("region", kubernetes.region());
        context.insert("name", self.name());
        context.insert("namespace", environment.namespace());
        context.insert("cluster_name", kubernetes.name());
        context.insert("total_cpus", &self.total_cpus());
        context.insert("total_ram_in_mib", &self.total_ram_in_mib());
        context.insert("total_instances", &self.total_instances());

        context.insert("is_private_port", &self.private_port().is_some());
        if self.private_port().is_some() {
            context.insert("private_port", &self.private_port().unwrap());
        }

        context.insert("version", self.version());

        context
    }

    fn progress_scope(&self) -> ProgressScope {
        let id = self.id().to_string();

        match self.service_type() {
            ServiceType::Application => ProgressScope::Application { id },
            ServiceType::ExternalService => ProgressScope::ExternalService { id },
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

pub trait StatefulService:
    Service + Create + Pause + Delete + Backup + Clone + Upgrade + Downgrade
{
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
    fn start_timeout_in_seconds(&self) -> u32;
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Application(self.id().to_string(), self.name().to_string())
    }
    fn engine_error(&self, cause: EngineErrorCause, message: String) -> EngineError {
        EngineError::new(
            cause,
            self.engine_error_scope(),
            self.context().execution_id(),
            Some(message),
        )
    }
}

pub trait ExternalService: StatelessService {
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::ExternalService(self.id().to_string(), self.name().to_string())
    }
    fn engine_error(&self, cause: EngineErrorCause, message: String) -> EngineError {
        EngineError::new(
            cause,
            self.engine_error_scope(),
            self.context().execution_id(),
            Some(message),
        )
    }
}

pub trait Router: StatelessService + Listen {
    fn domains(&self) -> Vec<&str>;
    fn check_domains(&self) -> Result<(), EngineError> {
        let listeners_helper = ListenersHelper::new(self.listeners());

        let mut resolver_options = ResolverOpts::default();
        resolver_options.cache_size = 0;
        resolver_options.use_hosts_file = false;

        let resolver = match Resolver::new(ResolverConfig::google(), resolver_options) {
            Ok(resolver) => resolver,
            Err(err) => {
                error!("{:?}", err);
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "can't get domain resolver for router '{}'; Error: {:?}",
                        self.name_with_id(),
                        err
                    ),
                ));
            }
        };

        for domain in self.domains() {
            listeners_helper.start_in_progress(ProgressInfo::new(
                ProgressScope::Router {
                    id: self.id().into(),
                },
                ProgressLevel::Info,
                Some(format!(
                    "Let's check domain resolution for '{}'. Please wait, it can take some time...",
                    domain
                )),
                self.context().execution_id(),
            ));

            let fixed_iterable = Fixed::from_millis(3000).take(100);
            let check_result = retry::retry(fixed_iterable, || match resolver.lookup_ip(domain) {
                Ok(lookup_ip) => OperationResult::Ok(lookup_ip),
                Err(err) => {
                    let x = format!(
                        "Domain resolution check for '{}' is still in progress...",
                        domain
                    );

                    info!("{}", x);

                    listeners_helper.start_in_progress(ProgressInfo::new(
                        ProgressScope::Router {
                            id: self.id().into(),
                        },
                        ProgressLevel::Info,
                        Some(x),
                        self.context().execution_id(),
                    ));

                    OperationResult::Retry(err)
                }
            });

            match check_result {
                Ok(_) => {
                    let x = format!("Domain {} is ready! ⚡️", domain);

                    info!("{}", x);

                    listeners_helper.start_in_progress(ProgressInfo::new(
                        ProgressScope::Router {
                            id: self.id().into(),
                        },
                        ProgressLevel::Info,
                        Some(x),
                        self.context().execution_id(),
                    ));
                }
                Err(_) => {
                    let message = format!(
                        "Unable to check domain availability for '{}'. It can be due to a \
                        too long domain propagation. Note: this is not critical.",
                        domain
                    );

                    warn!("{}", message);

                    listeners_helper.error(ProgressInfo::new(
                        ProgressScope::Router {
                            id: self.id().into(),
                        },
                        ProgressLevel::Warn,
                        Some(message),
                        self.context().execution_id(),
                    ));
                }
            }
        }

        Ok(())
    }
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Router(self.id().to_string(), self.name().to_string())
    }
    fn engine_error(&self, cause: EngineErrorCause, message: String) -> EngineError {
        EngineError::new(
            cause,
            self.engine_error_scope(),
            self.context().execution_id(),
            Some(message),
        )
    }
}

pub trait Database: StatefulService {
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Database(
            self.id().to_string(),
            self.service_type().name().to_string(),
            self.name().to_string(),
        )
    }
    fn engine_error(&self, cause: EngineErrorCause, message: String) -> EngineError {
        EngineError::new(
            cause,
            self.engine_error_scope(),
            self.context().execution_id(),
            Some(message),
        )
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

pub trait Helm {
    fn helm_release_name(&self) -> String;
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
pub enum ServiceType<'a> {
    Application,
    ExternalService,
    Database(DatabaseType<'a>),
    Router,
}

impl<'a> ServiceType<'a> {
    pub fn name(&self) -> &str {
        match self {
            ServiceType::Application => "Application",
            ServiceType::ExternalService => "ExternalService",
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
                        "error while retrieving debug logs from database {}; error: {:?}",
                        service.name(),
                        err
                    );
                    Vec::new()
                }
            }
        }
    }
}

/// deploy an app on Kubernetes
pub fn deploy_application<T>(
    target: &DeploymentTarget,
    application: &T,
    charts_dir: &str,
    tera_context: &TeraContext,
) -> Result<(), EngineError>
where
    T: Application + Helm,
{
    let (kubernetes, environment) = match target {
        DeploymentTarget::ManagedServices(k, env) => (*k, *env),
        DeploymentTarget::SelfHosted(k, env) => (*k, *env),
    };

    let workspace_dir = application.workspace_directory();

    let _ = cast_simple_error_to_engine_error(
        application.engine_error_scope(),
        application.context().execution_id(),
        crate::template::generate_and_copy_all_files_into_dir(
            charts_dir,
            workspace_dir.as_str(),
            tera_context,
        ),
    )?;

    let helm_release_name = application.helm_release_name();
    let kubernetes_config_file_path = kubernetes.config_file_path()?;

    // define labels to add to namespace
    let namespace_labels = match application.context().resource_expiration_in_seconds() {
        Some(_) => Some(vec![
            (LabelsContent {
                name: "ttl".to_string(),
                value: format! {"{}", application.context().resource_expiration_in_seconds().unwrap()},
            }),
        ]),
        None => None,
    };

    // create a namespace with labels if do not exists
    let _ = cast_simple_error_to_engine_error(
        application.engine_error_scope(),
        application.context().execution_id(),
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
        application.engine_error_scope(),
        application.context().execution_id(),
        crate::cmd::helm::helm_exec_with_upgrade_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            workspace_dir.as_str(),
            Timeout::Value(application.start_timeout_in_seconds()),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    )?;

    // check deployment status
    if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
        return Err(application.engine_error(
            EngineErrorCause::User(
                "Your application didn't start for some reason. \
                Are you sure your application is correctly running? You can give a try by running \
                locally `qovery run`. You can also check the application log from the web \
                interface or the CLI with `qovery log`",
            ),
            format!(
                "Application {} has failed to start ⤬",
                application.name_with_id()
            ),
        ));
    }

    // check app status
    let selector = format!("app={}", application.name());

    let _ = cast_simple_error_to_engine_error(
        application.engine_error_scope(),
        application.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_is_pod_ready_with_retry(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector.as_str(),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    )?;

    Ok(())
}

/// do specific operations on app deployment error
pub fn deploy_application_error<T>(
    target: &DeploymentTarget,
    application: &T,
) -> Result<(), EngineError>
where
    T: Application + Helm,
{
    let (kubernetes, environment) = match target {
        DeploymentTarget::ManagedServices(k, env) => (*k, *env),
        DeploymentTarget::SelfHosted(k, env) => (*k, *env),
    };

    let kubernetes_config_file_path = kubernetes.config_file_path()?;
    let helm_release_name = application.helm_release_name();

    let history_rows = cast_simple_error_to_engine_error(
        application.engine_error_scope(),
        application.context().execution_id(),
        crate::cmd::helm::helm_exec_history(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            helm_release_name.as_str(),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    )?;

    if history_rows.len() == 1 {
        cast_simple_error_to_engine_error(
            application.engine_error_scope(),
            application.context().execution_id(),
            crate::cmd::helm::helm_exec_uninstall(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name.as_str(),
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables(),
            ),
        )?;
    }

    Ok(())
}

pub fn delete_stateless_service<T>(
    target: &DeploymentTarget,
    application: &T,
    is_error: bool,
) -> Result<(), EngineError>
where
    T: Application + Helm,
{
    let (kubernetes, environment) = match target {
        DeploymentTarget::ManagedServices(k, env) => (*k, *env),
        DeploymentTarget::SelfHosted(k, env) => (*k, *env),
    };

    let helm_release_name = application.helm_release_name();
    let selector = format!("app={}", application.name());

    if is_error {
        let _ = get_stateless_resource_information(kubernetes, environment, selector.as_str())?;
    }

    // clean the resource
    let _ = do_stateless_service_cleanup(kubernetes, environment, helm_release_name.as_str())?;

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
        CheckAction::Deploy => listeners_helper.start_in_progress(progress_info),
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
                CheckAction::Deploy => listeners_helper.start_error(progress_info),
                CheckAction::Pause => listeners_helper.pause_error(progress_info),
                CheckAction::Delete => listeners_helper.delete_error(progress_info),
            }

            let debug_logs = service.debug_logs(deployment_target);
            let debug_logs_string = if debug_logs.len() > 0 {
                debug_logs.join("\n")
            } else {
                String::from("<no debug logs>")
            };

            info!("{}", debug_logs_string);

            let progress_info = ProgressInfo::new(
                service.progress_scope(),
                ProgressLevel::Info,
                Some(debug_logs_string),
                kubernetes.context().execution_id(),
            );

            match action {
                CheckAction::Deploy => listeners_helper.start_error(progress_info),
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
                CheckAction::Deploy => listeners_helper.start_in_progress(progress_info),
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
    let selector = format!("app={}", service.name());

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
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    )?;

    let _ = result.extend(logs);

    // get pod state
    let pods = cast_simple_error_to_engine_error(
        kubernetes.engine_error_scope(),
        kubernetes.context().execution_id(),
        crate::cmd::kubectl::kubectl_exec_get_pod(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            selector.as_str(),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ),
    )?;

    for pod in pods.items {
        for container_status in pod.status.container_statuses {
            if let Some(last_state) = container_status.last_state {
                if let Some(terminated) = last_state.terminated {
                    if let Some(message) = terminated.message {
                        result.push(format!("terminated state message: {}", message));
                    }

                    result.push(format!(
                        "terminated state exit code: {}",
                        terminated.exit_code
                    ));
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
        crate::cmd::kubectl::kubectl_exec_get_event(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
            "involvedObject.kind=Pod",
        ),
    )?;

    let pod_name_start = format!("{}-", service.name());
    for event in events.items {
        if event.type_.to_lowercase() != "normal"
            && event.involved_object.name.starts_with(&pod_name_start)
        {
            if let Some(message) = event.message {
                result.push(format!("{}: {}", event.type_, message));
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
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
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
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
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

pub fn do_stateless_service_cleanup(
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
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
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
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables(),
            ),
        )?;
    }

    Ok(())
}
