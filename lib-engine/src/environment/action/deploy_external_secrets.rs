use crate::cmd::helm::HelmError;
use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::models::external_secret::ExternalSecretGroup;
use crate::environment::report::logger::EnvProgressLogger;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::helm::{ChartInfo, HelmChartError, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::io_models::variable_utils::VariableInfo;
use crate::runtime::block_on;
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::Secret;
use kube::Api;
use kube::api::ListParams;
use kube::core::DynamicObject;
use kube::discovery::ApiResource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// Prefix shared with env variable replacement: `qovery.env.<KEY>`
pub const EXTERNAL_SECRET_PREFIX: &str = "qovery.env.";

const ESO_GROUP: &str = "external-secrets.io";
const ESO_VERSION: &str = "v1";
const ESO_KIND: &str = "ExternalSecret";
const ESO_SYNC_TIMEOUT: Duration = Duration::from_secs(30);
const ESO_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Returns the name of the companion Helm release that manages `ExternalSecret` objects
/// for a given service. The companion release (`{kube_name}-qovery-eso`) is deployed and
/// rolled back atomically together with the main service Helm release.
pub fn eso_companion_release_name(kube_name: &str) -> String {
    format!("{kube_name}-qovery-eso")
}

/// Tera context passed to `lib/common/charts/q-external-secret/templates/external_secret.j2.yaml`.
/// All UUID fields are stored as strings because Tera serialises them as-is.
#[derive(Serialize)]
pub struct EsoTeraContext {
    pub namespace: String,
    /// UUID of the service that owns these external secrets.
    pub service_id: String,
    /// Service type label value (e.g. `"helm"`, `"job"`, `"container"`).
    pub service_type: String,
    pub environment_id: String,
    pub project_id: String,
    /// Unix epoch in seconds — written as the `force-sync` annotation so ESO re-fetches
    /// on every deploy even when the spec hasn't changed.
    pub force_sync: String,
    pub external_secrets: Vec<ExternalSecretGroup>,
}

impl EsoTeraContext {
    pub fn new(
        service_id: Uuid,
        service_type: &str,
        namespace: &str,
        external_secrets: Vec<ExternalSecretGroup>,
        environment_id: Uuid,
        project_id: Uuid,
    ) -> Self {
        EsoTeraContext {
            namespace: namespace.to_string(),
            service_id: service_id.to_string(),
            service_type: service_type.to_string(),
            environment_id: environment_id.to_string(),
            project_id: project_id.to_string(),
            force_sync: force_sync_annotation(),
            external_secrets,
        }
    }
}

/// Returns true if at least one ExternalSecret CR labelled with the given service ID exists in
/// the namespace. Used before deployment to determine whether an orphaned companion release needs
/// to be cleaned up post-success
pub fn external_secrets_exist_for_service(kube_client: &kube::Client, namespace: &str, service_id: Uuid) -> bool {
    let api_resource = ApiResource {
        group: ESO_GROUP.to_string(),
        version: ESO_VERSION.to_string(),
        kind: ESO_KIND.to_string(),
        api_version: format!("{ESO_GROUP}/{ESO_VERSION}"),
        plural: "externalsecrets".to_string(),
    };
    let api: Api<DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);
    let label_selector = format!("qovery.com/service-id={service_id}");
    let list_params = ListParams::default().labels(&label_selector);
    match block_on(api.list(&list_params)) {
        Ok(list) => !list.items.is_empty(),
        Err(e) => {
            // If the CRD is not installed or the API is unavailable, treat as "not found".
            info!("Cannot check ExternalSecret existence for service {service_id}: {e}");
            false
        }
    }
}

pub fn deploy_helm_external_secrets(
    kube_name: &str,
    service_name: &str,
    service_id: Uuid,
    service_type: &str,
    namespace: &str,
    external_secrets: &[ExternalSecretGroup],
    environment_id: Uuid,
    project_id: Uuid,
    workspace_directory: &Path,
    lib_root_directory: &str,
    target: &DeploymentTarget,
    event_details: EventDetails,
    logger: &EnvProgressLogger,
) -> Result<HashMap<String, VariableInfo>, Box<EngineError>> {
    let companion_release = eso_companion_release_name(kube_name);

    logger.info(format!(
        "🔐 Deploying ESO companion release '{companion_release}' ({} external secret group(s))",
        external_secrets.len()
    ));

    let tera_context = EsoTeraContext::new(
        service_id,
        service_type,
        namespace,
        external_secrets.to_vec(),
        environment_id,
        project_id,
    );
    let chart_workspace = workspace_directory.join("qovery-eso-chart");
    let chart_info = ChartInfo {
        name: companion_release,
        path: chart_workspace.to_string_lossy().to_string(),
        namespace: HelmChartNamespaces::Custom(namespace.to_string()),
        timeout_in_seconds: 60,
        ..Default::default()
    };

    let helm_deployment = HelmDeployment::new(
        event_details.clone(),
        tera::Context::from_serialize(tera_context).unwrap_or_default(),
        PathBuf::from(helm_chart_eso_dir(lib_root_directory)),
        None,
        chart_info,
    );

    let deployment_start_time_utc = Utc::now();
    helm_deployment.on_create(target)?;

    let result = wait_and_fetch_eso_values(
        service_name,
        namespace,
        target.kube.client(),
        external_secrets,
        event_details,
        logger,
        deployment_start_time_utc,
    );

    // If any issue happens, we need to rollback to previous version
    if result.is_err() {
        if let Err(e) = rollback_helm_external_secrets(kube_name, target) {
            logger.warning(format!("Failed to rollback ESO companion release: {e}"));
        } else {
            logger.info("External secrets have been rollback to previous version".to_string());
        }
    }

    result
}

pub fn rollback_helm_external_secrets(kube_name: &str, target: &DeploymentTarget) -> Result<(), HelmError> {
    let companion_release = eso_companion_release_name(kube_name);
    let companion_chart = ChartInfo::new_from_release_name(&companion_release, target.environment.namespace());
    target.helm.rollback(&companion_chart, &[])
}

/// Rolls back the ESO companion release if external secrets are configured, logging a warning on
/// unexpected failures. `ReleaseDoesNotExist` is silently ignored (ESO was never deployed).
pub fn rollback_external_secrets_if_needed(
    kube_name: &str,
    external_secrets: &[ExternalSecretGroup],
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
) {
    if !external_secrets.is_empty()
        && let Err(eso_err) = rollback_helm_external_secrets(kube_name, target)
    {
        match eso_err {
            HelmError::ReleaseDoesNotExist(_) => {}
            _ => logger.warning(format!("Failed to rollback external secret(s): {eso_err}")),
        }
    }
}

pub fn helm_chart_eso_dir(lib_root_directory: &str) -> String {
    format!("{}/common/charts/q-external-secret", lib_root_directory)
}

/// Serde helpers for reading ESO status conditions from a `DynamicObject`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EsoCondition {
    #[serde(rename = "type")]
    condition_type: String,
    status: String,
    message: Option<String>,
    reason: Option<String>,
    last_transition_time: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct EsoStatus {
    conditions: Option<Vec<EsoCondition>>,
    refresh_time: Option<String>,
}

fn eso_api_resource() -> ApiResource {
    ApiResource {
        group: ESO_GROUP.to_string(),
        version: ESO_VERSION.to_string(),
        api_version: format!("{ESO_GROUP}/{ESO_VERSION}"),
        kind: ESO_KIND.to_string(),
        plural: "externalsecrets".to_string(),
    }
}

fn force_sync_annotation() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn wait_and_fetch_eso_values(
    service_name: &str,
    namespace: &str,
    kube_client: kube::Client,
    external_secrets: &[ExternalSecretGroup],
    event_details: EventDetails,
    logger: &EnvProgressLogger,
    deployment_start_time_utc: DateTime<Utc>,
) -> Result<HashMap<String, VariableInfo>, Box<EngineError>> {
    if external_secrets.is_empty() {
        return Ok(HashMap::new());
    }

    let to_error = |msg: String| -> Box<EngineError> {
        Box::new(EngineError::new_helm_chart_error(
            event_details.clone(),
            HelmChartError::CreateTemplateError {
                chart_name: service_name.to_string(),
                msg,
            },
        ))
    };

    let eso_api: Api<DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &eso_api_resource());

    // ── Wait for ESO Ready condition, then read k8s Secret values ─────────
    let secret_api: Api<Secret> = Api::namespaced(kube_client.clone(), namespace);
    let mut result: HashMap<String, VariableInfo> = HashMap::new();

    for group in external_secrets {
        let eso_name = &group.external_secret_kube_name;
        logger.info(format!("⏳ Waiting for external secret '{eso_name}' to be synced by ESO..."));

        let deadline = std::time::Instant::now() + ESO_SYNC_TIMEOUT;

        // Poll the ESO ExternalSecret status until Ready=True or Ready=False.
        loop {
            let eso_obj = block_on(eso_api.get(eso_name))
                .map_err(|e| to_error(format!("Cannot read ExternalSecret '{eso_name}' status: {e}")))?;

            let status: EsoStatus = eso_obj
                .data
                .get("status")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            // If the refresh time is set & has been updated since last deployment, it means the external secret has been synced
            if let Some(refresh_time) = status.refresh_time
                && let Ok(refresh_date_time) = refresh_time.parse::<DateTime<Utc>>()
                && refresh_date_time > deployment_start_time_utc
            {
                break;
            }

            // If the Ready condition is set & has been updated since last deployment, it means the external secret has been synced
            if let Some(ready) = status
                .conditions
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|c| {
                    c.last_transition_time
                        .as_ref()
                        .and_then(|t| t.parse::<DateTime<Utc>>().ok())
                        .is_some_and(|t| t > deployment_start_time_utc)
                })
                .find(|c| c.condition_type == "Ready")
            {
                if ready.status == "True" {
                    break; // synced — proceed to read the secret
                }
                // Ready=False: surface the ESO message immediately
                let reason = ready.reason.as_deref().unwrap_or("unknown");
                let message = ready.message.as_deref().unwrap_or("no message provided by ESO");
                return Err(to_error(format!(
                    "External secret '{eso_name}' failed to sync (reason: {reason}): {message}."
                )));
            }

            if std::time::Instant::now() >= deadline {
                return Err(to_error(format!(
                    "Timeout waiting for external secret '{eso_name}' to sync. Ensure your external secrets are valid."
                )));
            }

            thread::sleep(ESO_POLL_INTERVAL);
        }

        // Read values from the synced k8s Secret.
        let secret = block_on(secret_api.get(eso_name))
            .map_err(|e| to_error(format!("Cannot read synced k8s Secret '{eso_name}': {e}")))?;

        let data = secret.data.unwrap_or_default();
        for entry in &group.entries {
            match data.get(&entry.env_var_key) {
                Some(bytes) => {
                    // INFO (qov-1569) If upstream secrets are non-utf8, clients will need to encode it to base64 themselves
                    let value = String::from_utf8(bytes.0.clone()).map_err(|e| {
                        to_error(format!(
                            "External secret key '{}' in secret '{eso_name}' is not valid UTF-8: {e}",
                            entry.env_var_key
                        ))
                    })?;
                    result.insert(
                        entry.env_var_key.clone(),
                        VariableInfo {
                            value,
                            is_secret: false,
                        },
                    );
                }
                None => {
                    return Err(to_error(format!(
                        "External secret key '{}' was not found in synced secret '{eso_name}'. \
                         Check your remote secret manager key path.",
                        entry.env_var_key
                    )));
                }
            }
        }
    }

    Ok(result)
}
