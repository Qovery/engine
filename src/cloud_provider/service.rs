use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use crate::build_platform::Build;
use strum_macros::EnumIter;
use tera::Context as TeraContext;
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::models::InvalidStatefulsetStorage;
use crate::cmd::kubectl::{kubectl_exec_delete_pod, kubectl_exec_get_pods};
use crate::cmd::structs::KubernetesPodStatusPhase;
use crate::cmd::terraform::TerraformError;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::kubers_utils::{
    kube_create_from_resource, kube_delete_all_from_selector, kube_edit_pvc_size, kube_get_resources_by_selector,
    kube_rollout_restart_statefulset, KubeDeleteMode,
};
use crate::models;
use crate::models::database::{Database, DatabaseMode};

use crate::models::types::{CloudProvider, VersionsNumber};
use crate::runtime::block_on;

pub trait Service: Send {
    fn service_type(&self) -> ServiceType;
    fn id(&self) -> &str;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn version(&self) -> String;
    fn kube_name(&self) -> &str;
    fn kube_label_selector(&self) -> String;
    fn get_event_details(&self, stage: Stage) -> EventDetails;
    fn action(&self) -> &Action;
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
    Restart,
}

impl Action {
    pub fn to_environment_step(&self) -> EnvironmentStep {
        match self {
            Action::Create => EnvironmentStep::Deploy,
            Action::Pause => EnvironmentStep::Pause,
            Action::Delete => EnvironmentStep::Delete,
            Action::Restart => EnvironmentStep::Restart,
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
                Action::Restart => "Restart",
            },
        )
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Serialize, EnumIter)]
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
    HelmChart,
}

impl ServiceType {
    pub fn name(&self) -> String {
        match self {
            ServiceType::Application => "Application".to_string(),
            ServiceType::Database(db_type) => format!("{} database", db_type.to_string()),
            ServiceType::Router => "Router".to_string(),
            ServiceType::Container => "Container".to_string(),
            ServiceType::Job => "Job".to_string(),
            ServiceType::HelmChart => "HelmChart".to_string(),
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
    context.insert("sanitized_name", &service.kube_name());
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
                raw_message: format!("Terraform config error, database config cannot be found.\n{e}"),
            });
        }
    };

    let reader = BufReader::new(file_content);
    match serde_json::from_reader(reader) {
        Ok(config) => Ok(config),
        Err(e) => Err(TerraformError::ConfigFileInvalidContent {
            path: database_terraform_config_file.to_string(),
            raw_message: format!("Terraform config error, database config cannot be parsed.\n{e}"),
        }),
    }
}

// TODO(benjaminch): to be remove, doesn't make any sense now
#[deprecated(note = "This struct doesn't make more sense now, we should not change requested service version")]
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

pub async fn increase_storage_size(
    namespace: &str,
    invalid_statefulset: &InvalidStatefulsetStorage,
    event_details: &EventDetails,
    client: &kube::Client,
) -> Result<(), Box<EngineError>> {
    // get current statefulset before its deletion
    let mut current_statefulset = match kube_get_resources_by_selector::<StatefulSet>(
        client,
        namespace,
        &invalid_statefulset.statefulset_selector,
    )
    .await
    .map_err(|e| {
        EngineError::new_k8s_cannot_get_statefulset(
            event_details.clone(),
            namespace,
            &invalid_statefulset.statefulset_selector,
            e,
        )
    })?
    .items
    .first()
    {
        None => {
            return Err(Box::new(EngineError::new_k8s_cannot_get_statefulset(
                event_details.clone(),
                namespace,
                &invalid_statefulset.statefulset_selector,
                CommandError::new_from_safe_message(format!(
                    "Unable to get statefulset with selector {}",
                    invalid_statefulset.statefulset_selector
                )),
            )))
        }
        Some(statefulset) => statefulset.clone(),
    };

    // remove immutable/useless fields from statefulset
    current_statefulset.metadata.resource_version = None;
    current_statefulset.metadata.uid = None;
    current_statefulset.status = None;

    // adjust capacity of volume
    for invalid_pvc in &invalid_statefulset.invalid_pvcs {
        kube_edit_pvc_size(client, namespace, invalid_pvc).await.map_err(|e| {
            EngineError::new_k8s_cannot_edit_pvc(event_details.clone(), invalid_pvc.pvc_name.to_string(), e)
        })?;

        // todo(pmavro): find a way to get the name of the volume claim template
        let persistent_volume_claim_template_name = match invalid_statefulset.service_type {
            ServiceType::Database(type_) => match type_ {
                DatabaseType::Redis => "redis-data",
                _ => "data",
            },
            _ => &invalid_pvc.pvc_name,
        }
        .to_string();

        // edit statefulset volume claim templates in order to stick to new size
        if let Some(spec) = current_statefulset.spec.as_mut() {
            if let Some(volumes) = spec.volume_claim_templates.as_mut() {
                for volume in volumes {
                    if let Some(name) = &volume.metadata.name {
                        // find invalid volume claim template regarding invalid pvc name
                        if persistent_volume_claim_template_name.starts_with(name) {
                            if let Some(v_spec) = volume.spec.as_mut() {
                                if let Some(v_res) = v_spec.resources.as_mut() {
                                    if let Some(v_req) = v_res.requests.as_mut() {
                                        if let Some(storage) = v_req.get_mut("storage") {
                                            // edit storage size
                                            storage.0 = format!("{}Gi", invalid_pvc.required_disk_size_in_gib);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // orphan delete statefulset so its Pods remain in cluster
    kube_delete_all_from_selector::<StatefulSet>(
        client,
        invalid_statefulset.statefulset_selector.as_str(),
        namespace,
        KubeDeleteMode::Orphan,
    )
    .await
    .map_err(|e| {
        Box::new(EngineError::new_k8s_cannot_orphan_delete(
            event_details.clone(),
            invalid_statefulset.statefulset_selector.as_str(),
            CommandError::new_from_safe_message(e.to_string()),
        ))
    })?;

    // await for statefulset deletion before delete
    info!("Waiting for orphan StatefulSet deletion to perform.");
    let now = Instant::now();
    let deletion_timeout = Duration::from_secs(90);
    while now.elapsed() < deletion_timeout {
        match kube_get_resources_by_selector::<StatefulSet>(
            client,
            namespace,
            &invalid_statefulset.statefulset_selector,
        )
        .await
        {
            Ok(result) => {
                if result.items.is_empty() {
                    break;
                }
            }
            Err(e) => {
                return Err(Box::new(EngineError::new_k8s_cannot_get_statefulset(
                    event_details.clone(),
                    namespace,
                    &invalid_statefulset.statefulset_selector,
                    e,
                )))
            }
        };
        sleep(Duration::from_secs(10)).await;
    }

    if now.elapsed() >= deletion_timeout {
        return Err(Box::new(EngineError::new_k8s_cannot_orphan_delete(
            event_details.clone(),
            invalid_statefulset.statefulset_selector.as_str(),
            CommandError::new_from_safe_message("Timeout waiting for statefulset deletion".to_string()),
        )));
    }

    // recreate statefulset thru Helm deployment to sync with new PVC size(s)
    kube_create_from_resource(client, namespace, current_statefulset.clone())
        .await
        .map_err(|e| {
            Box::new(EngineError::new_k8s_cannot_apply_from_resource(
                event_details.clone(),
                current_statefulset,
                e,
            ))
        })?;

    // rollout restart statefulset to enforce sync
    let statefulset_name = invalid_statefulset.statefulset_name.as_str();
    kube_rollout_restart_statefulset(client, namespace, statefulset_name)
        .await
        .map_err(|e| {
            Box::new(EngineError::new_k8s_cannot_rollout_restart_statefulset(
                event_details.clone(),
                statefulset_name,
                e,
            ))
        })?;

    Ok(())
}

pub fn get_service_statefulset_name_and_volumes(
    kube_client: &kube::Client,
    namespace: &str,
    selector: &str,
    event_details: &EventDetails,
) -> Result<(String, Option<Vec<PersistentVolumeClaim>>), Box<EngineError>> {
    match block_on(kube_get_resources_by_selector::<StatefulSet>(kube_client, namespace, selector)) {
        Err(e) => Err(Box::new(EngineError::new_k8s_cannot_get_statefulset(
            event_details.clone(),
            namespace,
            selector,
            e,
        ))),
        Ok(result) => {
            if let Some(statefulset) = result.items.first() {
                if let Some(name) = &statefulset.metadata.name {
                    if let Some(spec) = statefulset.clone().spec {
                        return Ok((name.to_string(), spec.volume_claim_templates));
                    }

                    return Ok((name.to_string(), None));
                }
            }

            Err(Box::new(EngineError::new_k8s_cannot_get_statefulset(
                event_details.clone(),
                namespace,
                selector,
                CommandError::new_from_safe_message("No statefulset returned".to_string()),
            )))
        }
    }
}
