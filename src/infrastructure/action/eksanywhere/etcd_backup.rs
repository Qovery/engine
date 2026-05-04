use crate::errors::CommandError;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::eksanywhere::{EksAnywhere, EksAnywhereClusterBackupParameters};
use crate::runtime::block_on;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{Pod, Secret};
use kube::ResourceExt;
use kube::api::{Api, ApiResource, DeleteParams, ListParams, LogParams, PostParams};
use kube::core::{DynamicObject, GroupVersionKind};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use tera::{Context as TeraContext, Tera};
use tracing::{debug, info};
use url::Url;
use uuid::Uuid;

const EKSA_SYSTEM_NAMESPACE: &str = "eksa-system";
const DEFAULT_ETCDCTL_IMAGE: &str = "quay.io/coreos/etcd:v3.5.12";
const DEFAULT_UPLOAD_IMAGE: &str = "curlimages/curl:8.8.0";
const JOB_TTL_SECONDS_AFTER_FINISHED: i32 = 3600;
const JOB_POLL_INTERVAL_SECONDS: u64 = 5;
const CAPI_MACHINE_API_VERSIONS: [&str; 4] = ["v1beta1", "v1beta2", "v1alpha4", "v1alpha3"];
const ETCD_BACKUP_JOB_TEMPLATE_RELATIVE_PATH: &str = "etcd-backup/job.tpl.yaml";
const CLIENT_CERT_KEY_CANDIDATES: [&str; 4] = ["tls.crt", "cert.crt", "client.crt", "apiserver-etcd-client.crt"];
const CLIENT_PRIVATE_KEY_CANDIDATES: [&str; 4] = ["tls.key", "key.pem", "client.key", "apiserver-etcd-client.key"];
const CA_CERT_KEY_CANDIDATES: [&str; 4] = ["ca.crt", "ca.pem", "etcd-ca.crt", "tls-ca.crt"];
const CA_CERT_FALLBACK_KEY_CANDIDATES: [&str; 1] = ["tls.crt"];

pub(super) fn run_eks_anywhere_cluster_backup(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    cluster_config_path: &Path,
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    let Some(cluster_backup) = cluster
        .options
        .infrastructure_charts_parameters
        .eks_anywhere_parameters
        .as_ref()
        .and_then(|params| params.cluster_backup.as_ref())
    else {
        return Ok(());
    };

    if !cluster_backup.enabled {
        logger.info("Skipping etcd backup: disabled in EKS Anywhere parameters.");
        return Ok(());
    }

    log_section_title(logger, "🗄️", "EKS Anywhere etcd backup");

    let cluster_name = match cluster_name_from_config(cluster_config_path) {
        Ok(Some(name)) => name,
        Ok(None) => cluster.name().to_string(),
        Err(err) => {
            logger.warn(format!(
                "Unable to infer cluster name from config file, fallback to Kubernetes name `{}`: {}",
                cluster.name(),
                err.message_safe()
            ));
            cluster.name().to_string()
        }
    };
    info!("Using cluster name `{cluster_name}` for etcd backup discovery.");

    let kube_client = infra_ctx.mk_kube_client().map_err(|e| {
        CommandError::new_from_safe_message(format!("Cannot create Kubernetes client for etcd backup: {e}"))
    })?;
    let kube = kube_client.client();

    let etcd_endpoint = discover_etcd_endpoint(&kube, cluster_name.as_str())?;
    info!("Discovered etcd endpoint `{etcd_endpoint}`.");

    let certs_secret_names =
        resolve_etcd_certs_secret_names(&kube, cluster_name.as_str(), cluster_backup.certs_secret_name.as_deref())?;
    info!(
        "Using etcd cert secrets client=`{}`, ca=`{}`.",
        certs_secret_names.client_secret_name, certs_secret_names.ca_secret_name
    );
    debug!(
        "Using etcd cert keys client_cert=`{}`, client_key=`{}`, ca_cert=`{}`.",
        certs_secret_names.client_cert_key, certs_secret_names.client_private_key_key, certs_secret_names.ca_cert_key
    );

    let presigned_put_url =
        validate_presigned_put_url(cluster_backup.s3.etcd_presigned_put_url.as_str(), "etcd backup")?;
    info!(
        "Snapshot destination (pre-signed PUT URL): `{}`.",
        redact_url_for_logs(&presigned_put_url)
    );

    let job_name = format!("qovery-etcd-backup-{}-{}", cluster.short_id(), short_random_suffix());
    let backup_result = execute_cluster_backup_job(
        &kube,
        &job_name,
        certs_secret_names.client_secret_name.as_str(),
        certs_secret_names.ca_secret_name.as_str(),
        certs_secret_names.client_cert_key.as_str(),
        certs_secret_names.client_private_key_key.as_str(),
        certs_secret_names.ca_cert_key.as_str(),
        &etcd_endpoint,
        &presigned_put_url,
        cluster_backup,
        cluster.template_directory.as_path(),
        logger,
    );

    match backup_result {
        Ok(()) => {
            if let Err(cleanup_error) = cleanup_backup_resources(&kube, &job_name) {
                logger.warn(format!(
                    "Unable to cleanup temporary backup resources (`{job_name}`): {}",
                    cleanup_error.message_safe()
                ));
            }
            log_section_title(logger, "✅", "etcd backup completed");
            Ok(())
        }
        Err(backup_error) => {
            if cluster_backup.keep_failed_job_on_failure {
                logger.warn(format!(
                    "Keeping failed etcd backup job `{job_name}` in namespace `{EKSA_SYSTEM_NAMESPACE}` for investigation."
                ));
            } else if let Err(cleanup_error) = cleanup_backup_resources(&kube, &job_name) {
                logger.warn(format!(
                    "Unable to cleanup temporary backup resources (`{job_name}`): {}",
                    cleanup_error.message_safe()
                ));
            }

            Err(backup_error)
        }
    }
}

fn execute_cluster_backup_job(
    kube: &kube::Client,
    job_name: &str,
    client_certs_secret_name: &str,
    ca_certs_secret_name: &str,
    client_cert_key: &str,
    client_private_key_key: &str,
    ca_cert_key: &str,
    etcd_endpoint: &str,
    presigned_put_url: &str,
    cluster_backup: &EksAnywhereClusterBackupParameters,
    template_directory: &Path,
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    debug!(
        "Starting etcd backup job flow: job_name=`{}`, namespace=`{}`, timeout_seconds={}, keep_failed_job_on_failure={}, endpoint=`{}`, client_secret=`{}`, ca_secret=`{}`, client_cert_key=`{}`, client_key_key=`{}`, ca_cert_key=`{}`",
        job_name,
        EKSA_SYSTEM_NAMESPACE,
        cluster_backup.timeout_seconds,
        cluster_backup.keep_failed_job_on_failure,
        etcd_endpoint,
        client_certs_secret_name,
        ca_certs_secret_name,
        client_cert_key,
        client_private_key_key,
        ca_cert_key
    );

    create_cluster_backup_job(
        kube,
        job_name,
        client_certs_secret_name,
        ca_certs_secret_name,
        client_cert_key,
        client_private_key_key,
        ca_cert_key,
        etcd_endpoint,
        presigned_put_url,
        cluster_backup,
        template_directory,
    )?;

    logger.info(format!("Waiting for backup job `{job_name}` completion."));
    if let Err(wait_error) = wait_for_job_completion(kube, job_name, cluster_backup.timeout_seconds) {
        let logs = collect_job_logs(kube, job_name);
        let full_details = if logs.trim().is_empty() {
            wait_error.message_safe()
        } else {
            format!("{}\n\nJob logs:\n{}", wait_error.message_safe(), logs)
        };
        return Err(CommandError::new(
            format!("EKS Anywhere etcd backup job `{job_name}` failed"),
            Some(full_details),
            None,
        ));
    }

    logger.info(format!("Backup job `{job_name}` succeeded."));
    Ok(())
}

fn create_cluster_backup_job(
    kube: &kube::Client,
    job_name: &str,
    client_certs_secret_name: &str,
    ca_certs_secret_name: &str,
    client_cert_key: &str,
    client_private_key_key: &str,
    ca_cert_key: &str,
    etcd_endpoint: &str,
    presigned_put_url: &str,
    cluster_backup: &EksAnywhereClusterBackupParameters,
    template_directory: &Path,
) -> Result<(), CommandError> {
    let job = build_cluster_backup_job(
        job_name,
        client_certs_secret_name,
        ca_certs_secret_name,
        client_cert_key,
        client_private_key_key,
        ca_cert_key,
        etcd_endpoint,
        presigned_put_url,
        cluster_backup,
        template_directory,
    )?;

    let api: Api<Job> = Api::namespaced(kube.clone(), EKSA_SYSTEM_NAMESPACE);
    let created_job = block_on(api.create(&PostParams::default(), &job)).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot create etcd backup job `{}/{}`: {e}",
            EKSA_SYSTEM_NAMESPACE, job_name
        ))
    })?;
    debug!(
        "Created etcd backup job: namespace=`{}`, name=`{}`, uid={:?}, resource_version={:?}",
        EKSA_SYSTEM_NAMESPACE,
        created_job.name_any(),
        created_job.uid(),
        created_job.resource_version()
    );

    Ok(())
}

fn build_cluster_backup_job(
    job_name: &str,
    client_certs_secret_name: &str,
    ca_certs_secret_name: &str,
    client_cert_key: &str,
    client_private_key_key: &str,
    ca_cert_key: &str,
    etcd_endpoint: &str,
    presigned_put_url: &str,
    cluster_backup: &EksAnywhereClusterBackupParameters,
    template_directory: &Path,
) -> Result<Job, CommandError> {
    let timeout_seconds = i64::try_from(cluster_backup.timeout_seconds.max(1)).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Invalid etcd backup timeout `{}`: {e}",
            cluster_backup.timeout_seconds
        ))
    })?;

    let rendered_manifest = render_cluster_backup_job_manifest(
        template_directory,
        job_name,
        client_certs_secret_name,
        ca_certs_secret_name,
        client_cert_key,
        client_private_key_key,
        ca_cert_key,
        etcd_endpoint,
        presigned_put_url,
        timeout_seconds,
        cluster_backup,
    )?;

    serde_yaml::from_str::<Job>(&rendered_manifest).map_err(|e| {
        CommandError::new(
            "Cannot deserialize rendered etcd backup job manifest".to_string(),
            Some(format!(
                "{e}. Rendered manifest omitted because it may contain sensitive values (for example pre-signed URLs)."
            )),
            None,
        )
    })
}

fn render_cluster_backup_job_manifest(
    template_directory: &Path,
    job_name: &str,
    client_certs_secret_name: &str,
    ca_certs_secret_name: &str,
    client_cert_key: &str,
    client_private_key_key: &str,
    ca_cert_key: &str,
    etcd_endpoint: &str,
    presigned_put_url: &str,
    timeout_seconds: i64,
    cluster_backup: &EksAnywhereClusterBackupParameters,
) -> Result<String, CommandError> {
    let template_path = cluster_backup_job_template_path(template_directory);
    let template_content = fs::read_to_string(&template_path).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot read etcd backup job template `{}`: {e}",
            template_path.display()
        ))
    })?;

    let mut tera = Tera::default();
    tera.add_raw_template("cluster_backup_job", &template_content)
        .map_err(|e| CommandError::new_from_safe_message(format!("Cannot register etcd backup job template: {e}")))?;

    let mut context = TeraContext::new();
    context.insert("job_name", job_name);
    context.insert("namespace", EKSA_SYSTEM_NAMESPACE);
    context.insert("ttl_seconds_after_finished", &JOB_TTL_SECONDS_AFTER_FINISHED);
    context.insert("timeout_seconds", &timeout_seconds);
    context.insert(
        "etcdctl_image",
        &cluster_backup
            .etcdctl_image
            .as_ref()
            .cloned()
            .unwrap_or_else(|| DEFAULT_ETCDCTL_IMAGE.to_string()),
    );
    context.insert(
        "upload_image",
        &cluster_backup
            .upload_image
            .as_ref()
            .cloned()
            .unwrap_or_else(|| DEFAULT_UPLOAD_IMAGE.to_string()),
    );
    context.insert("client_certs_secret_name", client_certs_secret_name);
    context.insert("ca_certs_secret_name", ca_certs_secret_name);
    context.insert("client_cert_key", client_cert_key);
    context.insert("client_private_key_key", client_private_key_key);
    context.insert("ca_cert_key", ca_cert_key);
    context.insert("etcd_endpoint", etcd_endpoint);
    context.insert("presigned_put_url", presigned_put_url);

    tera.render("cluster_backup_job", &context)
        .map_err(|e| CommandError::new_from_safe_message(format!("Cannot render etcd backup job template: {e}")))
}

fn cluster_backup_job_template_path(template_directory: &Path) -> PathBuf {
    template_directory.join(ETCD_BACKUP_JOB_TEMPLATE_RELATIVE_PATH)
}

fn wait_for_job_completion(kube: &kube::Client, job_name: &str, timeout_seconds: u64) -> Result<(), CommandError> {
    let api: Api<Job> = Api::namespaced(kube.clone(), EKSA_SYSTEM_NAMESPACE);
    let pod_api: Api<Pod> = Api::namespaced(kube.clone(), EKSA_SYSTEM_NAMESPACE);
    let timeout = Duration::from_secs(timeout_seconds.max(1));
    let start = Instant::now();

    loop {
        let job = block_on(api.get(job_name)).map_err(|e| {
            CommandError::new_from_safe_message(format!(
                "Cannot fetch etcd backup job `{}/{}`: {e}",
                EKSA_SYSTEM_NAMESPACE, job_name
            ))
        })?;
        debug_log_job_status(job_name, &job, start.elapsed(), timeout);
        debug_log_job_pods(&pod_api, job_name);

        if job_has_succeeded(&job) {
            return Ok(());
        }

        if job_has_failed(&job) {
            return Err(CommandError::new_from_safe_message(format!(
                "Backup job `{job_name}` reported failed status"
            )));
        }

        if start.elapsed() >= timeout {
            return Err(CommandError::new_from_safe_message(format!(
                "Timeout while waiting for backup job `{job_name}` completion after {}s",
                timeout.as_secs()
            )));
        }

        thread::sleep(Duration::from_secs(JOB_POLL_INTERVAL_SECONDS));
    }
}

fn job_has_succeeded(job: &Job) -> bool {
    let Some(status) = job.status.as_ref() else {
        return false;
    };

    if status.succeeded.unwrap_or_default() > 0 {
        return true;
    }

    status.conditions.as_ref().is_some_and(|conditions| {
        conditions
            .iter()
            .any(|condition| condition.type_ == "Complete" && condition.status == "True")
    })
}

fn job_has_failed(job: &Job) -> bool {
    let Some(status) = job.status.as_ref() else {
        return false;
    };

    if status.failed.unwrap_or_default() > 0 {
        return true;
    }

    status.conditions.as_ref().is_some_and(|conditions| {
        conditions
            .iter()
            .any(|condition| condition.type_ == "Failed" && condition.status == "True")
    })
}

fn collect_job_logs(kube: &kube::Client, job_name: &str) -> String {
    let pod_api: Api<Pod> = Api::namespaced(kube.clone(), EKSA_SYSTEM_NAMESPACE);
    let selector = format!("job-name={job_name}");
    let pods = match block_on(pod_api.list(&ListParams::default().labels(selector.as_str()))) {
        Ok(pods) => pods,
        Err(e) => return format!("Unable to list backup job pods: {e}"),
    };
    debug!(
        "Collecting backup job logs for `{}`: found {} pod(s).",
        job_name,
        pods.items.len()
    );

    if pods.items.is_empty() {
        return "No pod found for backup job.".to_string();
    }

    let mut logs = Vec::new();
    for pod in pods.items {
        let pod_name = pod.name_any();
        for container in ["snapshot-create", "snapshot-status", "snapshot-upload"] {
            let log_params = LogParams {
                container: Some(container.to_string()),
                ..Default::default()
            };

            match block_on(pod_api.logs(pod_name.as_str(), &log_params)) {
                Ok(output) if !output.trim().is_empty() => {
                    logs.push(format!("--- pod/{pod_name} container/{container} ---\n{output}"))
                }
                Ok(_) => {}
                Err(e) => {
                    debug!(
                        "Unable to fetch backup job logs for pod=`{}` container=`{}`: {}",
                        pod_name, container, e
                    );
                    logs.push(format!(
                        "--- pod/{pod_name} container/{container} ---\nUnable to fetch logs: {e}"
                    ))
                }
            }
        }
    }

    logs.join("\n\n")
}

fn cleanup_backup_resources(kube: &kube::Client, job_name: &str) -> Result<(), CommandError> {
    let job_api: Api<Job> = Api::namespaced(kube.clone(), EKSA_SYSTEM_NAMESPACE);
    debug!(
        "Cleaning up etcd backup resources: deleting job `{}/{}`.",
        EKSA_SYSTEM_NAMESPACE, job_name
    );
    if let Err(e) = block_on(job_api.delete(job_name, &DeleteParams::background()))
        && !is_not_found_error(&e)
    {
        return Err(CommandError::new_from_safe_message(format!(
            "Cannot delete backup job `{}/{}`: {e}",
            EKSA_SYSTEM_NAMESPACE, job_name
        )));
    }

    debug!("Cleanup request sent for job `{}/{}`.", EKSA_SYSTEM_NAMESPACE, job_name);
    Ok(())
}

fn debug_log_job_status(job_name: &str, job: &Job, elapsed: Duration, timeout: Duration) {
    let status = job.status.as_ref();
    let active = status.and_then(|s| s.active).unwrap_or_default();
    let succeeded = status.and_then(|s| s.succeeded).unwrap_or_default();
    let failed = status.and_then(|s| s.failed).unwrap_or_default();
    let conditions = status
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| {
                    format!(
                        "{}={} reason={} message={}",
                        c.type_,
                        c.status,
                        c.reason.clone().unwrap_or_default(),
                        c.message.clone().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    debug!(
        "Backup job poll: job=`{}` elapsed={}s/{}s active={} succeeded={} failed={} conditions={:?}",
        job_name,
        elapsed.as_secs(),
        timeout.as_secs(),
        active,
        succeeded,
        failed,
        conditions
    );
}

fn debug_log_job_pods(pod_api: &Api<Pod>, job_name: &str) {
    let selector = format!("job-name={job_name}");
    let pods = match block_on(pod_api.list(&ListParams::default().labels(selector.as_str()))) {
        Ok(pods) => pods,
        Err(e) => {
            debug!("Cannot list backup job pods for `{}`: {}", job_name, e);
            return;
        }
    };

    let snapshot = pods
        .items
        .iter()
        .map(|pod| {
            let phase = pod
                .status
                .as_ref()
                .and_then(|status| status.phase.clone())
                .unwrap_or_else(|| "<none>".to_string());
            let init_states = pod
                .status
                .as_ref()
                .and_then(|status| status.init_container_statuses.as_ref())
                .map(|statuses| {
                    statuses
                        .iter()
                        .map(|s| {
                            let state = if s.state.as_ref().and_then(|st| st.running.as_ref()).is_some() {
                                "running"
                            } else if s.state.as_ref().and_then(|st| st.waiting.as_ref()).is_some() {
                                "waiting"
                            } else if s.state.as_ref().and_then(|st| st.terminated.as_ref()).is_some() {
                                "terminated"
                            } else {
                                "unknown"
                            };
                            format!("{}:{}", s.name, state)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            format!("{} phase={} init={:?}", pod.name_any(), phase, init_states)
        })
        .collect::<Vec<_>>();

    debug!(
        "Backup job pods snapshot for `{}`: {} pod(s) -> {:?}",
        job_name,
        snapshot.len(),
        snapshot
    );
}

fn discover_etcd_endpoint(kube: &kube::Client, cluster_name: &str) -> Result<String, CommandError> {
    let selector = format!("cluster.x-k8s.io/etcd-cluster={cluster_name}-etcd");
    let machines = list_capi_machines(kube, selector.as_str())?;

    if machines.is_empty() {
        return Err(CommandError::new_from_safe_message(format!(
            "No etcd Machine found in `{}` with selector `{selector}`",
            EKSA_SYSTEM_NAMESPACE
        )));
    }

    let mut machine_addresses: Vec<(String, String)> = machines
        .iter()
        .filter_map(|machine| {
            extract_machine_ip(machine).map(|ip| {
                let machine_name = machine
                    .metadata
                    .name
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| "<unknown-machine>".to_string());
                (machine_name, ip)
            })
        })
        .collect();

    if machine_addresses.is_empty() {
        return Err(CommandError::new_from_safe_message(format!(
            "No usable address found on etcd Machine resources with selector `{selector}`"
        )));
    }

    machine_addresses.sort_by(|a, b| a.0.cmp(&b.0));
    let (_, selected_ip) = machine_addresses.remove(0);

    Ok(format!("https://{selected_ip}:2379"))
}

fn list_capi_machines(kube: &kube::Client, selector: &str) -> Result<Vec<DynamicObject>, CommandError> {
    for api_version in CAPI_MACHINE_API_VERSIONS {
        let gvk = GroupVersionKind::gvk("cluster.x-k8s.io", api_version, "Machine");
        let api: Api<DynamicObject> =
            Api::namespaced_with(kube.clone(), EKSA_SYSTEM_NAMESPACE, &ApiResource::from_gvk(&gvk));

        match block_on(api.list(&ListParams::default().labels(selector))) {
            Ok(machine_list) => return Ok(machine_list.items),
            Err(e) if is_missing_api_resource_error(&e) => continue,
            Err(e) => {
                return Err(CommandError::new_from_safe_message(format!(
                    "Cannot list CAPI Machine resources (`cluster.x-k8s.io/{api_version}`): {e}"
                )));
            }
        }
    }

    Err(CommandError::new_from_safe_message(
        "Cannot find a served CAPI Machine API version in this cluster.".to_string(),
    ))
}

fn extract_machine_ip(machine: &DynamicObject) -> Option<String> {
    let addresses = machine
        .data
        .get("status")
        .and_then(|status| status.get("addresses"))
        .and_then(Value::as_array)?;
    select_machine_ip(addresses)
}

fn select_machine_ip(addresses: &[Value]) -> Option<String> {
    let mut fallback: Option<String> = None;
    for address in addresses {
        let candidate = address.get("address").and_then(Value::as_str).map(str::trim);
        let Some(candidate) = candidate.filter(|ip| !ip.is_empty()) else {
            continue;
        };

        if address
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|ip_type| ip_type == "InternalIP")
        {
            return Some(candidate.to_string());
        }

        if fallback.is_none() {
            fallback = Some(candidate.to_string());
        }
    }

    fallback
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EtcdCertsSecretNames {
    client_secret_name: String,
    ca_secret_name: String,
    client_cert_key: String,
    client_private_key_key: String,
    ca_cert_key: String,
}

#[derive(Debug, Clone)]
struct EtcdSecretCandidate {
    name: String,
    data_keys: BTreeSet<String>,
}

fn resolve_etcd_certs_secret_names(
    kube: &kube::Client,
    cluster_name: &str,
    explicit_secret_name: Option<&str>,
) -> Result<EtcdCertsSecretNames, CommandError> {
    let api: Api<Secret> = Api::namespaced(kube.clone(), EKSA_SYSTEM_NAMESPACE);
    let secrets = block_on(api.list(&ListParams::default())).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot list secrets in `{}` for etcd certs discovery: {e}",
            EKSA_SYSTEM_NAMESPACE
        ))
    })?;

    let available_secrets = secrets
        .items
        .into_iter()
        .filter_map(|secret| {
            let name = secret.metadata.name?;
            let data_keys = secret.data.unwrap_or_default().into_keys().collect::<BTreeSet<_>>();
            Some(EtcdSecretCandidate { name, data_keys })
        })
        .collect::<Vec<_>>();

    let mut etcd_related_secret_names = available_secrets
        .iter()
        .filter(|secret| {
            secret.name.contains("etcd") || secret_has_client_material(secret) || secret_has_ca_material(secret)
        })
        .map(|secret| secret.name.clone())
        .collect::<Vec<_>>();
    etcd_related_secret_names.sort();
    etcd_related_secret_names.dedup();
    let etcd_related_summary = if etcd_related_secret_names.is_empty() {
        "<none>".to_string()
    } else {
        etcd_related_secret_names.join(", ")
    };
    debug!(
        "Discovered {} secret(s) in `{}`; etcd-related candidate(s): {}",
        available_secrets.len(),
        EKSA_SYSTEM_NAMESPACE,
        etcd_related_summary
    );

    if let Some(explicit_secret_name) = explicit_secret_name.map(str::trim).filter(|name| !name.is_empty()) {
        debug!(
            "Using configured etcd cert secret override `{explicit_secret_name}` and auto-detecting complementary secret if needed."
        );
    } else {
        debug!("No explicit etcd cert secret override configured; using split auto-detection.");
    }

    select_etcd_certs_secret_names(cluster_name, explicit_secret_name, &available_secrets)
}

fn select_etcd_certs_secret_names(
    cluster_name: &str,
    explicit_secret_name: Option<&str>,
    available_secrets: &[EtcdSecretCandidate],
) -> Result<EtcdCertsSecretNames, CommandError> {
    let explicit_secret_name = explicit_secret_name.map(str::trim).filter(|name| !name.is_empty());

    if let Some(explicit_secret_name) = explicit_secret_name {
        let explicit_secret = available_secrets
            .iter()
            .find(|secret| secret.name == explicit_secret_name)
            .ok_or_else(|| {
                CommandError::new_from_safe_message(format!(
                    "Configured etcd certs secret `{explicit_secret_name}` was not found in namespace `{EKSA_SYSTEM_NAMESPACE}`"
                ))
            })?;

        let explicit_has_client_material = secret_has_client_material(explicit_secret);
        let explicit_has_strong_ca_material = secret_has_strong_ca_material(explicit_secret);
        let explicit_has_ca_material = secret_has_ca_material(explicit_secret);

        if explicit_has_client_material && explicit_has_strong_ca_material {
            return build_etcd_certs_secret_names(explicit_secret_name, explicit_secret_name, available_secrets);
        }

        if explicit_has_client_material {
            let ca_secret_name = select_ca_secret_name(cluster_name, available_secrets, Some(explicit_secret_name))?;
            return build_etcd_certs_secret_names(explicit_secret_name, ca_secret_name.as_str(), available_secrets);
        }

        if explicit_has_ca_material {
            let client_secret_name =
                select_client_secret_name(cluster_name, available_secrets, Some(explicit_secret_name))?;
            return build_etcd_certs_secret_names(client_secret_name.as_str(), explicit_secret_name, available_secrets);
        }

        return Err(CommandError::new_from_safe_message(format!(
            "Configured etcd certs secret `{explicit_secret_name}` does not contain usable etcd client or CA keys"
        )));
    }

    if let Some(single_secret_name) = select_single_full_cert_secret(cluster_name, available_secrets)? {
        return build_etcd_certs_secret_names(
            single_secret_name.as_str(),
            single_secret_name.as_str(),
            available_secrets,
        );
    }

    let client_secret_name = select_client_secret_name(cluster_name, available_secrets, None)?;
    let selected_client_secret = available_secrets
        .iter()
        .find(|secret| secret.name == client_secret_name)
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Internal error: selected etcd client secret `{client_secret_name}` was not found"
            ))
        })?;
    let excluded_client_secret_for_ca = if secret_has_strong_ca_material(selected_client_secret) {
        None
    } else {
        Some(client_secret_name.as_str())
    };
    let ca_secret_name = select_ca_secret_name(cluster_name, available_secrets, excluded_client_secret_for_ca)?;

    build_etcd_certs_secret_names(client_secret_name.as_str(), ca_secret_name.as_str(), available_secrets)
}

fn build_etcd_certs_secret_names(
    client_secret_name: &str,
    ca_secret_name: &str,
    available_secrets: &[EtcdSecretCandidate],
) -> Result<EtcdCertsSecretNames, CommandError> {
    let client_secret = available_secrets
        .iter()
        .find(|secret| secret.name == client_secret_name)
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Internal error: selected etcd client secret `{client_secret_name}` was not found"
            ))
        })?;
    let ca_secret = available_secrets
        .iter()
        .find(|secret| secret.name == ca_secret_name)
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Internal error: selected etcd CA secret `{ca_secret_name}` was not found"
            ))
        })?;

    let client_cert_key = pick_secret_key(client_secret, &CLIENT_CERT_KEY_CANDIDATES, "etcd client certificate key")?;
    let client_private_key_key =
        pick_secret_key(client_secret, &CLIENT_PRIVATE_KEY_CANDIDATES, "etcd client private key key")?;
    let ca_cert_key = pick_secret_key_with_fallback(
        ca_secret,
        &CA_CERT_KEY_CANDIDATES,
        &CA_CERT_FALLBACK_KEY_CANDIDATES,
        "etcd CA certificate key",
    )?;

    Ok(EtcdCertsSecretNames {
        client_secret_name: client_secret_name.to_string(),
        ca_secret_name: ca_secret_name.to_string(),
        client_cert_key,
        client_private_key_key,
        ca_cert_key,
    })
}

fn pick_secret_key(secret: &EtcdSecretCandidate, candidates: &[&str], key_kind: &str) -> Result<String, CommandError> {
    candidates
        .iter()
        .find(|candidate| secret.data_keys.contains(**candidate))
        .map(|candidate| (*candidate).to_string())
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Cannot find {key_kind} in secret `{}`. Available keys: {}",
                secret.name,
                secret.data_keys.iter().cloned().collect::<Vec<_>>().join(", ")
            ))
        })
}

fn pick_secret_key_with_fallback(
    secret: &EtcdSecretCandidate,
    preferred_candidates: &[&str],
    fallback_candidates: &[&str],
    key_kind: &str,
) -> Result<String, CommandError> {
    pick_secret_key(secret, preferred_candidates, key_kind)
        .or_else(|_| pick_secret_key(secret, fallback_candidates, key_kind))
}

fn select_single_full_cert_secret(
    cluster_name: &str,
    available_secrets: &[EtcdSecretCandidate],
) -> Result<Option<String>, CommandError> {
    let candidates: Vec<&EtcdSecretCandidate> = available_secrets
        .iter()
        .filter(|s| {
            secret_has_client_material(s) && secret_has_strong_ca_material(s) && s.name.ends_with("-etcd-certs")
        })
        .collect();

    let preferred = format!("{cluster_name}-etcd-certs");
    if candidates.iter().any(|s| s.name == preferred) {
        return Ok(Some(preferred));
    }

    match candidates.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one.name.clone())),
        many => Err(CommandError::new_from_safe_message(format!(
            "Ambiguous full etcd cert secret for cluster `{cluster_name}` in namespace `{EKSA_SYSTEM_NAMESPACE}`: {}. \
Configure `certs_secret_name` explicitly to resolve the ambiguity.",
            many.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

fn select_client_secret_name(
    cluster_name: &str,
    available_secrets: &[EtcdSecretCandidate],
    excluded_secret_name: Option<&str>,
) -> Result<String, CommandError> {
    let candidates: Vec<&EtcdSecretCandidate> = available_secrets
        .iter()
        .filter(|s| excluded_secret_name.is_none_or(|e| s.name != e))
        .filter(|s| secret_has_client_material(s))
        .collect();

    for name in [
        format!("{cluster_name}-apiserver-etcd-client"),
        format!("{cluster_name}-etcd-certs"),
        format!("{cluster_name}-etcd-client"),
    ] {
        if candidates.iter().any(|s| s.name == name) {
            return Ok(name);
        }
    }

    let cluster_prefix = format!("{cluster_name}-");
    let cluster_candidates = candidates
        .iter()
        .copied()
        .filter(|s| s.name.starts_with(&cluster_prefix))
        .collect::<Vec<_>>();
    match cluster_candidates.as_slice() {
        [one] => return Ok(one.name.clone()),
        many if many.len() > 1 => {
            return Err(CommandError::new_from_safe_message(format!(
                "Ambiguous etcd client cert secret for cluster `{cluster_name}` in namespace `{EKSA_SYSTEM_NAMESPACE}`: {}",
                many.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
            )));
        }
        _ => {}
    }

    match candidates.as_slice() {
        [] => Err(CommandError::new_from_safe_message(format!(
            "Cannot find etcd client cert secret for cluster `{cluster_name}` in namespace `{EKSA_SYSTEM_NAMESPACE}`"
        ))),
        [one] => Ok(one.name.clone()),
        many => Err(CommandError::new_from_safe_message(format!(
            "Ambiguous etcd client cert secret for cluster `{cluster_name}` in namespace `{EKSA_SYSTEM_NAMESPACE}`: {}",
            many.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

fn select_ca_secret_name(
    cluster_name: &str,
    available_secrets: &[EtcdSecretCandidate],
    excluded_secret_name: Option<&str>,
) -> Result<String, CommandError> {
    let candidates: Vec<&EtcdSecretCandidate> = available_secrets
        .iter()
        .filter(|s| excluded_secret_name.is_none_or(|e| s.name != e))
        .filter(|s| secret_has_ca_material(s))
        .collect();

    for name in [
        format!("{cluster_name}-etcd"),
        format!("{cluster_name}-managed-etcd"),
        format!("{cluster_name}-etcd-certs"),
    ] {
        if candidates.iter().any(|s| s.name == name) {
            return Ok(name);
        }
    }

    // In the fallback, prefer secrets with a dedicated CA key (ca.crt/ca.pem)
    // over those matched only via the tls.crt fallback (e.g. apiserver-etcd-client).
    let strong: Vec<&EtcdSecretCandidate> = candidates
        .iter()
        .copied()
        .filter(|s| secret_has_strong_ca_material(s))
        .collect();
    let pool: &[&EtcdSecretCandidate] = if strong.is_empty() { &candidates } else { &strong };

    match pool {
        [] => Err(CommandError::new_from_safe_message(format!(
            "Cannot find etcd CA cert secret for cluster `{cluster_name}` in namespace `{EKSA_SYSTEM_NAMESPACE}`"
        ))),
        [one] => Ok(one.name.clone()),
        many => Err(CommandError::new_from_safe_message(format!(
            "Ambiguous etcd CA cert secret for cluster `{cluster_name}` in namespace `{EKSA_SYSTEM_NAMESPACE}`: {}",
            many.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

fn secret_has_client_material(secret: &EtcdSecretCandidate) -> bool {
    secret_has_any_key(secret, &CLIENT_CERT_KEY_CANDIDATES)
        && secret_has_any_key(secret, &CLIENT_PRIVATE_KEY_CANDIDATES)
}

fn secret_has_strong_ca_material(secret: &EtcdSecretCandidate) -> bool {
    secret_has_any_key(secret, &CA_CERT_KEY_CANDIDATES)
}

fn secret_has_ca_material(secret: &EtcdSecretCandidate) -> bool {
    secret_has_strong_ca_material(secret) || secret_has_any_key(secret, &CA_CERT_FALLBACK_KEY_CANDIDATES)
}

fn secret_has_any_key(secret: &EtcdSecretCandidate, key_candidates: &[&str]) -> bool {
    key_candidates
        .iter()
        .any(|candidate| secret.data_keys.contains(*candidate))
}

fn validate_presigned_put_url(presigned_put_url: &str, backup_kind: &str) -> Result<String, CommandError> {
    let normalized = presigned_put_url.trim();
    if normalized.is_empty() {
        return Err(CommandError::new_from_safe_message(format!(
            "Configured {backup_kind} pre-signed PUT URL is empty."
        )));
    }

    let parsed_url = Url::parse(normalized).map_err(|e| {
        CommandError::new_from_safe_message(format!("Configured {backup_kind} pre-signed PUT URL is invalid: {e}"))
    })?;

    if parsed_url.scheme() != "https" {
        return Err(CommandError::new_from_safe_message(format!(
            "Configured {backup_kind} pre-signed PUT URL must use `https`."
        )));
    }

    Ok(normalized.to_string())
}

fn redact_url_for_logs(url: &str) -> String {
    match Url::parse(url) {
        Ok(parsed_url) => {
            let host = parsed_url.host_str().unwrap_or("<unknown-host>");
            format!("{}://{}{}?[REDACTED]", parsed_url.scheme(), host, parsed_url.path())
        }
        Err(_) => "<invalid-url>".to_string(),
    }
}

fn cluster_name_from_config(cluster_config_path: &Path) -> Result<Option<String>, CommandError> {
    let content = fs::read_to_string(cluster_config_path).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot read EKS Anywhere cluster config file `{}`: {e}",
            cluster_config_path.display()
        ))
    })?;

    for yaml_doc in serde_yaml::Deserializer::from_str(&content) {
        let value = Value::deserialize(yaml_doc).map_err(|e| {
            CommandError::new_from_safe_message(format!(
                "Cannot parse EKS Anywhere cluster config `{}`: {e}",
                cluster_config_path.display()
            ))
        })?;

        if value.get("kind").and_then(Value::as_str) != Some("Cluster") {
            continue;
        }

        if let Some(name) = value
            .get("metadata")
            .and_then(|metadata| metadata.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            return Ok(Some(name.to_string()));
        }
    }

    Ok(None)
}

fn is_not_found_error(error: &kube::Error) -> bool {
    matches!(error, kube::Error::Api(api_error) if api_error.code == 404)
}

fn is_missing_api_resource_error(error: &kube::Error) -> bool {
    if is_not_found_error(error) {
        return true;
    }

    let lower = error.to_string().to_ascii_lowercase();
    lower.contains("no matches for kind")
        || lower.contains("the server could not find the requested resource")
        || lower.contains("could not find the requested resource")
}

fn short_random_suffix() -> String {
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

fn log_section_title(logger: &impl InfraLogger, icon: &str, title: &str) {
    logger.info("");
    logger.info(format!("***** {icon} {title} *****"));
    logger.info("");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn should_select_internal_ip_first() {
        let addresses = vec![
            json!({
                "type": "ExternalIP",
                "address": "1.2.3.4"
            }),
            json!({
                "type": "InternalIP",
                "address": "10.0.0.10"
            }),
        ];

        let ip = select_machine_ip(&addresses).expect("an IP should be selected");
        assert_eq!(ip, "10.0.0.10");
    }

    #[test]
    fn should_fallback_to_first_non_empty_address() {
        let addresses = vec![
            json!({
                "type": "Hostname",
                "address": "machine-a.local"
            }),
            json!({
                "type": "ExternalIP",
                "address": "  "
            }),
        ];

        let ip = select_machine_ip(&addresses).expect("an IP should be selected");
        assert_eq!(ip, "machine-a.local");
    }

    #[test]
    fn should_select_split_client_and_ca_secrets() {
        let selection = select_etcd_certs_secret_names(
            "cluster-a",
            None,
            &[
                secret_candidate("cluster-a-apiserver-etcd-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-a-etcd", &["tls.crt"]),
            ],
        )
        .expect("cert secrets should be selected");

        assert_eq!(
            selection,
            EtcdCertsSecretNames {
                client_secret_name: "cluster-a-apiserver-etcd-client".to_string(),
                ca_secret_name: "cluster-a-etcd".to_string(),
                client_cert_key: "tls.crt".to_string(),
                client_private_key_key: "tls.key".to_string(),
                ca_cert_key: "tls.crt".to_string(),
            }
        );
    }

    #[test]
    fn should_select_explicit_client_secret_and_detect_ca_secret() {
        let selection = select_etcd_certs_secret_names(
            "cluster-a",
            Some("cluster-a-apiserver-etcd-client"),
            &[
                secret_candidate("cluster-a-apiserver-etcd-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-a-etcd", &["tls.crt"]),
            ],
        )
        .expect("cert secrets should be selected");

        assert_eq!(
            selection,
            EtcdCertsSecretNames {
                client_secret_name: "cluster-a-apiserver-etcd-client".to_string(),
                ca_secret_name: "cluster-a-etcd".to_string(),
                client_cert_key: "tls.crt".to_string(),
                client_private_key_key: "tls.key".to_string(),
                ca_cert_key: "tls.crt".to_string(),
            }
        );
    }

    #[test]
    fn should_select_single_full_secret_when_available() {
        let selection = select_etcd_certs_secret_names(
            "cluster-a",
            None,
            &[secret_candidate(
                "cluster-a-etcd-certs",
                &["ca.crt", "tls.crt", "tls.key"],
            )],
        )
        .expect("cert secrets should be selected");

        assert_eq!(
            selection,
            EtcdCertsSecretNames {
                client_secret_name: "cluster-a-etcd-certs".to_string(),
                ca_secret_name: "cluster-a-etcd-certs".to_string(),
                client_cert_key: "tls.crt".to_string(),
                client_private_key_key: "tls.key".to_string(),
                ca_cert_key: "ca.crt".to_string(),
            }
        );
    }

    #[test]
    fn should_prefer_strong_ca_over_fallback_tls_crt_in_ca_selection() {
        // apiserver-etcd-client has tls.crt (fallback CA match) but cluster-a-etcd has ca.crt (strong match)
        // The strong candidate should win even though both pass secret_has_ca_material.
        let selection = select_etcd_certs_secret_names(
            "cluster-a",
            None,
            &[
                secret_candidate("cluster-a-custom-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-a-custom-ca", &["ca.crt"]),
                secret_candidate("cluster-a-fallback-only", &["tls.crt"]),
            ],
        )
        .expect("cert secrets should be selected");

        assert_eq!(selection.ca_secret_name, "cluster-a-custom-ca");
        assert_eq!(selection.ca_cert_key, "ca.crt");
    }

    #[test]
    fn should_fail_when_full_cert_secret_selection_is_ambiguous() {
        let error = select_etcd_certs_secret_names(
            "cluster-a",
            None,
            &[
                secret_candidate("cluster-x-etcd-certs", &["ca.crt", "tls.crt", "tls.key"]),
                secret_candidate("cluster-y-etcd-certs", &["ca.crt", "tls.crt", "tls.key"]),
            ],
        )
        .expect_err("selection should fail when multiple full-cert secrets match");

        assert!(error.message_safe().contains("Ambiguous full etcd cert secret"));
    }

    #[test]
    fn should_fail_when_client_secret_selection_is_ambiguous() {
        let error = select_etcd_certs_secret_names(
            "cluster-a",
            None,
            &[
                secret_candidate("cluster-a-first-etcd-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-a-second-etcd-client", &["tls.crt", "tls.key"]),
            ],
        )
        .expect_err("selection should fail");

        assert!(error.message_safe().contains("Ambiguous etcd client cert secret"));
    }

    #[test]
    fn should_prefer_cluster_prefixed_client_secret_when_other_cluster_secret_exists() {
        let selection = select_etcd_certs_secret_names(
            "cluster-a",
            None,
            &[
                secret_candidate("cluster-a-custom-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-b-custom-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-a-etcd", &["ca.crt"]),
            ],
        )
        .expect("selection should prefer single cluster-prefixed client secret");

        assert_eq!(selection.client_secret_name, "cluster-a-custom-client");
    }

    #[test]
    fn should_fail_when_multiple_cluster_prefixed_client_secrets_exist() {
        let error = select_etcd_certs_secret_names(
            "cluster-a",
            None,
            &[
                secret_candidate("cluster-a-first-custom-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-a-second-custom-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-b-custom-client", &["tls.crt", "tls.key"]),
                secret_candidate("cluster-a-etcd", &["ca.crt"]),
            ],
        )
        .expect_err("selection should fail when multiple cluster-prefixed client secrets exist");

        assert!(error.message_safe().contains("Ambiguous etcd client cert secret"));
        assert!(error.message_safe().contains("cluster-a-first-custom-client"));
        assert!(error.message_safe().contains("cluster-a-second-custom-client"));
        assert!(!error.message_safe().contains("cluster-b-custom-client"));
    }

    #[test]
    fn should_accept_https_presigned_put_url() {
        let url = validate_presigned_put_url("https://example.s3.eu-west-3.amazonaws.com/a.db?sig=abc", "etcd backup")
            .expect("URL should be valid");
        assert_eq!(url, "https://example.s3.eu-west-3.amazonaws.com/a.db?sig=abc");
    }

    #[test]
    fn should_reject_empty_presigned_put_url() {
        let error = validate_presigned_put_url("  ", "etcd backup").expect_err("URL should be rejected");
        assert!(error.message_safe().contains("is empty"));
    }

    #[test]
    fn should_reject_non_https_presigned_put_url() {
        let error = validate_presigned_put_url("http://example.com/a.db?sig=abc", "etcd backup")
            .expect_err("URL should be rejected");
        assert!(error.message_safe().contains("must use `https`"));
    }

    fn secret_candidate(name: &str, keys: &[&str]) -> EtcdSecretCandidate {
        EtcdSecretCandidate {
            name: name.to_string(),
            data_keys: keys.iter().map(|key| key.to_string()).collect(),
        }
    }
}
