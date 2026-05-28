use crate::cmd::command::CommandKiller;
use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::models::external_secret::ExternalSecretGroup;
use crate::environment::report::logger::EnvProgressLogger;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::io_models::variable_utils::VariableInfo;
use crate::runtime::block_on;
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::Secret;
use kube::Api;
use kube::api::{DeleteParams, ListParams};
use kube::core::DynamicObject;
use kube::discovery::ApiResource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

// Prefix shared with env variable replacement: `qovery.env.<KEY>`
pub const EXTERNAL_SECRET_PREFIX: &str = "qovery.env.";

const ESO_GROUP: &str = "external-secrets.io";
const ESO_VERSION: &str = "v1";
const ESO_KIND: &str = "ExternalSecret";
const ESO_SYNC_TIMEOUT: Duration = Duration::from_secs(15);
const ESO_POLL_INTERVAL: Duration = Duration::from_secs(3);
const DATA_HASH_ANNOTATION: &str = "reconcile.external-secrets.io/data-hash";

/// Returns the name of the companion Helm release that manages `ExternalSecret` objects
/// for a given service. The companion release (`{kube_name}-qovery-eso`) is deployed and
/// rolled back atomically together with the main service Helm release.
pub fn eso_companion_release_name(kube_name: &str) -> String {
    let suffix = "-qovery-eso";
    let max_base_len = 53 - suffix.len();
    let truncated = &kube_name[..kube_name.len().min(max_base_len)];
    format!("{truncated}{suffix}")
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

pub struct DeployExternalSecretsResult {
    pub external_secrets_groups_with_values: Vec<ExternalSecretGroupValuesWithTargetSecretName>,
}

pub struct ExternalSecretGroupValuesWithTargetSecretName {
    /// Values of the Secret fetched by the ExternalSecret
    pub group_values: HashMap<String, VariableInfo>,
    /// Kubernetes Secret name used by the service
    /// The rule is:
    /// * if the new secret generated contains the exact same values, fallback to previous secret used (to have an atomic service rollback)
    /// * otherwise service will use the new secret generated
    pub target_secret_name: String,
    /// The External Secret name used by the service
    pub external_secret_name: String,
}

struct ServiceSecret {
    secret_name: String,
    store_name: String,
    external_secret_hash: String,
}

pub fn deploy_helm_external_secrets(
    kube_name: &str,
    service_id: Uuid,
    service_type: &str,
    namespace: &str,
    external_secret_groups: &[ExternalSecretGroup],
    environment_id: Uuid,
    project_id: Uuid,
    workspace_directory: &Path,
    lib_root_directory: &str,
    target: &DeploymentTarget,
    event_details: EventDetails,
    logger: &EnvProgressLogger,
) -> Result<DeployExternalSecretsResult, Box<EngineError>> {
    // Fetch current secrets
    let current_service_secrets =
        fetch_current_service_kubernetes_secrets(target.kube.client(), namespace, &service_id, external_secret_groups);

    // Apply new external secrets
    let deployment_start_time_utc = install_external_secrets(
        kube_name,
        service_id,
        service_type,
        namespace,
        external_secret_groups,
        environment_id,
        project_id,
        workspace_directory,
        lib_root_directory,
        target,
        event_details.clone(),
        logger,
    )?;

    let result = wait_and_fetch_eso_values(
        namespace,
        target.kube.client(),
        external_secret_groups,
        &current_service_secrets,
        event_details,
        logger,
        deployment_start_time_utc,
    )?;

    Ok(DeployExternalSecretsResult {
        external_secrets_groups_with_values: result,
    })
}

fn install_external_secrets(
    kube_name: &str,
    service_id: Uuid,
    service_type: &str,
    namespace: &str,
    external_secret_groups: &[ExternalSecretGroup],
    environment_id: Uuid,
    project_id: Uuid,
    workspace_directory: &Path,
    lib_root_directory: &str,
    target: &DeploymentTarget,
    event_details: EventDetails,
    logger: &EnvProgressLogger,
) -> Result<DateTime<Utc>, Box<EngineError>> {
    let companion_release = eso_companion_release_name(kube_name);

    logger.info(format!(
        "🔐 Deploying ESO companion release '{companion_release}' ({} external secret group(s))",
        external_secret_groups.len()
    ));

    let tera_context = EsoTeraContext::new(
        service_id,
        service_type,
        namespace,
        external_secret_groups.to_vec(),
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
        event_details,
        tera::Context::from_serialize(tera_context).unwrap_or_default(),
        PathBuf::from(helm_chart_eso_dir(lib_root_directory)),
        None,
        chart_info,
    );

    let deployment_start_time_utc = Utc::now();
    helm_deployment.on_create(target)?;
    Ok(deployment_start_time_utc)
}

fn fetch_current_service_kubernetes_secrets(
    kube_client: kube::Client,
    namespace: &str,
    service_id: &Uuid,
    external_secret_groups: &[ExternalSecretGroup],
) -> Vec<ServiceSecret> {
    let secret_api: Api<Secret> = Api::namespaced(kube_client, namespace);
    let mut result = Vec::with_capacity(external_secret_groups.len());

    for group in external_secret_groups {
        let label_selector = format!("qovery.com/service-id={service_id},qovery.com/store-name={}", group.store_name);
        let list_params = ListParams::default().labels(&label_selector);

        let secrets = match block_on(secret_api.list(&list_params)) {
            Ok(list) => list.items,
            Err(e) => {
                info!(
                    "Cannot fetch k8s Secret for service {service_id} / store '{}': {e}",
                    group.store_name
                );
                continue;
            }
        };

        for secret in secrets {
            let secret_name = match secret.metadata.name {
                Some(name) => name,
                None => continue,
            };
            let external_secret_hash = match secret
                .metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get(DATA_HASH_ANNOTATION))
            {
                Some(hash) => hash.clone(),
                None => continue,
            };
            result.push(ServiceSecret {
                secret_name,
                store_name: group.store_name.clone(),
                external_secret_hash,
            });
        }
    }

    result
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

/// Waits for External Secrets to be ready and fetches their values
/// Targets either the new secret or the previous secret based on annotation `reconcile.external-secrets.io/data-hash`
/// Returns the mapping between the external secret and the target secret name with decoded secret values
fn wait_and_fetch_eso_values(
    namespace: &str,
    kube_client: kube::Client,
    external_secret_groups: &[ExternalSecretGroup],
    current_service_secrets: &[ServiceSecret],
    event_details: EventDetails,
    logger: &EnvProgressLogger,
    deployment_start_time_utc: DateTime<Utc>,
) -> Result<Vec<ExternalSecretGroupValuesWithTargetSecretName>, Box<EngineError>> {
    if external_secret_groups.is_empty() {
        return Ok(vec![]);
    }

    let to_error = |safe_message: String| -> Box<EngineError> {
        Box::new(EngineError::new_external_secrets_failed_to_resolve(
            event_details.clone(),
            safe_message,
        ))
    };

    let eso_api: Api<DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &eso_api_resource());

    // ── Wait for ESO Ready condition, then read k8s Secret values ─────────
    let secret_api: Api<Secret> = Api::namespaced(kube_client.clone(), namespace);
    let mut result: Vec<ExternalSecretGroupValuesWithTargetSecretName> =
        Vec::with_capacity(external_secret_groups.len());

    logger.info("⏳ Resolving your external secrets".to_string());

    for external_secret_group in external_secret_groups.iter() {
        let external_secret_kube_name = &external_secret_group.external_secret_kube_name;
        let deadline = std::time::Instant::now() + ESO_SYNC_TIMEOUT;

        // Poll the ESO ExternalSecret status until Ready=True or Ready=False.
        loop {
            let eso_obj = block_on(eso_api.get(external_secret_kube_name)).map_err(|e| {
                to_error(format!("Cannot read ExternalSecret '{external_secret_kube_name}' status: {e}"))
            })?;

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

            // The secret is synced only if all conditions are met:
            // - the Ready condition is set
            // - it has been updated since last deployment
            // - the status is "True"
            // Otherwise we fallback to the timeout block because deploying more than 1 time a failing ExternalSecret won't update the last_transition_time
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
                && ready.status == "True"
            {
                break; // synced — proceed to read the secret
            }

            // If the timeout is reached, it means an error happened (i.e secret doesn't exist anymore)
            if std::time::Instant::now() >= deadline {
                let error_messages = status
                    .conditions
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|c| {
                        format!(
                            "{}: '{}'",
                            c.reason.as_deref().unwrap_or("Unknown reason"),
                            c.message.as_deref().unwrap_or("no message provided")
                        )
                    })
                    .collect::<Vec<String>>();
                let detail = if error_messages.is_empty() {
                    "No error message found".to_string()
                } else {
                    error_messages.join(", ")
                };
                let user_message = format!(
                    "❗An issue happened when attempting to resolve your external secrets. You need to ensure the external secrets referenced in your environment / service still exist ({detail})"
                );
                return Err(to_error(user_message));
            }

            thread::sleep(ESO_POLL_INTERVAL);
        }

        // Fetch new kube Secret hash
        let new_secret_name = &external_secret_group.secret_name;
        let new_secret_hash = block_on(secret_api.get(new_secret_name.as_str()))
            .ok()
            .and_then(|s| s.metadata.annotations)
            .and_then(|a| a.get(DATA_HASH_ANNOTATION).cloned());

        // If the new secret hash matches the previous one, read values from the previous secret
        // to guarantee atomic rollback: the service keeps pointing to an already-validated secret.
        let target_secret_name = current_service_secrets
            .iter()
            .find(|s| s.store_name == external_secret_group.store_name)
            .filter(|s| new_secret_hash.as_deref() == Some(s.external_secret_hash.as_str()))
            .map(|s| s.secret_name.as_str())
            .unwrap_or(new_secret_name.as_str());

        // Read values from the target secret (either the old one or the new one)
        let secret = block_on(secret_api.get(target_secret_name))
            .map_err(|e| to_error(format!("Cannot read synced k8s Secret '{target_secret_name}': {e}")))?;

        let data = secret.data.unwrap_or_default();
        let mut secret_values: HashMap<String, VariableInfo> = HashMap::new();
        for entry in &external_secret_group.entries {
            match data.get(&entry.env_var_key) {
                Some(bytes) => {
                    // INFO (qov-1569) If upstream secrets are non-utf8, clients will need to encode it to base64 themselves
                    let value = String::from_utf8(bytes.0.clone()).map_err(|e| {
                        to_error(format!(
                            "External secret key '{}' in secret '{target_secret_name}' is not valid UTF-8: {e}",
                            entry.env_var_key
                        ))
                    })?;
                    secret_values.insert(
                        entry.env_var_key.clone(),
                        VariableInfo {
                            value,
                            is_secret: false,
                        },
                    );
                }
                None => {
                    return Err(to_error(format!(
                        "External secret key '{}' was not found in synced secret '{target_secret_name}'. \
                         Check your remote secret manager key path.",
                        entry.env_var_key
                    )));
                }
            }
        }
        result.push(ExternalSecretGroupValuesWithTargetSecretName {
            group_values: secret_values,
            target_secret_name: target_secret_name.to_string(),
            external_secret_name: external_secret_group.external_secret_kube_name.clone(),
        })
    }

    Ok(result)
}

/// Uninstall External Secrets helm release for the given service and delete all orphaned k8s
/// Secrets that ESO left behind due to `creationPolicy: Orphan`.
/// If helm release doesn't exist, it will be silently ignored.
pub fn uninstall_service_external_secret(
    service_kube_name: &str,
    service_id: &Uuid,
    deployment_target: &DeploymentTarget,
) {
    let namespace = deployment_target.environment.namespace();
    let external_secrets_helm_release_name = eso_companion_release_name(service_kube_name);
    let external_secrets_helm_chart = ChartInfo::new_from_release_name(&external_secrets_helm_release_name, namespace);
    if let Err(e) = deployment_target.helm.uninstall(
        &external_secrets_helm_chart,
        &[],
        &CommandKiller::never(),
        &mut |_| {},
        &mut |_| {},
    ) {
        warn!("Failed to uninstall external secrets for release '{external_secrets_helm_release_name}': {e}");
    }
    delete_all_eso_secrets_for_service(deployment_target.kube.client(), namespace, service_id);
}

/// Deletes all Kubernetes Secrets that were created by ESO for a given service.
/// Used when the service itself is being deleted — no secrets should remain.
pub fn delete_all_eso_secrets_for_service(kube_client: kube::Client, namespace: &str, service_id: &Uuid) {
    clean_unused_secrets_generated_by_eso(kube_client, namespace, service_id, vec![]);
}

/// Deletes Kubernetes Secrets that were previously created by ESO for a given service but are no
/// longer in use (i.e. not present in `service_secret_names`).
///
/// A secret is considered an ESO-managed secret for this service when it has both:
/// - annotation `reconcile.external-secrets.io/data-hash` (any value)
/// - annotation `qovery.com/service-id` equal to the service UUID
pub fn clean_unused_secrets_generated_by_eso(
    kube_client: kube::Client,
    namespace: &str,
    service_id: &Uuid,
    service_secret_names: Vec<String>,
) {
    // Get all secrets used by the service
    // * those created by Qovery as usual
    // * those created by ESO (previous & new)
    let secret_api: Api<Secret> = Api::namespaced(kube_client, namespace);
    let label_selector = format!("qovery.com/service-id={service_id}");
    let secrets = match block_on(secret_api.list(&ListParams::default().labels(&label_selector))) {
        Ok(list) => list,
        Err(err) => {
            error!("Failed to clean secrets in namespace {namespace} for service {service_id}: {err}");
            return;
        }
    };

    // Iterate on service's secrets and remove the unused
    for secret in secrets.items {
        let annotations = match secret.metadata.annotations.as_ref() {
            Some(a) => a,
            None => continue,
        };

        let has_eso_annotation = annotations.contains_key("reconcile.external-secrets.io/data-hash");

        if !has_eso_annotation {
            continue;
        }

        let secret_name = match secret.metadata.name.as_deref() {
            Some(n) => n,
            None => continue,
        };

        if !service_secret_names.iter().any(|s| s == secret_name)
            && let Err(err) = block_on(secret_api.delete(secret_name, &DeleteParams::background()))
        {
            error!("Failed to delete unused ESO secret {secret_name} for service {service_id}: {err}");
        }
    }
}
