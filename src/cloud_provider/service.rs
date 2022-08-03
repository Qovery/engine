use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;

use tera::Context as TeraContext;
use uuid::Uuid;

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cmd::kubectl::{kubectl_exec_delete_pod, kubectl_exec_get_pods};
use crate::cmd::structs::KubernetesPodStatusPhase;
use crate::cmd::terraform::TerraformError;
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventDetails, EventMessage, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::progress_listener::{Listener, Listeners, ProgressScope};
use crate::io_models::QoveryIdentifier;
use crate::logger::Logger;

use crate::models::types::VersionsNumber;

// todo: delete this useless trait
pub trait Service {
    fn context(&self) -> &Context;
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn sanitized_name(&self) -> String;
    fn workspace_directory(&self) -> String {
        let dir_root = match self.service_type() {
            ServiceType::Application => "applications",
            ServiceType::Database(_) => "databases",
            ServiceType::Router => "routers",
            ServiceType::Container => "containers",
        };

        crate::fs::workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("{}/{}", dir_root, self.long_id()),
        )
        .unwrap()
    }
    fn get_event_details(&self, stage: Stage) -> EventDetails {
        let context = self.context();
        EventDetails::new(
            None,
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            None,
            stage,
            self.to_transmitter(),
        )
    }
    fn version(&self) -> String;
    fn action(&self) -> &Action;
    // used to retrieve logs by using Kubernetes labels (selector)
    fn selector(&self) -> Option<String>;
    fn logger(&self) -> &dyn Logger;
    fn listeners(&self) -> &Listeners;
    fn add_listener(&mut self, listener: Listener);
    fn to_transmitter(&self) -> Transmitter;
    fn progress_scope(&self) -> ProgressScope {
        let id = self.id().to_string();

        match self.service_type() {
            ServiceType::Application => ProgressScope::Application { id },
            ServiceType::Database(_) => ProgressScope::Database { id },
            ServiceType::Router => ProgressScope::Router { id },
            ServiceType::Container => ProgressScope::Container { id: *self.long_id() },
        }
    }

    fn as_service(&self) -> &dyn Service;
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub enum Action {
    Create,
    Pause,
    Delete,
    Nothing,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Serialize)]
pub enum DatabaseType {
    PostgreSQL,
    MongoDB,
    MySQL,
    Redis,
}

impl ToString for DatabaseType {
    fn to_string(&self) -> String {
        match self {
            DatabaseType::PostgreSQL => "PostgreSQL".to_string(),
            DatabaseType::MongoDB => "MongoDB".to_string(),
            DatabaseType::MySQL => "MySQL".to_string(),
            DatabaseType::Redis => "Redis".to_string(),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ServiceType {
    Application,
    Database(DatabaseType),
    Router,
    Container,
}

impl ServiceType {
    pub fn name(&self) -> String {
        match self {
            ServiceType::Application => "Application".to_string(),
            ServiceType::Database(db_type) => format!("{} database", db_type.to_string()),
            ServiceType::Router => "Router".to_string(),
            ServiceType::Container => "Container".to_string(),
        }
    }
}

impl ToString for ServiceType {
    fn to_string(&self) -> String {
        self.name()
    }
}

pub fn default_tera_context(
    service: &dyn Service,
    kubernetes: &dyn Kubernetes,
    environment: &Environment,
) -> TeraContext {
    let mut context = TeraContext::new();
    context.insert("id", service.id());
    context.insert("long_id", service.long_id());
    context.insert("owner_id", environment.owner_id.as_str());
    context.insert("project_id", environment.project_id.as_str());
    context.insert("project_long_id", &environment.project_long_id);
    context.insert("organization_id", environment.organization_id.as_str());
    context.insert("organization_long_id", &environment.organization_long_id);
    context.insert("environment_id", environment.id.as_str());
    context.insert("environment_long_id", &environment.long_id);
    context.insert("region", kubernetes.region());
    context.insert("zone", kubernetes.zone());
    context.insert("name", service.name());
    context.insert("sanitized_name", &service.sanitized_name());
    context.insert("namespace", environment.namespace());
    context.insert("cluster_name", kubernetes.name());
    context.insert("version", &service.version());

    context
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseTerraformConfig {
    #[serde(rename = "database_target_id")]
    pub target_id: String,
    #[serde(rename = "database_target_hostname")]
    pub target_hostname: String,
    #[serde(rename = "database_target_fqdn_id")]
    pub target_fqdn_id: String,
    #[serde(rename = "database_target_fqdn")]
    pub target_fqdn: String,
}

pub fn get_database_terraform_config(
    database_terraform_config_file: &str,
) -> Result<DatabaseTerraformConfig, TerraformError> {
    let file_content = match File::open(&database_terraform_config_file) {
        Ok(f) => f,
        Err(e) => {
            return Err(TerraformError::ConfigFileNotFound {
                path: database_terraform_config_file.to_string(),
                raw_message: format!("Terraform config error, database config cannot be found.\n{}", e),
            });
        }
    };

    let reader = BufReader::new(file_content);
    match serde_json::from_reader(reader) {
        Ok(config) => Ok(config),
        Err(e) => Err(TerraformError::ConfigFileInvalidContent {
            path: database_terraform_config_file.to_string(),
            raw_message: format!("Terraform config error, database config cannot be parsed.\n{}", e),
        }),
    }
}

pub struct ServiceVersionCheckResult {
    requested_version: VersionsNumber,
    matched_version: VersionsNumber,
    message: Option<String>,
}

impl ServiceVersionCheckResult {
    pub fn new(requested_version: VersionsNumber, matched_version: VersionsNumber, message: Option<String>) -> Self {
        ServiceVersionCheckResult {
            requested_version,
            matched_version,
            message,
        }
    }

    pub fn matched_version(&self) -> VersionsNumber {
        self.matched_version.clone()
    }

    pub fn requested_version(&self) -> &VersionsNumber {
        &self.requested_version
    }

    pub fn message(&self) -> Option<String> {
        self.message.clone()
    }
}

pub fn check_service_version<T>(
    result: Result<String, CommandError>,
    service: &T,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<ServiceVersionCheckResult, EngineError>
where
    T: Service,
{
    match result {
        Ok(version) => {
            if service.version() != version.as_str() {
                let message = format!(
                    "{} version `{}` has been requested by the user; but matching version is `{}`",
                    service.service_type().name(),
                    service.version(),
                    version.as_str()
                );

                logger.log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(message.to_string()),
                ));

                return Ok(ServiceVersionCheckResult::new(
                    VersionsNumber::from_str(&service.version()).map_err(|e| {
                        EngineError::new_version_number_parsing_error(event_details.clone(), service.version(), e)
                    })?,
                    VersionsNumber::from_str(&version).map_err(|e| {
                        EngineError::new_version_number_parsing_error(event_details.clone(), version.to_string(), e)
                    })?,
                    Some(message),
                ));
            }

            Ok(ServiceVersionCheckResult::new(
                VersionsNumber::from_str(&service.version()).map_err(|e| {
                    EngineError::new_version_number_parsing_error(event_details.clone(), service.version(), e)
                })?,
                VersionsNumber::from_str(&version).map_err(|e| {
                    EngineError::new_version_number_parsing_error(event_details.clone(), version.to_string(), e)
                })?,
                None,
            ))
        }
        Err(_err) => {
            let error = EngineError::new_unsupported_version_error(
                event_details,
                service.service_type().name(),
                service.version(),
            );

            logger.log(EngineEvent::Error(error.clone(), None));

            Err(error)
        }
    }
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

pub fn delete_pending_service<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
    event_details: EventDetails,
) -> Result<(), EngineError>
where
    P: AsRef<Path>,
{
    match kubectl_exec_get_pods(&kubernetes_config, Some(namespace), Some(selector), envs.clone()) {
        Ok(pods) => {
            for pod in pods.items {
                if pod.status.phase == KubernetesPodStatusPhase::Pending {
                    if let Err(e) = kubectl_exec_delete_pod(
                        &kubernetes_config,
                        pod.metadata.namespace.as_str(),
                        pod.metadata.name.as_str(),
                        envs.clone(),
                    ) {
                        return Err(EngineError::new_k8s_service_issue(event_details, e));
                    }
                }
            }

            Ok(())
        }
        Err(e) => Err(EngineError::new_k8s_service_issue(event_details, e)),
    }
}
