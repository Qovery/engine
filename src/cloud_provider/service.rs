use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;

use crate::build_platform::Build;
use tera::Context as TeraContext;
use uuid::Uuid;

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cmd::kubectl::{kubectl_exec_delete_pod, kubectl_exec_get_pods};
use crate::cmd::structs::KubernetesPodStatusPhase;
use crate::cmd::terraform::TerraformError;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::models;
use crate::models::database::{Database, DatabaseMode};

use crate::models::types::{CloudProvider, VersionsNumber};

pub trait Service {
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn sanitized_name(&self) -> String;
    fn get_event_details(&self, stage: Stage) -> EventDetails;
    fn action(&self) -> &Action;
    // used to retrieve logs by using Kubernetes labels (selector)
    fn selector(&self) -> Option<String>;
    fn as_service(&self) -> &dyn Service;
    fn as_service_mut(&mut self) -> &mut dyn Service;
    fn build(&self) -> Option<&Build>;
    fn build_mut(&mut self) -> Option<&mut Build>;
}

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum Action {
    Create,
    Pause,
    Delete,
}

impl Action {
    pub fn to_environment_step(&self) -> EnvironmentStep {
        match self {
            Action::Create => EnvironmentStep::Deploy,
            Action::Pause => EnvironmentStep::Pause,
            Action::Delete => EnvironmentStep::Delete,
        }
    }
}

impl Display for Action {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                Action::Create => "Deployment",
                Action::Pause => "Pause",
                Action::Delete => "Deletion",
            },
        )
    }
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
    Job,
}

impl ServiceType {
    pub fn name(&self) -> String {
        match self {
            ServiceType::Application => "Application".to_string(),
            ServiceType::Database(db_type) => format!("{} database", db_type.to_string()),
            ServiceType::Router => "Router".to_string(),
            ServiceType::Container => "Container".to_string(),
            ServiceType::Job => "Job".to_string(),
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
    let file_content = match File::open(database_terraform_config_file) {
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

pub fn check_service_version<C: CloudProvider, M: DatabaseMode, T: models::database::DatabaseType<C, M>>(
    result: Result<String, CommandError>,
    service: &Database<C, M, T>,
    event_details: EventDetails,
) -> Result<ServiceVersionCheckResult, Box<EngineError>>
where
{
    let srv_version = service.version.to_string();
    match result {
        Ok(version) => {
            if srv_version != version.as_str() {
                let message = format!(
                    "{} version `{}` has been requested by the user; but matching version is `{}`",
                    service.service_type().name(),
                    srv_version,
                    version.as_str()
                );

                return Ok(ServiceVersionCheckResult::new(
                    VersionsNumber::from_str(&srv_version).map_err(|e| {
                        EngineError::new_version_number_parsing_error(event_details.clone(), srv_version.clone(), e)
                    })?,
                    VersionsNumber::from_str(&version).map_err(|e| {
                        EngineError::new_version_number_parsing_error(event_details.clone(), srv_version, e)
                    })?,
                    Some(message),
                ));
            }

            Ok(ServiceVersionCheckResult::new(
                VersionsNumber::from_str(&srv_version).map_err(|e| {
                    EngineError::new_version_number_parsing_error(event_details.clone(), srv_version, e)
                })?,
                VersionsNumber::from_str(&version).map_err(|e| {
                    EngineError::new_version_number_parsing_error(event_details.clone(), version.to_string(), e)
                })?,
                None,
            ))
        }
        Err(_err) => {
            let error =
                EngineError::new_unsupported_version_error(event_details, service.service_type().name(), srv_version);
            Err(Box::new(error))
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
) -> Result<(), Box<EngineError>>
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
                        return Err(Box::new(EngineError::new_k8s_service_issue(event_details, e)));
                    }
                }
            }

            Ok(())
        }
        Err(e) => Err(Box::new(EngineError::new_k8s_service_issue(event_details, e))),
    }
}
