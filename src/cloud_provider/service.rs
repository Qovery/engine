use std::net::TcpStream;

use retry::delay::Fixed;
use retry::OperationResult;
use tera::Context as TeraContext;
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::Resolver;

use crate::build_platform::Image;
use crate::cloud_provider::aws::databases::debug_logs;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::DeploymentTarget;
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
