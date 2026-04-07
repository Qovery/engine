use crate::io_models::models::ExternalSecret;
use crate::utilities::to_short_id;
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

/// One entry within an ESO ExternalSecret object (a single key/value pair to sync).
#[derive(Serialize, Debug, Clone)]
pub struct ExternalSecretEntry {
    /// The environment variable name / key in the resulting k8s Secret.
    pub env_var_key: String,
    /// The key/path in the remote secret manager.
    pub remote_key: String,
    /// Absolute mount path (e.g. `/tmp/db.txt`). Present only for file-mounted entries.
    pub mount_path: Option<String>,
    /// Relative mount path without leading `/` (e.g. `tmp/db.txt`). Used for k8s `subPath` and
    /// `items[].path`. Present only when `mount_path` is Some.
    pub mount_path_relative: Option<String>,
    /// Pre-computed k8s volume name. Present only when `mount_path` is Some.
    pub volume_name: Option<String>,
}

/// A group of external secret entries that share the same ClusterSecretStore, mapping to one ESO
/// ExternalSecret object and one generated k8s Secret.
#[derive(Serialize, Debug, Clone)]
pub struct ExternalSecretGroup {
    /// Name of the ESO ExternalSecret k8s object: `{prefix}-{service_short_id}-{access_id}`.
    pub external_secret_kube_name: String,
    /// Name of the ClusterSecretStore to target: `store-{full_access_uuid}`.
    pub store_name: String,
    /// Individual secret entries (each becomes one `.spec.data` item in the ESO object).
    pub entries: Vec<ExternalSecretEntry>,
}

/// Groups raw external secret entries by their `secret_manager_access_id`, producing one
/// [`ExternalSecretGroup`] per unique store (which maps to one ESO ExternalSecret object).
///
/// Both the ESO `ExternalSecret` object name and the generated k8s `Secret` name follow the
/// pattern `{prefix}-{service_short_id}-{access_id}`, where `prefix` is derived from the first
/// dash-separated component of `kube_name` (e.g. `"app"` from `"app-zf8e21411-my-service"`).
pub fn build_external_secret_groups(
    service_long_id: &Uuid,
    kube_name: &str,
    external_secrets: BTreeMap<String, ExternalSecret>,
) -> Vec<ExternalSecretGroup> {
    let prefix = kube_name.split('-').next().unwrap_or("app");
    let service_short_id = to_short_id(service_long_id);

    let mut by_access: BTreeMap<Uuid, Vec<(String, ExternalSecret)>> = BTreeMap::new();
    for (key, info) in external_secrets {
        by_access
            .entry(info.secret_manager_access_id)
            .or_default()
            .push((key, info));
    }

    by_access
        .into_iter()
        .map(|(access_id, entries)| {
            let mut mount_idx: usize = 0;
            let entries = entries
                .into_iter()
                .map(|(key, info)| {
                    let (mount_path_relative, volume_name) = match &info.mount_path {
                        Some(p) => {
                            let relative = p.trim_start_matches('/').to_string();
                            let vol_name = format!("ext-{access_id}-{mount_idx}");
                            mount_idx += 1;
                            (Some(relative), Some(vol_name))
                        }
                        None => (None, None),
                    };
                    let remote_key = info.external_secret_name;
                    ExternalSecretEntry {
                        env_var_key: key,
                        remote_key,
                        mount_path: info.mount_path,
                        mount_path_relative,
                        volume_name,
                    }
                })
                .collect();
            let object_name = format!("{prefix}-{service_short_id}-{access_id}");
            ExternalSecretGroup {
                external_secret_kube_name: object_name.clone(),
                store_name: format!("store-{access_id}"),
                entries,
            }
        })
        .collect()
}
