use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::errors::CommandError;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhere;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tar::Builder as TarBuilder;
use tracing::{debug, info};
use url::Url;
use uuid::Uuid;

const EKSA_SYSTEM_NAMESPACE: &str = "eksa-system";
const CAPI_BACKUP_ARCHIVE_UPLOAD_TIMEOUT_SECONDS: u64 = 300;
const CAPI_BACKUP_MIN_UPLOAD_WINDOW_SECONDS: i64 = 900;
const CAPI_BACKUP_KUBECTL_TIMEOUT_SECONDS: u64 = 120;
const CAPI_BACKUP_METADATA_FILE_NAME: &str = ".qovery-capi-backup-metadata.json";
const CAPI_BACKUP_FALLBACK_EXPORT_RESOURCES: &[&str] = &[
    "clusters.cluster.x-k8s.io",
    "clusterresourcesets.addons.cluster.x-k8s.io",
    "clusterresourcesetbindings.addons.cluster.x-k8s.io",
    "kubeadmcontrolplanes.controlplane.cluster.x-k8s.io",
    "machinedeployments.cluster.x-k8s.io",
    "machines.cluster.x-k8s.io",
    "machinesets.cluster.x-k8s.io",
    "machinehealthchecks.cluster.x-k8s.io",
    "kubeadmconfigs.bootstrap.cluster.x-k8s.io",
    "kubeadmconfigtemplates.bootstrap.cluster.x-k8s.io",
    "etcdadmclusters.etcdcluster.cluster.x-k8s.io",
    "etcdadmconfigs.bootstrap.cluster.x-k8s.io",
    "vsphereclusters.infrastructure.cluster.x-k8s.io",
    "vspheremachines.infrastructure.cluster.x-k8s.io",
    "vspheremachinetemplates.infrastructure.cluster.x-k8s.io",
    "vspherevms.infrastructure.cluster.x-k8s.io",
    "configmaps",
    "secrets",
];

pub(super) fn upload_eks_anywhere_capi_backup(
    cluster: &EksAnywhere,
    backup_directory: &Path,
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
        logger.info("Skipping CAPI backup upload: disabled in EKS Anywhere parameters.");
        return Ok(());
    }

    log_section_title(logger, "📦", "EKS Anywhere CAPI backup upload");

    let presigned_put_url =
        validate_presigned_put_url(cluster_backup.s3.capi_presigned_put_url.as_str(), "CAPI backup")?;
    info!(
        "CAPI backup destination (pre-signed PUT URL): `{}`.",
        redact_url_for_logs(&presigned_put_url)
    );
    validate_presigned_put_url_upload_window(presigned_put_url.as_str(), CAPI_BACKUP_MIN_UPLOAD_WINDOW_SECONDS)?;
    if !backup_directory.exists() || !backup_directory.is_dir() {
        return Err(CommandError::new_from_safe_message(format!(
            "Explicit pre-upgrade CAPI backup directory `{}` is not available.",
            backup_directory.display()
        )));
    }
    let backup_metadata = read_capi_backup_metadata(backup_directory)?;
    let backup_name = capi_backup_directory_name(backup_directory)?;
    if backup_metadata.format == CapiBackupFormat::KubectlSnapshot {
        logger.info("Uploading kubectl CAPI snapshot.");
    } else {
        logger.info(format!(
            "Using explicit clusterctl CAPI backup captured before upgrade: `{}`.",
            backup_name
        ));
    }
    debug!("Found CAPI backup directory `{}`.", backup_directory.display());

    let archive_file_name = format!("{}-{}.tar.gz", backup_name, short_random_suffix());
    let archive_path = cluster.temp_dir().join(archive_file_name);

    create_tar_gz_archive_from_directory(backup_directory, &archive_path)?;
    debug!("Created CAPI backup archive `{}`.", archive_path.display());

    let upload_result = upload_file_to_presigned_put_url(presigned_put_url.as_str(), &archive_path);
    if let Err(cleanup_error) = fs::remove_file(&archive_path) {
        logger.warn(format!(
            "Cannot remove temporary CAPI backup archive `{}`: {}",
            archive_path.display(),
            cleanup_error
        ));
    }

    upload_result?;

    logger.info("CAPI backup archive uploaded.");
    log_section_title(logger, "✅", "CAPI backup upload completed");
    Ok(())
}

pub(super) fn run_eks_anywhere_capi_backup_before_upgrade(
    cluster: &EksAnywhere,
    cluster_config_path: &Path,
    logger: &impl InfraLogger,
) -> Result<Option<PathBuf>, CommandError> {
    let Some(cluster_backup) = cluster
        .options
        .infrastructure_charts_parameters
        .eks_anywhere_parameters
        .as_ref()
        .and_then(|params| params.cluster_backup.as_ref())
    else {
        return Ok(None);
    };

    if !cluster_backup.enabled {
        logger.info("Skipping CAPI backup creation: disabled in EKS Anywhere parameters.");
        return Ok(None);
    }

    log_section_title(logger, "📁", "EKS Anywhere CAPI backup creation");

    let presigned_put_url =
        validate_presigned_put_url(cluster_backup.s3.capi_presigned_put_url.as_str(), "CAPI backup")?;
    validate_presigned_put_url_upload_window(presigned_put_url.as_str(), CAPI_BACKUP_MIN_UPLOAD_WINDOW_SECONDS)?;

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

    let destination_directory = cluster.temp_dir().join(cluster_name.as_str()).join(format!(
        "{}-backup-{}",
        cluster_name,
        timestamp_for_backup_name()
    ));
    fs::create_dir_all(&destination_directory).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot create explicit CAPI backup directory `{}`: {e}",
            destination_directory.display()
        ))
    })?;
    let backup_name = capi_backup_directory_name(destination_directory.as_path())?;

    logger.info(format!("Creating CAPI snapshot before upgrade: `{backup_name}`."));

    let kubeconfig_path_str = cluster.kubeconfig_local_file_path().to_string_lossy().to_string();
    run_capi_backup_kubectl_export_fallback(
        cluster,
        destination_directory.as_path(),
        kubeconfig_path_str.as_str(),
        cluster_name.as_str(),
        logger,
    )?;
    let backup_format = CapiBackupFormat::KubectlSnapshot;

    if !destination_directory.exists() || !destination_directory.is_dir() {
        return Err(CommandError::new_from_safe_message(format!(
            "Explicit CAPI backup directory `{}` was not created.",
            destination_directory.display()
        )));
    }

    let has_files = fs::read_dir(&destination_directory)
        .map_err(|e| {
            CommandError::new_from_safe_message(format!(
                "Cannot inspect explicit CAPI backup directory `{}`: {e}",
                destination_directory.display()
            ))
        })?
        .next()
        .transpose()
        .map_err(|e| {
            CommandError::new_from_safe_message(format!(
                "Cannot read explicit CAPI backup directory entries `{}`: {e}",
                destination_directory.display()
            ))
        })?
        .is_some();

    if !has_files {
        return Err(CommandError::new_from_safe_message(format!(
            "Explicit CAPI backup directory `{}` is empty after backup command.",
            destination_directory.display()
        )));
    }

    write_capi_backup_metadata(destination_directory.as_path(), backup_format, has_files)?;

    match backup_format {
        CapiBackupFormat::ClusterctlMove => logger.info(format!("CAPI backup created successfully: `{backup_name}`.")),
        CapiBackupFormat::KubectlSnapshot => {
            logger.info(format!("CAPI snapshot created successfully: `{backup_name}`."))
        }
    }
    log_section_title(logger, "✅", "CAPI backup creation completed");
    Ok(Some(destination_directory))
}

fn run_capi_backup_kubectl_export_fallback(
    cluster: &EksAnywhere,
    destination_directory: &Path,
    kubeconfig_path: &str,
    cluster_name: &str,
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    fs::create_dir_all(destination_directory).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot ensure CAPI export directory `{}`: {e}",
            destination_directory.display()
        ))
    })?;

    let label_selector = format!("cluster.x-k8s.io/cluster-name={cluster_name}");
    let mut exported_files_count = 0usize;
    let mut listed_objects_count = 0usize;
    let mut per_resource_stats: Vec<(String, usize, usize)> = Vec::new();
    for resource in CAPI_BACKUP_FALLBACK_EXPORT_RESOURCES {
        let mut listed_for_resource = 0usize;
        let mut exported_for_resource = 0usize;
        let list_args = [
            "get",
            *resource,
            "-n",
            EKSA_SYSTEM_NAMESPACE,
            "-l",
            label_selector.as_str(),
            "-o",
            "name",
            "--kubeconfig",
            kubeconfig_path,
        ];
        let (stdout, stderr) = run_command_collect_output(
            cluster,
            "kubectl",
            &list_args,
            Duration::from_secs(CAPI_BACKUP_KUBECTL_TIMEOUT_SECONDS),
        )?;
        if !stderr.is_empty() {
            logger.warn(format!("kubectl listing stderr for `{}`: {}", resource, stderr.join(" | ")));
        }

        for object_ref in stdout.iter().map(|line| line.trim()).filter(|line| !line.is_empty()) {
            listed_for_resource += 1;
            let get_args = [
                "get",
                object_ref,
                "-n",
                EKSA_SYSTEM_NAMESPACE,
                "-o",
                "yaml",
                "--kubeconfig",
                kubeconfig_path,
            ];
            let (object_stdout, object_stderr) = run_command_collect_output(
                cluster,
                "kubectl",
                &get_args,
                Duration::from_secs(CAPI_BACKUP_KUBECTL_TIMEOUT_SECONDS),
            )?;
            if !object_stderr.is_empty() {
                logger.warn(format!(
                    "kubectl get stderr for `{}`: {}",
                    object_ref,
                    object_stderr.join(" | ")
                ));
            }

            let file_name = format!("{}.yaml", sanitize_resource_name_for_file(object_ref));
            let file_path = destination_directory.join(file_name);
            fs::write(&file_path, object_stdout.join("\n")).map_err(|e| {
                CommandError::new_from_safe_message(format!(
                    "Cannot write CAPI export file `{}`: {e}",
                    file_path.display()
                ))
            })?;
            exported_files_count += 1;
            exported_for_resource += 1;
        }

        listed_objects_count += listed_for_resource;
        per_resource_stats.push((resource.to_string(), listed_for_resource, exported_for_resource));
    }

    if exported_files_count == 0 {
        return Err(CommandError::new_from_safe_message(format!(
            "CAPI export produced no files under `{}`.",
            destination_directory.display()
        )));
    }

    let resource_types_touched = per_resource_stats
        .iter()
        .filter(|(_, listed, exported)| *listed > 0 || *exported > 0)
        .count();
    logger.info(format!(
        "CAPI export completed: {} file(s) across {} resource type(s) (listed {} object(s), exported {}).",
        exported_files_count, resource_types_touched, listed_objects_count, exported_files_count
    ));
    logger.info("CAPI export uses raw kubectl YAML.");
    Ok(())
}

fn run_command_collect_output(
    cluster: &EksAnywhere,
    command: &str,
    args: &[&str],
    timeout_duration: Duration,
) -> Result<(Vec<String>, Vec<String>), CommandError> {
    let mut cmd = QoveryCommand::new(command, args, &[]);
    cmd.set_current_dir(cluster.temp_dir());

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let timeout = CommandKiller::from_timeout(timeout_duration);
    cmd.exec_with_abort(&mut |line| stdout.push(line), &mut |line| stderr.push(line), &timeout)
        .map_err(|e| {
            CommandError::new(
                format!("Cannot run `{}` command", command),
                Some(format!(
                    "args: {:?} / stderr: {} / stdout: {} / error: {}",
                    args,
                    stderr.join(" | "),
                    stdout.join(" | "),
                    e
                )),
                None,
            )
        })?;

    Ok((stdout, stderr))
}

fn sanitize_resource_name_for_file(resource_name: &str) -> String {
    resource_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CapiBackupFormat {
    ClusterctlMove,
    KubectlSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CapiBackupMetadata {
    format: CapiBackupFormat,
    completed: bool,
}

fn write_capi_backup_metadata(directory: &Path, format: CapiBackupFormat, completed: bool) -> Result<(), CommandError> {
    let metadata_path = directory.join(CAPI_BACKUP_METADATA_FILE_NAME);
    let metadata = CapiBackupMetadata { format, completed };
    let raw = serde_json::to_vec_pretty(&metadata).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot serialize CAPI backup metadata `{}`: {e}",
            metadata_path.display()
        ))
    })?;
    fs::write(&metadata_path, raw).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot write CAPI backup metadata `{}`: {e}",
            metadata_path.display()
        ))
    })?;
    Ok(())
}

fn read_capi_backup_metadata(directory: &Path) -> Result<CapiBackupMetadata, CommandError> {
    let metadata_path = directory.join(CAPI_BACKUP_METADATA_FILE_NAME);
    let raw = fs::read_to_string(&metadata_path).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot read CAPI backup metadata `{}`: {e}",
            metadata_path.display()
        ))
    })?;
    let parsed: CapiBackupMetadata = serde_json::from_str(raw.as_str()).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot parse CAPI backup metadata `{}`: {e}",
            metadata_path.display()
        ))
    })?;
    if !parsed.completed {
        return Err(CommandError::new_from_safe_message(format!(
            "CAPI backup metadata `{}` indicates an incomplete backup export.",
            metadata_path.display()
        )));
    }
    Ok(parsed)
}

fn validate_presigned_put_url_upload_window(
    presigned_put_url: &str,
    min_window_seconds: i64,
) -> Result<(), CommandError> {
    let parsed = Url::parse(presigned_put_url).map_err(|e| {
        CommandError::new_from_safe_message(format!("Cannot parse pre-signed PUT URL for upload window checks: {e}"))
    })?;
    let mut x_amz_date = None;
    let mut x_amz_expires = None;
    for (key, value) in parsed.query_pairs() {
        if key.eq_ignore_ascii_case("X-Amz-Date") {
            x_amz_date = Some(value.to_string());
        } else if key.eq_ignore_ascii_case("X-Amz-Expires") {
            x_amz_expires = Some(value.to_string());
        }
    }

    let (Some(amz_date), Some(amz_expires)) = (x_amz_date, x_amz_expires) else {
        return Ok(());
    };
    let signed_at = chrono::NaiveDateTime::parse_from_str(amz_date.as_str(), "%Y%m%dT%H%M%SZ")
        .map_err(|e| CommandError::new_from_safe_message(format!("Invalid `X-Amz-Date` in pre-signed URL: {e}")))?;
    let expires_seconds: i64 = amz_expires
        .parse()
        .map_err(|e| CommandError::new_from_safe_message(format!("Invalid `X-Amz-Expires` in pre-signed URL: {e}")))?;
    let expires_at = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(signed_at, chrono::Utc)
        + chrono::Duration::seconds(expires_seconds);
    let remaining = expires_at.signed_duration_since(chrono::Utc::now()).num_seconds();
    if remaining <= 0 {
        return Err(CommandError::new_from_safe_message(
            "Pre-signed PUT URL for CAPI backup is already expired.".to_string(),
        ));
    }
    if remaining < min_window_seconds {
        return Err(CommandError::new_from_safe_message(format!(
            "Pre-signed PUT URL for CAPI backup expires too soon ({}s remaining, minimum required {}s).",
            remaining, min_window_seconds
        )));
    }
    Ok(())
}

fn capi_backup_directory_name(capi_backup_directory: &Path) -> Result<String, CommandError> {
    capi_backup_directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Cannot derive CAPI backup directory name from path `{}`.",
                capi_backup_directory.display()
            ))
        })
}

fn create_tar_gz_archive_from_directory(
    source_directory: &Path,
    destination_archive_path: &Path,
) -> Result<(), CommandError> {
    let archive_file = File::create(destination_archive_path).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot create CAPI backup archive `{}`: {e}",
            destination_archive_path.display()
        ))
    })?;

    let encoder = GzEncoder::new(archive_file, Compression::fast());
    let mut tar_builder = TarBuilder::new(encoder);
    let archive_root = capi_backup_directory_name(source_directory)?;
    tar_builder
        .append_dir_all(archive_root.as_str(), source_directory)
        .map_err(|e| {
            CommandError::new_from_safe_message(format!(
                "Cannot append CAPI backup directory `{}` to archive `{}`: {e}",
                source_directory.display(),
                destination_archive_path.display()
            ))
        })?;

    let encoder = tar_builder.into_inner().map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot finalize CAPI backup tar archive `{}`: {e}",
            destination_archive_path.display()
        ))
    })?;
    encoder.finish().map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot finalize CAPI backup gzip archive `{}`: {e}",
            destination_archive_path.display()
        ))
    })?;

    Ok(())
}

fn upload_file_to_presigned_put_url(presigned_put_url: &str, file_path: &Path) -> Result<(), CommandError> {
    let file = File::open(file_path).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot open backup archive `{}` for upload: {e}",
            file_path.display()
        ))
    })?;

    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| CommandError::new_from_safe_message(format!("Cannot create upload HTTP client: {e}")))?
        .put(presigned_put_url)
        .body(file)
        .timeout(Duration::from_secs(CAPI_BACKUP_ARCHIVE_UPLOAD_TIMEOUT_SECONDS))
        .send()
        .map_err(|e| CommandError::new_from_safe_message(format!("CAPI backup upload request failed: {e}")))?
        .error_for_status()
        .map_err(|e| {
            CommandError::new_from_safe_message(format!("CAPI backup upload returned an error status: {e}"))
        })?;

    Ok(())
}

fn validate_presigned_put_url(presigned_put_url: &str, backup_kind: &str) -> Result<String, CommandError> {
    let sanitized = presigned_put_url.trim();
    if sanitized.is_empty() {
        return Err(CommandError::new_from_safe_message(format!(
            "Pre-signed PUT URL for {} is empty.",
            backup_kind
        )));
    }

    let parsed = Url::parse(sanitized).map_err(|e| {
        CommandError::new_from_safe_message(format!("Pre-signed PUT URL for {} is invalid: {}", backup_kind, e))
    })?;

    if parsed.scheme() != "https" {
        return Err(CommandError::new_from_safe_message(format!(
            "Pre-signed PUT URL for {} must use `https`.",
            backup_kind
        )));
    }

    Ok(sanitized.to_string())
}

fn redact_url_for_logs(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            if parsed.query().is_some() {
                parsed.set_query(Some("[REDACTED]"));
            }
            parsed.to_string()
        }
        Err(_) => "[INVALID_URL]".to_string(),
    }
}

fn cluster_name_from_config(cluster_config_path: &Path) -> Result<Option<String>, CommandError> {
    let raw = fs::read_to_string(cluster_config_path).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Cannot read cluster config file `{}`: {e}",
            cluster_config_path.display()
        ))
    })?;

    for yaml_doc in serde_yaml::Deserializer::from_str(&raw) {
        let doc = Value::deserialize(yaml_doc).map_err(|e| {
            CommandError::new_from_safe_message(format!(
                "Cannot parse cluster config file `{}` as YAML: {e}",
                cluster_config_path.display()
            ))
        })?;

        let kind = doc
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if kind != "cluster" {
            continue;
        }

        if let Some(name) = doc
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

fn short_random_suffix() -> String {
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

fn timestamp_for_backup_name() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H_%M_%S").to_string()
}

fn log_section_title(logger: &impl InfraLogger, icon: &str, title: &str) {
    logger.info("");
    logger.info(format!("***** {icon} {title} *****"));
    logger.info("");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_write_and_read_capi_backup_metadata() {
        let temp_directory = tempfile::tempdir().expect("temp dir should be created");
        let backup_directory = temp_directory.path().join("eksa-powens-backup");
        fs::create_dir_all(&backup_directory).expect("backup directory should be created");

        write_capi_backup_metadata(&backup_directory, CapiBackupFormat::ClusterctlMove, true)
            .expect("metadata should be written");
        let metadata = read_capi_backup_metadata(&backup_directory).expect("metadata should be readable");

        assert_eq!(metadata.format, CapiBackupFormat::ClusterctlMove);
        assert!(metadata.completed);
    }
}
