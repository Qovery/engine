use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::errors::{CommandError, ErrorMessageVerbosity};
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

const COMMAND_STDOUT_PREFIX: &str = "CMD│ ";
const COMMAND_STDERR_PREFIX: &str = "CMD┃ ";

#[derive(Debug, Clone, PartialEq, Eq)]
struct VSphereTemplateRef {
    machine_config_name: String,
    template: Option<String>,
    os_family: Option<String>,
    datastore: Option<String>,
    resource_pool: Option<String>,
    folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct VSphereClusterMetadata {
    kubernetes_version: Option<String>,
    vcenter_server: Option<String>,
    insecure: Option<bool>,
    network: Option<String>,
}

#[derive(Debug, Clone)]
struct VSphereTemplateInstallConfig {
    template_path: String,
    template_name: String,
    folder_path: String,
    datastore: String,
    resource_pool: String,
    os_family: Option<String>,
}

type TemplateIndexEntry = (BTreeSet<String>, BTreeSet<String>, Vec<VSphereTemplateRef>);

pub(super) fn run_vsphere_preflight(
    cluster_config_path: &Path,
    cloud_provider: &dyn CloudProvider,
    install_missing: bool,
    expected_eksd_release_tag: Option<&str>,
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    let content = fs::read_to_string(cluster_config_path).map_err(|e| {
        CommandError::new(
            format!("Cannot read cluster config file {}", cluster_config_path.display()),
            Some(e.to_string()),
            None,
        )
    })?;

    let templates = extract_vsphere_templates_from_yaml(&content)?;
    let metadata = extract_vsphere_cluster_metadata_from_yaml(&content)?;

    log_vsphere_section_title(logger, "🖥️", "vSphere preflight");

    if templates.is_empty() {
        logger.info("ℹ️ No vSphere machine config found in cluster config.");
        return Ok(());
    }

    for line in summarize_vsphere_templates_for_user(
        &templates,
        cluster_config_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| cluster_config_path.display().to_string()),
    ) {
        logger.info(line);
    }

    let govc_env = build_govc_envs(cloud_provider, &metadata);
    validate_govc_auth_envs(&govc_env)?;
    log_govc_version(logger, &govc_env);
    check_templates_with_govc(
        &templates,
        &metadata,
        &govc_env,
        cluster_config_path,
        install_missing,
        expected_eksd_release_tag,
        logger,
    )?;
    log_vsphere_section_title(logger, "✅", "vSphere preflight completed");

    Ok(())
}

fn log_vsphere_section_title(logger: &impl InfraLogger, icon: &str, title: &str) {
    logger.info("");
    logger.info(format!("***** {icon} {title} *****"));
    logger.info("");
}

fn build_govc_envs(cloud_provider: &dyn CloudProvider, metadata: &VSphereClusterMetadata) -> Vec<(String, String)> {
    let mut envs: Vec<(String, String)> = cloud_provider
        .credentials_environment_variables()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    inject_govc_env_if_missing(&mut envs, "GOVC_USERNAME", &["VSPHERE_USER"]);
    inject_govc_env_if_missing(&mut envs, "GOVC_PASSWORD", &["VSPHERE_PASSWORD"]);
    inject_govc_env_if_missing(&mut envs, "GOVC_URL", &[]);
    inject_govc_env_if_missing(&mut envs, "GOVC_INSECURE", &[]);
    inject_govc_env_if_missing(&mut envs, "GOVC_PERSIST_SESSION", &[]);

    if let Some(server) = metadata.vcenter_server.as_ref()
        && !envs.iter().any(|(k, _)| k == "GOVC_URL")
    {
        let govc_url = if server.starts_with("http://") || server.starts_with("https://") {
            server.to_string()
        } else {
            format!("https://{server}")
        };
        envs.push(("GOVC_URL".to_string(), govc_url));
    }

    if let Some(insecure) = metadata.insecure
        && !envs.iter().any(|(k, _)| k == "GOVC_INSECURE")
    {
        envs.push(("GOVC_INSECURE".to_string(), if insecure { "1" } else { "0" }.to_string()));
    }

    if !envs.iter().any(|(k, _)| k == "GOVC_PERSIST_SESSION") {
        // Avoid stale govc session cache between runs that can hide recent tag changes.
        envs.push(("GOVC_PERSIST_SESSION".to_string(), "false".to_string()));
    }

    envs
}

fn inject_govc_env_if_missing(envs: &mut Vec<(String, String)>, env_name: &str, fallback_env_names: &[&str]) {
    if envs.iter().any(|(k, _)| k == env_name) {
        return;
    }

    for candidate in std::iter::once(env_name).chain(fallback_env_names.iter().copied()) {
        let Ok(value) = env::var(candidate) else {
            continue;
        };
        if value.trim().is_empty() {
            continue;
        }
        envs.push((env_name.to_string(), value));
        return;
    }
}

fn govc_env_value<'a>(govc_env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    govc_env
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn validate_govc_auth_envs(govc_env: &[(String, String)]) -> Result<(), CommandError> {
    let username = govc_env_value(govc_env, "GOVC_USERNAME");
    let password = govc_env_value(govc_env, "GOVC_PASSWORD");

    if username.is_some() ^ password.is_some() {
        return Err(CommandError::new_from_safe_message(
            "Incomplete vSphere credentials for govc: set both `GOVC_USERNAME` and `GOVC_PASSWORD`.".to_string(),
        ));
    }

    let has_user_password = username.is_some() && password.is_some();
    let has_client_cert_auth = govc_env_value(govc_env, "GOVC_TLS_CERTIFICATE").is_some()
        && govc_env_value(govc_env, "GOVC_TLS_KEY").is_some();
    let has_url_user_info = govc_env_value(govc_env, "GOVC_URL")
        .and_then(|govc_url| Url::parse(govc_url).ok())
        .map(|url| !url.username().is_empty())
        .unwrap_or(false);

    if has_user_password || has_client_cert_auth || has_url_user_info {
        return Ok(());
    }

    Err(CommandError::new_from_safe_message(
        "Missing vSphere credentials for govc preflight. Set `GOVC_USERNAME`/`GOVC_PASSWORD` (or `VSPHERE_USER`/`VSPHERE_PASSWORD`).".to_string(),
    ))
}

fn log_govc_version(logger: &impl InfraLogger, govc_env: &[(String, String)]) {
    match run_govc_command(&["version"], govc_env) {
        Ok(lines) if !lines.is_empty() => logger.info(format!("Using govc: {}", lines.join(" ").trim())),
        _ => logger.warn("Unable to get `govc` version using `govc version`."),
    }
}

fn check_templates_with_govc(
    templates: &[VSphereTemplateRef],
    metadata: &VSphereClusterMetadata,
    govc_env: &[(String, String)],
    cluster_config_path: &Path,
    install_missing: bool,
    expected_eksd_release_tag: Option<&str>,
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    let mut missing_template_field = Vec::new();
    let mut templates_index: BTreeMap<String, TemplateIndexEntry> = BTreeMap::new();

    for template_ref in templates {
        let Some(template) = template_ref.template.as_ref() else {
            missing_template_field.push(template_ref.machine_config_name.clone());
            continue;
        };

        let (machine_configs, os_families, refs) = templates_index
            .entry(template.clone())
            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), Vec::new()));
        machine_configs.insert(template_ref.machine_config_name.clone());
        if let Some(os_family) = template_ref.os_family.as_ref() {
            os_families.insert(os_family.to_lowercase());
        }
        refs.push(template_ref.clone());
    }

    if !missing_template_field.is_empty() {
        missing_template_field.sort();
        return Err(CommandError::new_from_safe_message(format!(
            "Missing `spec.template` in VSphereMachineConfig for [{}]",
            missing_template_field
                .into_iter()
                .map(|name| backtick(&name))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let expected_eksd_fragment = metadata
        .kubernetes_version
        .as_deref()
        .and_then(expected_eksd_release_fragment_from_kubernetes_version);
    let expected_eksd_tag = expected_eksd_release_tag.map(str::to_string);

    logger.info("🧪 Running vSphere template checks.");
    logger.info(format!("🔎 Validating {} unique template(s) with govc.", templates_index.len()));

    for (template, (_machine_configs, os_families, refs)) in templates_index {
        let mut planned_retag_tag: Option<String> = None;
        let vm_info_err = run_govc_command(&["vm.info", "-json", template.as_str()], govc_env).err();
        if let Some(err) = vm_info_err {
            if !install_missing {
                return Err(CommandError::new(
                    format!("Template `{template}` is not available in vSphere"),
                    Some(err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    None,
                ));
            }

            let install_config = build_template_install_config(template.as_str(), &refs)?;
            logger.warn(format!(
                "⚠️ Template `{}` not found. Non dry-run mode: automatic OVA import will be attempted.",
                template_label(template.as_str())
            ));
            install_missing_template(
                cluster_config_path,
                &install_config,
                expected_eksd_tag.as_deref(),
                metadata,
                govc_env,
                logger,
            )?;

            run_govc_command(&["vm.info", "-json", template.as_str()], govc_env).map_err(|e| {
                CommandError::new(
                    format!("Template `{template}` is still not available in vSphere after import attempt"),
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    None,
                )
            })?;
        }

        let attached_tags =
            run_govc_command(&["tags.attached.ls", "-r", template.as_str()], govc_env).map_err(|e| {
                CommandError::new(
                    format!("Unable to list tags attached to template `{template}`"),
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    None,
                )
            })?;
        debug!(
            "vSphere preflight tag snapshot for template `{}`: {:?}",
            template, attached_tags
        );

        let expected_eksd_tag_name = expected_eksd_tag
            .as_deref()
            .map(|expected_tag| canonical_tag_name_for_category("eksdRelease", expected_tag));

        let mut has_expected_eksd_release = match expected_eksd_tag_name.as_deref() {
            Some(expected_tag_name) => {
                is_tag_directly_attached_to_object("eksdRelease", expected_tag_name, template.as_str(), govc_env)?
            }
            None => has_expected_eksd_release_tag(&attached_tags, expected_eksd_fragment.as_deref()),
        };

        if !has_expected_eksd_release {
            let retag_eligibility = expected_eksd_tag
                .as_deref()
                .map(|expected_tag| {
                    assess_template_retag_eligibility(
                        cluster_config_path,
                        template.as_str(),
                        &attached_tags,
                        expected_tag,
                        expected_eksd_fragment.as_deref(),
                    )
                })
                .transpose()?;

            if matches!(retag_eligibility, Some(RetagEligibility::Eligible)) {
                if install_missing {
                    let expected_tag_name = expected_eksd_tag_name.as_deref().ok_or_else(|| {
                        CommandError::new_from_safe_message("Missing expected eksdRelease tag".to_string())
                    })?;
                    logger.info(format!(
                        "🏷️ Template `{}` is compatible but missing `{expected_tag_name}`. Applying tag now.",
                        template_label(template.as_str())
                    ));
                    match ensure_tag_and_attach("eksdRelease", expected_tag_name, template.as_str(), govc_env) {
                        Ok(()) => {
                            has_expected_eksd_release = is_tag_directly_attached_to_object(
                                "eksdRelease",
                                expected_tag_name,
                                template.as_str(),
                                govc_env,
                            )?;
                            if !has_expected_eksd_release {
                                return Err(CommandError::new_from_safe_message(format!(
                                    "Template `{template}` is still missing exact tag `{expected_tag_name}` after retag attempt"
                                )));
                            }
                        }
                        Err(attach_error) if is_tagging_cardinality_violation(&attach_error) => {
                            return Err(CommandError::new(
                                format!(
                                    "Cannot apply required exact tag `{expected_tag_name}` on template `{template}` due to vSphere tag cardinality. \
EKS Anywhere upgrade requires this exact tag; minor-compatible tags are not sufficient."
                                ),
                                Some(attach_error.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                                None,
                            ));
                        }
                        Err(attach_error) => return Err(attach_error),
                    }
                } else if let Some(expected_tag_name) = expected_eksd_tag_name.as_deref() {
                    planned_retag_tag = Some(expected_tag_name.to_string());
                }
            } else {
                if let Some(RetagEligibility::NotEligible(reason)) = retag_eligibility {
                    return Err(CommandError::new_from_safe_message(format!(
                        "Template `{}` is missing expected `eksdRelease` tag and cannot be safely retagged: {}",
                        template_label(template.as_str()),
                        reason
                    )));
                }
                let expected_display = expected_eksd_tag_name
                    .as_deref()
                    .map(|tag| format!("`{tag}`"))
                    .unwrap_or_else(|| "`eksdRelease`".to_string());
                return Err(CommandError::new_from_safe_message(format!(
                    "Template `{template}` does not have expected {expected_display} and cannot be safely retagged"
                )));
            }
        }

        if let Some(expected_os_family) = os_families.first()
            && !has_expected_os_tag(&attached_tags, expected_os_family)
        {
            return Err(CommandError::new_from_safe_message(format!(
                "Template `{template}` does not have expected OS tag for `{expected_os_family}`"
            )));
        }

        logger.info(format!(
            "✅ Template `{}` is present in vSphere.",
            template_label(template.as_str())
        ));
        if let Some(expected_tag_name) = expected_eksd_tag_name.as_deref() {
            if has_expected_eksd_release {
                logger.info(format!("🏷️ Tag `eksdRelease`: matches `{expected_tag_name}`."));
            } else {
                logger.info(format!(
                    "🏷️ Tag `eksdRelease`: missing `{expected_tag_name}` (dry-run only, planned to be attached in apply mode)."
                ));
            }
        } else if let Some(expected_fragment) = expected_eksd_fragment.as_deref() {
            logger.info(format!("🏷️ Tag `eksdRelease`: matches `{expected_fragment}`."));
        } else {
            logger.info("🏷️ Tag `eksdRelease`: present.");
        }

        if let Some(expected_os_family) = os_families.first() {
            logger.info(format!("🏷️ Tag `os`: matches `{expected_os_family}`."));
        }
        if let Some(expected_tag) = planned_retag_tag.as_deref() {
            logger.warn(format!(
                "Dry-run: template `{}` is present and compatible; only tag `{expected_tag}` is missing. Apply mode will attach this tag.",
                template_label(template.as_str())
            ));
        }
    }

    Ok(())
}

fn has_exact_eksd_release_tag(attached_tags: &[String], expected_tag: &str) -> bool {
    let normalized_expected = canonical_tag_value_for_category("eksdRelease", expected_tag);
    attached_tag_values_for_category(attached_tags, "eksdRelease")
        .iter()
        .any(|current| current.eq_ignore_ascii_case(normalized_expected.as_str()))
}

fn assess_template_retag_eligibility(
    cluster_config_path: &Path,
    template_path: &str,
    attached_tags: &[String],
    expected_eksd_tag: &str,
    expected_minor_fragment: Option<&str>,
) -> Result<RetagEligibility, CommandError> {
    if has_exact_eksd_release_tag(attached_tags, expected_eksd_tag) {
        return Ok(RetagEligibility::Eligible);
    }

    let Some(expected_minor_fragment) = expected_minor_fragment else {
        return Ok(RetagEligibility::NotEligible(
            "cannot infer expected Kubernetes minor branch for this cluster".to_string(),
        ));
    };

    let has_same_minor_eksd_release = attached_tag_values_for_category(attached_tags, "eksdRelease")
        .iter()
        .any(|tag_value| tag_value.to_lowercase().contains(expected_minor_fragment));
    if !has_same_minor_eksd_release {
        return Ok(RetagEligibility::NotEligible(format!(
            "template does not already carry an `eksdRelease` tag for `{expected_minor_fragment}`"
        )));
    }

    let template_name = template_name_from_path(template_path).ok_or_else(|| {
        CommandError::new_from_safe_message(format!(
            "Invalid template path `{template_path}` in VSphereMachineConfig (cannot resolve template name)"
        ))
    })?;
    let expected_template_name = expected_ova_template_name_for_template(cluster_config_path, template_name.as_str())?;
    if !are_template_names_compatible(template_name.as_str(), expected_template_name.as_str()) {
        return Ok(RetagEligibility::NotEligible(format!(
            "template `{template_name}` is not compatible with expected OVA template `{expected_template_name}`"
        )));
    }

    Ok(RetagEligibility::Eligible)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RetagEligibility {
    Eligible,
    NotEligible(String),
}

fn build_template_install_config(
    template_path: &str,
    refs: &[VSphereTemplateRef],
) -> Result<VSphereTemplateInstallConfig, CommandError> {
    let template_name = template_name_from_path(template_path).ok_or_else(|| {
        CommandError::new_from_safe_message(format!(
            "Invalid template path `{template_path}` in VSphereMachineConfig (cannot resolve template name)"
        ))
    })?;
    let folder_path = template_folder_from_path(template_path)
        .or_else(|| refs.iter().find_map(|r| r.folder.as_ref().cloned()))
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Invalid template path `{template_path}` in VSphereMachineConfig (cannot resolve template folder)"
            ))
        })?;

    let datastore = refs
        .iter()
        .find_map(|r| r.datastore.as_ref())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!("Missing `spec.datastore` for template `{template_path}`"))
        })?;

    let resource_pool = refs
        .iter()
        .find_map(|r| r.resource_pool.as_ref())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!("Missing `spec.resourcePool` for template `{template_path}`"))
        })?;

    let os_family = refs.iter().find_map(|r| r.os_family.as_ref()).map(|v| v.to_lowercase());

    Ok(VSphereTemplateInstallConfig {
        template_path: template_path.to_string(),
        template_name,
        folder_path,
        datastore,
        resource_pool,
        os_family,
    })
}

fn install_missing_template(
    cluster_config_path: &Path,
    install_config: &VSphereTemplateInstallConfig,
    expected_eksd_exact_tag: Option<&str>,
    metadata: &VSphereClusterMetadata,
    govc_env: &[(String, String)],
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    let ova_url =
        resolve_ova_url_for_template_with_logging(cluster_config_path, install_config.template_name.as_str(), logger)?;
    logger.info(format!(
        "📦 OVA source resolved for template `{}`.",
        install_config.template_name
    ));

    ensure_vsphere_folder_exists(install_config.folder_path.as_str(), govc_env)?;
    logger.info(format!("📥 Downloading OVA for template `{}`.", install_config.template_name));
    let downloaded_ova_path = download_ova_to_temp_file(
        ova_url.as_str(),
        install_config.template_name.as_str(),
        cluster_config_path,
        logger,
    )?;
    logger.info(format!("✅ OVA downloaded for template `{}`.", install_config.template_name));
    import_ova_as_vm_template(downloaded_ova_path.as_path(), install_config, metadata, govc_env, logger)?;
    apply_required_tags_on_template(install_config, expected_eksd_exact_tag, govc_env, logger)?;

    Ok(())
}

fn run_eksctl_list_ovas(
    cluster_config_path: &Path,
    mut stderr_handler: impl FnMut(&str),
) -> Result<Vec<String>, CommandError> {
    let config_path_str = cluster_config_path
        .to_str()
        .ok_or_else(|| CommandError::new_from_safe_message("Invalid cluster config path".to_string()))?;
    let args = ["anywhere", "list", "ovas", "-f", config_path_str];

    let mut cmd = QoveryCommand::new("eksctl", &args, &[]);
    let mut stdout = Vec::new();
    let mut stderr: Vec<String> = Vec::new();
    cmd.exec_with_abort(
        &mut |line| stdout.push(line),
        &mut |line| {
            stderr_handler(line.trim());
            stderr.push(line);
        },
        &CommandKiller::from_timeout(Duration::from_secs(90)),
    )
    .map_err(|e| {
        CommandError::new(
            "Cannot run `eksctl anywhere list ovas`".to_string(),
            Some(stderr_or_error(&stderr, e.to_string())),
            None,
        )
    })?;
    Ok(stdout)
}

fn find_ova_url_in_output(template_name: &str, stdout: &[String]) -> Result<String, CommandError> {
    let urls = extract_http_urls(stdout);
    if let Some(url) = select_ova_url_for_template(template_name, &urls) {
        return Ok(url);
    }
    Err(CommandError::new(
        format!("Cannot resolve OVA URL for template `{template_name}` from `eksctl anywhere list ovas` output"),
        Some(stdout.join("\n")),
        None,
    ))
}

fn resolve_ova_url_for_template(cluster_config_path: &Path, template_name: &str) -> Result<String, CommandError> {
    let stdout = run_eksctl_list_ovas(cluster_config_path, |_| {})?;
    find_ova_url_in_output(template_name, &stdout)
}

fn resolve_ova_url_for_template_with_logging(
    cluster_config_path: &Path,
    template_name: &str,
    logger: &impl InfraLogger,
) -> Result<String, CommandError> {
    logger.info("⏳ Resolving expected OVA from `eksctl anywhere list ovas`.");
    let stdout = run_eksctl_list_ovas(cluster_config_path, |trimmed| {
        if trimmed == "Command still running. No output available. Waiting for next line..." {
            logger.info("⏳ `eksctl anywhere list ovas` is still running...");
        } else if !trimmed.is_empty() {
            warn!("eksctl list ovas: {}", trimmed);
        }
    })?;
    find_ova_url_in_output(template_name, &stdout)
}

fn expected_ova_template_name_for_template(
    cluster_config_path: &Path,
    template_name: &str,
) -> Result<String, CommandError> {
    let ova_url = resolve_ova_url_for_template(cluster_config_path, template_name)?;
    template_name_from_ova_url(ova_url.as_str())
        .ok_or_else(|| CommandError::new(format!("Cannot extract template name from OVA URL `{ova_url}`"), None, None))
}

fn extract_http_urls(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .flat_map(|line| line.split_whitespace())
        .map(|token| token.trim_matches(|c: char| ",;()[]{}\"'".contains(c)))
        .filter(|token| token.starts_with("http://") || token.starts_with("https://"))
        .map(str::to_string)
        .collect()
}

fn template_name_from_ova_url(ova_url: &str) -> Option<String> {
    let parsed = Url::parse(ova_url).ok()?;
    let file_name = parsed.path_segments()?.next_back()?;
    file_name.strip_suffix(".ova").or(Some(file_name)).map(str::to_string)
}

fn select_ova_url_for_template(template_name: &str, urls: &[String]) -> Option<String> {
    if let Some(url) = urls
        .iter()
        .find(|u| template_name_from_ova_url(u).is_some_and(|name| name.eq_ignore_ascii_case(template_name)))
        .cloned()
    {
        return Some(url);
    }

    let lower_template_name = template_name.to_lowercase();
    if let Some(url) = urls
        .iter()
        .find(|u| u.to_lowercase().contains(&lower_template_name))
        .cloned()
    {
        return Some(url);
    }

    let template_minor = kubernetes_minor_token_from_template_name(template_name);
    let template_os = os_family_token_from_template_name(template_name);
    let mut compatible_urls: Vec<String> = urls
        .iter()
        .filter(|url| {
            let lower = url.to_lowercase();
            let minor_ok = template_minor
                .as_ref()
                .is_none_or(|minor| lower.contains(format!("/{minor}/").as_str()) || lower.contains(minor));
            let os_ok = template_os.as_ref().is_none_or(|os| lower.contains(os.as_str()));
            minor_ok && os_ok
        })
        .cloned()
        .collect();
    compatible_urls.sort();
    compatible_urls.dedup();

    if compatible_urls.len() == 1 {
        return compatible_urls.into_iter().next();
    }

    None
}

fn are_template_names_compatible(current_template_name: &str, expected_template_name: &str) -> bool {
    let current_os = os_family_token_from_template_name(current_template_name);
    let expected_os = os_family_token_from_template_name(expected_template_name);
    let os_compatible = match (current_os, expected_os) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    };

    let current_minor = kubernetes_minor_token_from_template_name(current_template_name);
    let expected_minor = kubernetes_minor_token_from_template_name(expected_template_name);
    let minor_compatible = match (current_minor, expected_minor) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    };

    os_compatible && minor_compatible
}

fn os_family_token_from_template_name(template_name: &str) -> Option<String> {
    let lower = template_name.to_lowercase();
    if lower.contains("bottlerocket") {
        return Some("bottlerocket".to_string());
    }
    if lower.contains("ubuntu") {
        return Some("ubuntu".to_string());
    }
    if lower.contains("redhat") || lower.contains("rhel") {
        return Some("redhat".to_string());
    }
    None
}

fn kubernetes_minor_token_from_template_name(template_name: &str) -> Option<String> {
    let lower = template_name.to_lowercase();
    if let Some(pos) = lower.find("k8s-")
        && let Some(minor) = parse_kubernetes_minor_token(&lower[(pos + 4)..])
    {
        return Some(minor);
    }
    if let Some(pos) = lower.find("-v")
        && let Some(minor) = parse_kubernetes_minor_token(&lower[(pos + 2)..])
    {
        return Some(minor);
    }
    None
}

fn parse_kubernetes_minor_token(input: &str) -> Option<String> {
    let mut chars = input.chars().peekable();
    let mut major = String::new();
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        major.push(chars.next()?);
    }
    let separator = chars.next()?;
    if separator != '.' && separator != '-' {
        return None;
    }
    let mut minor = String::new();
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        minor.push(chars.next()?);
    }
    if major.is_empty() || minor.is_empty() {
        return None;
    }
    Some(format!("{major}-{minor}"))
}

fn ensure_vsphere_folder_exists(folder_path: &str, govc_env: &[(String, String)]) -> Result<(), CommandError> {
    if run_govc_command(&["folder.create", folder_path], govc_env).is_ok() {
        return Ok(());
    }

    // Ignore already-existing folder.
    run_govc_command(&["folder.info", folder_path], govc_env).map(|_| ())
}

fn download_ova_to_temp_file(
    ova_url: &str,
    template_name: &str,
    cluster_config_path: &Path,
    logger: &impl InfraLogger,
) -> Result<PathBuf, CommandError> {
    let destination_dir = cluster_config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir)
        .join("eksa-ova-cache");
    fs::create_dir_all(&destination_dir).map_err(|e| {
        CommandError::new(
            format!(
                "Cannot create local directory for downloaded OVA files at {}",
                destination_dir.display()
            ),
            Some(e.to_string()),
            None,
        )
    })?;

    let destination_path = destination_dir.join(format!("{template_name}.ova"));
    let args = [
        "-fsSL",
        "--retry",
        "3",
        "--retry-delay",
        "2",
        "-o",
        destination_path
            .to_str()
            .ok_or_else(|| CommandError::new_from_safe_message("Invalid local OVA destination path".to_string()))?,
        ova_url,
    ];
    let mut cmd = QoveryCommand::new("curl", &args, &[]);
    let mut stderr = Vec::new();
    cmd.exec_with_abort(
        &mut |_| {},
        &mut |line| {
            let trimmed = line.trim();
            if trimmed == "Command still running. No output available. Waiting for next line..." {
                logger.info("⏳ OVA download is still running...");
            } else if !trimmed.is_empty() {
                warn!("OVA download: {}", trimmed);
            }
            stderr.push(line);
        },
        &CommandKiller::from_timeout(Duration::from_secs(1800)),
    )
    .map_err(|e| {
        CommandError::new(
            format!("Cannot download OVA from `{ova_url}`"),
            Some(stderr_or_error(&stderr, e.to_string())),
            None,
        )
    })?;

    Ok(destination_path)
}

fn import_ova_as_vm_template(
    downloaded_ova_path: &Path,
    install_config: &VSphereTemplateInstallConfig,
    metadata: &VSphereClusterMetadata,
    govc_env: &[(String, String)],
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    logger.info(format!(
        "🛠️ Importing OVA into vSphere as template `{}`.",
        install_config.template_name
    ));

    let mut import_args = vec![
        "import.ova",
        "-name",
        install_config.template_name.as_str(),
        "-ds",
        install_config.datastore.as_str(),
        "-folder",
        install_config.folder_path.as_str(),
        "-pool",
        install_config.resource_pool.as_str(),
    ];

    if let Some(network) = metadata.network.as_deref()
        && !network.trim().is_empty()
    {
        import_args.push("-net");
        import_args.push(network);
    }

    let ova_local_path = downloaded_ova_path
        .to_str()
        .ok_or_else(|| CommandError::new_from_safe_message("Invalid local OVA path".to_string()))?;
    import_args.push(ova_local_path);

    logger.info("⏳ `govc import.ova` can take several minutes depending on OVA size and vSphere load.");
    run_govc_command_with_timeout_logged(&import_args, govc_env, Duration::from_secs(1800), logger, "govc import.ova")?;
    run_govc_command(&["vm.markastemplate", install_config.template_path.as_str()], govc_env)?;
    logger.info(format!(
        "✅ Template `{}` imported and marked as template.",
        install_config.template_path
    ));
    Ok(())
}

fn apply_required_tags_on_template(
    install_config: &VSphereTemplateInstallConfig,
    expected_eksd_exact_tag: Option<&str>,
    govc_env: &[(String, String)],
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    logger.info(format!(
        "🏷️ Applying required tags on template `{}`.",
        install_config.template_path
    ));

    if let Some(expected_eksd_exact_tag) = expected_eksd_exact_tag {
        ensure_tag_and_attach(
            "eksdRelease",
            expected_eksd_exact_tag,
            install_config.template_path.as_str(),
            govc_env,
        )?;
    }

    if let Some(os_family) = install_config.os_family.as_deref() {
        ensure_tag_and_attach("os", os_family, install_config.template_path.as_str(), govc_env)?;
    }

    logger.info(format!(
        "✅ Required tags applied on template `{}`.",
        install_config.template_path
    ));
    Ok(())
}

fn ensure_tag_and_attach(
    category: &str,
    tag_value: &str,
    template_path: &str,
    govc_env: &[(String, String)],
) -> Result<(), CommandError> {
    let requested_tag_name = canonical_tag_name_for_category(category, tag_value);
    if requested_tag_name.is_empty() {
        return Err(CommandError::new_from_safe_message(format!(
            "Cannot ensure empty tag for category `{category}`"
        )));
    }
    let requested_tag_value = canonical_tag_value_for_category(category, requested_tag_name.as_str());

    let _ = run_govc_command(&["tags.category.create", "-t", "VirtualMachine", category], govc_env);
    let _ = run_govc_command(&["tags.create", "-c", category, requested_tag_name.as_str()], govc_env);

    let attached_tags = run_govc_command(&["tags.attached.ls", "-r", template_path], govc_env)?;
    let mut expected_tag_seen_in_recursive_listing = false;
    for attached_tag in attached_tags {
        let Some(attached_tag_value) = attached_tag_value_for_category(attached_tag.as_str(), category) else {
            continue;
        };

        if attached_tag_value.eq_ignore_ascii_case(requested_tag_value.as_str()) {
            expected_tag_seen_in_recursive_listing = true;
            continue;
        }

        let mut detached = false;
        for tag_candidate in tag_name_candidates_for_category(category, attached_tag_value.as_str()) {
            match run_govc_command(
                &["tags.detach", "-c", category, tag_candidate.as_str(), template_path],
                govc_env,
            ) {
                Ok(_) => {
                    detached = true;
                    break;
                }
                Err(detach_error)
                    if is_tag_not_found_in_category(&detach_error) || is_tag_name_not_found(&detach_error) =>
                {
                    continue;
                }
                Err(detach_error) => return Err(detach_error),
            }
        }
        if !detached {
            debug!(
                "Could not detach `{}` on `{}` with category `{}` using known tag-name variants",
                attached_tag_value, template_path, category
            );
        }
    }

    if expected_tag_seen_in_recursive_listing
        && is_tag_directly_attached_to_object(category, requested_tag_name.as_str(), template_path, govc_env)?
    {
        return Ok(());
    }

    if let Err(attach_error) = run_govc_command(
        &[
            "tags.attach",
            "-c",
            category,
            requested_tag_name.as_str(),
            template_path,
        ],
        govc_env,
    ) {
        if is_tag_already_attached(&attach_error) {
            return Ok(());
        }

        if is_tagging_cardinality_violation(&attach_error) {
            let recursive_attached_tags =
                run_govc_command(&["tags.attached.ls", "-r", template_path], govc_env).unwrap_or_default();
            let conflicting_tags = attached_tag_values_for_category(&recursive_attached_tags, category)
                .into_iter()
                .filter(|current| !current.eq_ignore_ascii_case(requested_tag_value.as_str()))
                .collect::<Vec<_>>();

            // Best effort: resolve inherited conflicts by detaching the old category tag from
            // ancestor objects that own it, then retry attaching the expected tag on the template.
            let mut detached_objects = Vec::new();
            for conflicting_tag in &conflicting_tags {
                detached_objects.extend(detach_conflicting_tag_from_template_ancestors(
                    category,
                    conflicting_tag,
                    template_path,
                    govc_env,
                )?);
            }

            match run_govc_command(
                &[
                    "tags.attach",
                    "-c",
                    category,
                    requested_tag_name.as_str(),
                    template_path,
                ],
                govc_env,
            ) {
                Ok(_) => return Ok(()),
                Err(retry_error) if is_tag_already_attached(&retry_error) => return Ok(()),
                Err(retry_error) => {
                    let conflicts_display = if conflicting_tags.is_empty() {
                        format!("category `{category}` already has a conflicting attached or inherited tag")
                    } else {
                        format!("conflicting tag(s): `{}`", conflicting_tags.join("`, `"))
                    };
                    let detached_display = if detached_objects.is_empty() {
                        "no ancestor object could be detached automatically".to_string()
                    } else {
                        format!("detached conflicting tags from: `{}`", detached_objects.join("`, `"))
                    };

                    return Err(CommandError::new(
                        format!(
                            "Cannot attach `{requested_tag_name}` to `{template_path}` due to vSphere tag cardinality. {conflicts_display}. \
Automatic remediation attempted ({detached_display}) but attach still failed."
                        ),
                        Some(retry_error.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                        None,
                    ));
                }
            }
        }

        return Err(attach_error);
    }

    Ok(())
}

fn detach_conflicting_tag_from_template_ancestors(
    category: &str,
    conflicting_tag: &str,
    template_path: &str,
    govc_env: &[(String, String)],
) -> Result<Vec<String>, CommandError> {
    let attached_objects = list_objects_attached_to_tag(category, conflicting_tag, govc_env)?;
    let mut detached_objects = Vec::new();

    for object_path in attached_objects {
        if !is_same_or_ancestor_inventory_path(template_path, object_path.as_str()) {
            continue;
        }

        let mut detached = false;
        for tag_candidate in tag_name_candidates_for_category(category, conflicting_tag) {
            match run_govc_command(
                &[
                    "tags.detach",
                    "-c",
                    category,
                    tag_candidate.as_str(),
                    object_path.as_str(),
                ],
                govc_env,
            ) {
                Ok(_) => {
                    detached = true;
                    break;
                }
                Err(detach_error)
                    if is_tag_not_found_in_category(&detach_error) || is_tag_name_not_found(&detach_error) =>
                {
                    continue;
                }
                Err(detach_error) => return Err(detach_error),
            }
        }
        if detached {
            detached_objects.push(object_path);
        }
    }

    if detached_objects.is_empty() {
        // Fallback for older/inconsistent tagging APIs: try detaching on each ancestor path.
        for candidate in inventory_path_ancestors(template_path) {
            let mut detached = false;
            for tag_candidate in tag_name_candidates_for_category(category, conflicting_tag) {
                match run_govc_command(
                    &[
                        "tags.detach",
                        "-c",
                        category,
                        tag_candidate.as_str(),
                        candidate.as_str(),
                    ],
                    govc_env,
                ) {
                    Ok(_) => {
                        detached = true;
                        break;
                    }
                    Err(detach_error)
                        if is_tag_not_found_in_category(&detach_error) || is_tag_name_not_found(&detach_error) =>
                    {
                        continue;
                    }
                    Err(detach_error) => return Err(detach_error),
                }
            }
            if detached {
                detached_objects.push(candidate);
            }
        }
        detached_objects.sort();
        detached_objects.dedup();
    }

    Ok(detached_objects)
}

fn list_objects_attached_to_tag(
    category: &str,
    tag_value: &str,
    govc_env: &[(String, String)],
) -> Result<Vec<String>, CommandError> {
    Ok(normalize_inventory_object_paths(&list_attached_entries_for_tag(
        category, tag_value, govc_env,
    )?))
}

fn list_attached_entries_for_tag(
    category: &str,
    tag_value: &str,
    govc_env: &[(String, String)],
) -> Result<Vec<String>, CommandError> {
    let tag_candidates = tag_name_candidates_for_category(category, tag_value);
    let mut last_error = None;
    let mut supports_attached_ls_with_category = true;
    debug!(
        "Resolving attached objects for tag category=`{}` value=`{}` candidates={:?}",
        category, tag_value, tag_candidates
    );

    for candidate in &tag_candidates {
        match run_govc_command(&["tags.attached.ls", "-c", category, candidate.as_str()], govc_env) {
            Ok(output) => {
                debug!(
                    "Resolved attached objects with `tags.attached.ls -c`: candidate=`{}` output={:?}",
                    candidate, output
                );
                return Ok(output);
            }
            Err(err) if is_tag_not_found_in_category(&err) || is_tag_name_not_found(&err) => {
                last_error = Some(err);
                continue;
            }
            Err(err) if is_flag_not_defined_error(&err) => {
                // Older govc versions do not support `tags.attached.ls -c`.
                supports_attached_ls_with_category = false;
                last_error = Some(err);
                break;
            }
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        }
    }

    let mut identifiers = tag_candidates.clone();
    for candidate in tag_candidates {
        if let Some(tag_id) = resolve_tag_id_for_category(category, candidate.as_str(), govc_env)?
            && !identifiers
                .iter()
                .any(|identifier| identifier.eq_ignore_ascii_case(tag_id.as_str()))
        {
            identifiers.push(tag_id);
        }
    }

    for identifier in identifiers {
        match run_govc_command(&["tags.attached.ls", identifier.as_str()], govc_env) {
            Ok(output) => {
                debug!(
                    "Resolved attached objects with `tags.attached.ls` identifier=`{}` output={:?}",
                    identifier, output
                );
                return Ok(output);
            }
            Err(err) if is_tag_identifier_not_found_error(&err) => {
                last_error = Some(err);
                continue;
            }
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        }
    }

    if let Some(err) = last_error {
        if (supports_attached_ls_with_category && (is_tag_not_found_in_category(&err) || is_tag_name_not_found(&err)))
            || is_tag_identifier_not_found_error(&err)
            || is_tag_not_found_in_category(&err)
            || is_tag_name_not_found(&err)
        {
            return Ok(Vec::new());
        }

        return Err(err);
    }

    Ok(Vec::new())
}

fn attached_tag_value_for_category(attached_tag: &str, category: &str) -> Option<String> {
    let trimmed = attached_tag.trim();
    if trimmed.is_empty() {
        return None;
    }

    for separator in ['/', ':'] {
        if let Some((left, right)) = trimmed.split_once(separator) {
            let left = left.trim();
            if left.eq_ignore_ascii_case(category) {
                return normalize_attached_tag_value(right);
            }
        }
    }

    None
}

fn normalize_attached_tag_value(raw_value: &str) -> Option<String> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return None;
    }

    // `govc tags.attached.ls -r` may append inherited/source metadata after the tag value.
    let without_annotation = trimmed.split_once(" (").map_or(trimmed, |(tag, _)| tag.trim());
    let first_token = without_annotation.split_whitespace().next().unwrap_or("").trim();
    let normalized = first_token.trim_matches(['`', '"']).trim();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn attached_tag_values_for_category(attached_tags: &[String], category: &str) -> Vec<String> {
    let mut values = attached_tags
        .iter()
        .flat_map(|tag| attached_tag_candidates_for_category(tag.as_str(), category))
        .collect::<Vec<_>>();
    values.sort();
    values.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    values
}

fn attached_tag_candidates_for_category(attached_tag: &str, category: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(qualified_value) = attached_tag_value_for_category(attached_tag, category) {
        candidates.push(qualified_value);
    }

    if let Some(raw_name) = normalize_attached_tag_value(attached_tag) {
        if is_category_prefixed_tag_name(category, raw_name.as_str()) {
            if let Some((_, raw_value)) = raw_name.split_once(':')
                && let Some(unqualified_value) = normalize_attached_tag_value(raw_value)
            {
                candidates.push(unqualified_value);
            }
        } else if !raw_name.contains(':') && !raw_name.contains('/') {
            // Some govc/vCenter combinations return only the tag name without category prefix.
            candidates.push(raw_name);
        }
    }

    candidates.sort();
    candidates.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    candidates
}

fn normalize_inventory_object_path(raw_line: &str) -> Option<String> {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let start_index = trimmed.find('/')?;
    let path_candidate = trimmed[start_index..]
        .split_once(" (")
        .map_or(&trimmed[start_index..], |(path, _)| path)
        .trim();
    let normalized = path_candidate.trim_matches(['`', '"']);

    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn normalize_inventory_object_paths(output_lines: &[String]) -> Vec<String> {
    output_lines
        .iter()
        .filter_map(|line| normalize_inventory_object_path(line.as_str()))
        .collect::<Vec<_>>()
}

fn is_same_or_ancestor_inventory_path(descendant_path: &str, ancestor_path: &str) -> bool {
    let descendant = descendant_path.trim_end_matches('/');
    let ancestor = ancestor_path.trim_end_matches('/');

    descendant == ancestor || descendant.starts_with(format!("{ancestor}/").as_str())
}

fn is_same_inventory_path(left: &str, right: &str) -> bool {
    left.trim_end_matches('/')
        .eq_ignore_ascii_case(right.trim_end_matches('/'))
}

fn is_tag_directly_attached_to_object(
    category: &str,
    tag_value: &str,
    object_path: &str,
    govc_env: &[(String, String)],
) -> Result<bool, CommandError> {
    let attached_entries = list_attached_entries_for_tag(category, tag_value, govc_env)?;
    let vm_moid = vm_moid_for_template(object_path, govc_env)?;
    let is_attached = attached_entries.iter().any(|entry| {
        normalize_inventory_object_path(entry.as_str())
            .is_some_and(|path| is_same_inventory_path(path.as_str(), object_path))
            || vm_moid
                .as_deref()
                .is_some_and(|moid| is_dynamic_vm_reference_to_moid(entry.as_str(), moid))
    });
    debug!(
        "Direct tag attachment check: category=`{}` tag=`{}` object=`{}` vm_moid={:?} attached_entries={:?} result={}",
        category, tag_value, object_path, vm_moid, attached_entries, is_attached
    );
    Ok(is_attached)
}

fn vm_moid_for_template(template_path: &str, govc_env: &[(String, String)]) -> Result<Option<String>, CommandError> {
    let vm_info_output = run_govc_command(&["vm.info", "-json", template_path], govc_env)?;
    let vm_info_json = vm_info_output.join("\n");
    let vm_info: JsonValue = serde_json::from_str(vm_info_json.as_str()).map_err(|e| {
        CommandError::new(
            format!("Cannot parse `govc vm.info -json` output for `{template_path}`"),
            Some(e.to_string()),
            None,
        )
    })?;
    let structured_moid = vm_info
        .get("VirtualMachines")
        .and_then(JsonValue::as_array)
        .and_then(|vms| vms.first())
        .and_then(|vm| vm.get("Self"))
        .and_then(|self_ref| self_ref.get("Value"))
        .and_then(JsonValue::as_str)
        .and_then(normalize_vm_moid);
    let fallback_moid = extract_vm_moid_from_vm_info_json(vm_info_json.as_str());
    let final_moid = structured_moid.or(fallback_moid);
    debug!("Resolved VM MoID for template `{}`: {:?}", template_path, final_moid);
    Ok(final_moid)
}

fn normalize_vm_moid(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = trimmed.split(':').next().unwrap_or(trimmed).trim();
    if candidate.starts_with("vm-") {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn extract_vm_moid_from_vm_info_json(vm_info_json: &str) -> Option<String> {
    for (idx, ch) in vm_info_json.char_indices() {
        if ch != 'v' {
            continue;
        }

        let remaining = &vm_info_json[idx..];
        if !remaining.starts_with("vm-") {
            continue;
        }

        let mut end = idx + 3;
        let mut has_digit = false;
        for (offset, c) in vm_info_json[(idx + 3)..].char_indices() {
            if c.is_ascii_digit() {
                has_digit = true;
                end = idx + 3 + offset + c.len_utf8();
                continue;
            }
            break;
        }

        if has_digit {
            return Some(vm_info_json[idx..end].to_string());
        }
    }

    None
}

fn is_dynamic_vm_reference_to_moid(entry: &str, moid: &str) -> bool {
    let trimmed = entry.trim();
    if trimmed.is_empty() || moid.trim().is_empty() {
        return false;
    }

    let lower_entry = trimmed.to_ascii_lowercase();
    let lower_moid = moid.trim().to_ascii_lowercase();

    let dynamic_prefix = format!("virtualmachine:{lower_moid}");
    if lower_entry == dynamic_prefix || lower_entry.starts_with(format!("{dynamic_prefix}:").as_str()) {
        return true;
    }

    lower_entry.contains(format!("id = {lower_moid}").as_str())
        || lower_entry.contains(format!("id={lower_moid}").as_str())
        || lower_entry.contains(format!("id = {lower_moid}:").as_str())
        || lower_entry.contains(format!("id={lower_moid}:").as_str())
}

fn inventory_path_ancestors(path: &str) -> Vec<String> {
    let mut normalized = path.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() || !normalized.starts_with('/') {
        return Vec::new();
    }

    let mut ancestors = Vec::new();
    loop {
        ancestors.push(normalized.clone());
        if let Some((parent, _)) = normalized.rsplit_once('/') {
            if parent.is_empty() {
                break;
            }
            normalized = parent.to_string();
        } else {
            break;
        }
    }

    ancestors
}

fn is_category_prefixed_tag_name(category: &str, tag_name: &str) -> bool {
    let Some((prefix, _)) = tag_name.split_once(':') else {
        return false;
    };
    prefix.trim().eq_ignore_ascii_case(category)
}

fn category_requires_prefixed_tag_name(category: &str) -> bool {
    category.eq_ignore_ascii_case("eksdRelease")
}

fn canonical_tag_name_for_category(category: &str, tag_value: &str) -> String {
    let normalized_value = tag_value.trim();
    if normalized_value.is_empty() {
        return String::new();
    }

    if category_requires_prefixed_tag_name(category) && !is_category_prefixed_tag_name(category, normalized_value) {
        format!("{category}:{normalized_value}")
    } else {
        normalized_value.to_string()
    }
}

fn canonical_tag_value_for_category(category: &str, tag_value: &str) -> String {
    attached_tag_value_for_category(tag_value, category).unwrap_or_else(|| tag_value.trim().to_string())
}

fn tag_name_candidates_for_category(category: &str, tag_value: &str) -> Vec<String> {
    let normalized_value = tag_value.trim();
    if normalized_value.is_empty() {
        return Vec::new();
    }

    let mut candidates = vec![normalized_value.to_string()];
    if let Some(unqualified_value) = attached_tag_value_for_category(normalized_value, category) {
        candidates.push(unqualified_value.clone());
        candidates.push(format!("{category}:{unqualified_value}"));
    }
    if !is_category_prefixed_tag_name(category, normalized_value) {
        candidates.push(format!("{category}:{normalized_value}"));
    }

    candidates.sort();
    candidates.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    candidates
}

fn is_tag_not_found_in_category(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("tag") && lower_error.contains("not found in category")
}

fn is_tag_name_not_found(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("tag \"") && lower_error.contains("\" not found")
}

fn is_flag_not_defined_error(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("flag provided but not defined")
        || lower_error.contains("unknown flag")
        || lower_error.contains("unknown shorthand flag")
}

fn is_tag_identifier_not_found_error(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    (lower_error.contains("/rest/com/vmware/cis/tagging/tag/id:")
        || lower_error.contains("inventoryservicetag")
        || lower_error.contains("tag/id:"))
        && lower_error.contains("404")
}

fn is_tagging_cardinality_violation(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("tagging cardinality violation")
        || (lower_error.contains("cardinality") && lower_error.contains("violation"))
}

fn is_tag_already_attached(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("already attached")
}

fn resolve_tag_id_for_category(
    category: &str,
    tag_value: &str,
    govc_env: &[(String, String)],
) -> Result<Option<String>, CommandError> {
    let output = match run_govc_command(&["tags.info", "-c", category, tag_value], govc_env) {
        Ok(output) => output,
        Err(err)
            if is_tag_not_found_in_category(&err)
                || is_tag_name_not_found(&err)
                || is_tag_identifier_not_found_error(&err) =>
        {
            return Ok(None);
        }
        Err(err) => return Err(err),
    };

    Ok(parse_tag_id_from_tags_info_output(&output))
}

fn parse_tag_id_from_tags_info_output(output_lines: &[String]) -> Option<String> {
    output_lines.iter().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if !key.trim().eq_ignore_ascii_case("id") {
            return None;
        }

        let id = value.trim();
        if id.is_empty() { None } else { Some(id.to_string()) }
    })
}

fn stderr_or_error(stderr: &[String], fallback: String) -> String {
    if stderr.is_empty() { fallback } else { stderr.join("\n") }
}

fn backtick(s: &str) -> String {
    format!("`{s}`")
}

fn template_name_from_path(template_path: &str) -> Option<String> {
    Path::new(template_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
}

fn template_label(template_path: &str) -> String {
    template_name_from_path(template_path).unwrap_or_else(|| template_path.to_string())
}

fn template_folder_from_path(template_path: &str) -> Option<String> {
    Path::new(template_path)
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
}

fn run_govc_command(args: &[&str], govc_env: &[(String, String)]) -> Result<Vec<String>, CommandError> {
    run_govc_command_with_timeout(args, govc_env, Duration::from_secs(45))
}

fn run_govc_command_impl(
    args: &[&str],
    govc_env: &[(String, String)],
    timeout: Duration,
    mut stdout_handler: impl FnMut(&str),
    mut stderr_handler: impl FnMut(&str),
) -> Result<Vec<String>, CommandError> {
    let env_pairs: Vec<(&str, &str)> = govc_env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut cmd = QoveryCommand::new("govc", args, &env_pairs);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    cmd.exec_with_abort(
        &mut |line| {
            stdout_handler(line.trim());
            stdout.push(line);
        },
        &mut |line| {
            stderr_handler(line.trim());
            stderr.push(line);
        },
        &CommandKiller::from_timeout(timeout),
    )
    .map_err(|e| {
        CommandError::new(
            format!("Cannot run `govc {}`", args.join(" ")),
            Some(stderr_or_error(&stderr, e.to_string())),
            None,
        )
    })?;

    Ok(stdout
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

fn run_govc_command_with_timeout_logged(
    args: &[&str],
    govc_env: &[(String, String)],
    timeout: Duration,
    logger: &impl InfraLogger,
    label: &str,
) -> Result<Vec<String>, CommandError> {
    logger.info(format!("{COMMAND_STDOUT_PREFIX}▶️ Running `{label}`."));

    let result = run_govc_command_impl(
        args,
        govc_env,
        timeout,
        |trimmed| {
            if !trimmed.is_empty() {
                logger.info(format!("{COMMAND_STDOUT_PREFIX}{trimmed}"));
            }
        },
        |trimmed| {
            if trimmed == "Command still running. No output available. Waiting for next line..." {
                logger.info(format!("{COMMAND_STDOUT_PREFIX}⏳ `{label}` is still running..."));
            } else if !trimmed.is_empty() {
                logger.warn(format!("{COMMAND_STDERR_PREFIX}{trimmed}"));
            }
        },
    );

    match result {
        Ok(lines) => {
            logger.info(format!("{COMMAND_STDOUT_PREFIX}✅ `{label}` completed."));
            Ok(lines)
        }
        Err(error) => {
            logger.warn(format!("{COMMAND_STDERR_PREFIX}❌ `{label}` failed."));
            Err(error)
        }
    }
}

fn run_govc_command_with_timeout(
    args: &[&str],
    govc_env: &[(String, String)],
    timeout: Duration,
) -> Result<Vec<String>, CommandError> {
    run_govc_command_impl(args, govc_env, timeout, |_| {}, |_| {})
}

fn has_expected_eksd_release_tag(attached_tags: &[String], expected_fragment: Option<&str>) -> bool {
    attached_tag_values_for_category(attached_tags, "eksdRelease")
        .iter()
        .any(|tag_value| match expected_fragment {
            Some(fragment) => tag_value.to_lowercase().contains(fragment),
            None => true,
        })
}

fn has_expected_os_tag(attached_tags: &[String], expected_os_family: &str) -> bool {
    let expected_os_family = expected_os_family.to_lowercase();

    attached_tag_values_for_category(attached_tags, "os")
        .iter()
        .any(|tag_value| tag_value.to_lowercase().contains(&expected_os_family))
}

fn expected_eksd_release_fragment_from_kubernetes_version(kubernetes_version: &str) -> Option<String> {
    let version = kubernetes_version.trim().trim_start_matches('v');
    let mut parts = version.split('.');
    let major = parts.next()?;
    let minor_with_suffix = parts.next()?;

    let minor = minor_with_suffix
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();

    if major.is_empty() || minor.is_empty() {
        return None;
    }

    Some(format!("kubernetes-{major}-{minor}"))
}

fn summarize_vsphere_templates_for_user(templates: &[VSphereTemplateRef], cluster_config_name: String) -> Vec<String> {
    let mut templates_by_image: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut missing_templates: Vec<String> = Vec::new();

    for template_ref in templates {
        match &template_ref.template {
            Some(template) => templates_by_image
                .entry(template.clone())
                .or_default()
                .push(template_ref.machine_config_name.clone()),
            None => missing_templates.push(template_ref.machine_config_name.clone()),
        }
    }

    let unique_template_count = templates_by_image.len();
    let mut lines: Vec<String> = vec![format!(
        "🖼️ vSphere machine configs in `{}`: {}",
        cluster_config_name,
        templates
            .iter()
            .map(|t| backtick(&t.machine_config_name))
            .collect::<Vec<_>>()
            .join(", ")
    )];

    if unique_template_count == 0 {
        lines.push("🖼️ vSphere templates: none.".to_string());
    } else {
        lines.push(format!(
            "🖼️ vSphere templates ({} unique): {}",
            unique_template_count,
            templates_by_image
                .keys()
                .map(|template| backtick(&template_label(template)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    for (template, mut machine_configs) in templates_by_image {
        machine_configs.sort();
        lines.push(format!(
            "🧩 Template usage: `{}` <- [{}]",
            template_label(template.as_str()),
            machine_configs
                .iter()
                .map(|name| backtick(name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        info!("vSphere template full path: {}", template);
    }

    if !missing_templates.is_empty() {
        missing_templates.sort();
        lines.push(format!(
            "⚠️ Missing `spec.template` for [{}]",
            missing_templates
                .iter()
                .map(|name| backtick(name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    lines
}

fn extract_vsphere_cluster_metadata_from_yaml(content: &str) -> Result<VSphereClusterMetadata, CommandError> {
    let mut metadata = VSphereClusterMetadata::default();

    for yaml_doc in serde_yaml::Deserializer::from_str(content) {
        let value = Value::deserialize(yaml_doc).map_err(|e| {
            CommandError::new(
                "Cannot parse EKS Anywhere cluster YAML while inspecting vSphere metadata".to_string(),
                Some(e.to_string()),
                None,
            )
        })?;

        match value.get("kind").and_then(Value::as_str) {
            Some("Cluster") => {
                metadata.kubernetes_version = value
                    .get("spec")
                    .and_then(|s| s.get("kubernetesVersion"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            Some("VSphereDatacenterConfig") => {
                metadata.vcenter_server = value
                    .get("spec")
                    .and_then(|s| s.get("server"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                metadata.insecure = value
                    .get("spec")
                    .and_then(|s| s.get("insecure"))
                    .and_then(Value::as_bool);
                metadata.network = value
                    .get("spec")
                    .and_then(|s| s.get("network"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            _ => {}
        }
    }

    Ok(metadata)
}

fn extract_vsphere_templates_from_yaml(content: &str) -> Result<Vec<VSphereTemplateRef>, CommandError> {
    let mut templates = Vec::new();

    for yaml_doc in serde_yaml::Deserializer::from_str(content) {
        let value = Value::deserialize(yaml_doc).map_err(|e| {
            CommandError::new(
                "Cannot parse EKS Anywhere cluster YAML while inspecting vSphere templates".to_string(),
                Some(e.to_string()),
                None,
            )
        })?;

        if value.get("kind").and_then(Value::as_str) != Some("VSphereMachineConfig") {
            continue;
        }

        let machine_config_name = value
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("<unknown>")
            .to_string();
        let template = value
            .get("spec")
            .and_then(|s| s.get("template"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let os_family = value
            .get("spec")
            .and_then(|s| s.get("osFamily"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let datastore = value
            .get("spec")
            .and_then(|s| s.get("datastore"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let resource_pool = value
            .get("spec")
            .and_then(|s| s.get("resourcePool"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let folder = value
            .get("spec")
            .and_then(|s| s.get("folder"))
            .and_then(Value::as_str)
            .map(str::to_string);

        templates.push(VSphereTemplateRef {
            machine_config_name,
            template,
            os_family,
            datastore,
            resource_pool,
            folder,
        });
    }

    Ok(templates)
}

#[cfg(test)]
mod tests {
    use super::{
        VSphereClusterMetadata, VSphereTemplateRef, attached_tag_value_for_category, attached_tag_values_for_category,
        expected_eksd_release_fragment_from_kubernetes_version, extract_vm_moid_from_vm_info_json,
        extract_vsphere_cluster_metadata_from_yaml, extract_vsphere_templates_from_yaml, has_exact_eksd_release_tag,
        has_expected_eksd_release_tag, has_expected_os_tag, is_category_prefixed_tag_name, is_flag_not_defined_error,
        is_same_or_ancestor_inventory_path, is_tag_already_attached, is_tag_identifier_not_found_error,
        is_tag_name_not_found, is_tag_not_found_in_category, is_tagging_cardinality_violation,
        normalize_inventory_object_path, normalize_vm_moid, parse_tag_id_from_tags_info_output,
        summarize_vsphere_templates_for_user, tag_name_candidates_for_category, validate_govc_auth_envs,
    };
    use crate::errors::CommandError;

    #[test]
    fn should_extract_vsphere_templates_from_multi_document_yaml() {
        let yaml = r#"
kind: VSphereMachineConfig
metadata:
  name: cp-machine
spec:
  osFamily: bottlerocket
  template: bottlerocket-vmware-k8s-1.32-x86_64-1.51.0-47438798
---
kind: Cluster
metadata:
  name: ignored
---
kind: VSphereMachineConfig
metadata:
  name: worker-machine
spec:
  osFamily: bottlerocket
  template: bottlerocket-vmware-k8s-1.32-x86_64-1.51.0-47438798
"#;

        let refs = extract_vsphere_templates_from_yaml(yaml).expect("YAML should parse");
        assert_eq!(
            refs,
            vec![
                VSphereTemplateRef {
                    machine_config_name: "cp-machine".to_string(),
                    template: Some("bottlerocket-vmware-k8s-1.32-x86_64-1.51.0-47438798".to_string()),
                    os_family: Some("bottlerocket".to_string()),
                    datastore: None,
                    resource_pool: None,
                    folder: None,
                },
                VSphereTemplateRef {
                    machine_config_name: "worker-machine".to_string(),
                    template: Some("bottlerocket-vmware-k8s-1.32-x86_64-1.51.0-47438798".to_string()),
                    os_family: Some("bottlerocket".to_string()),
                    datastore: None,
                    resource_pool: None,
                    folder: None,
                }
            ]
        );
    }

    #[test]
    fn should_extract_machine_config_even_when_template_is_missing() {
        let yaml = r#"
kind: VSphereMachineConfig
metadata:
  name: cp-machine
spec: {}
"#;

        let refs = extract_vsphere_templates_from_yaml(yaml).expect("YAML should parse");
        assert_eq!(
            refs,
            vec![VSphereTemplateRef {
                machine_config_name: "cp-machine".to_string(),
                template: None,
                os_family: None,
                datastore: None,
                resource_pool: None,
                folder: None,
            }]
        );
    }

    #[test]
    fn should_summarize_shared_template_in_one_line() {
        let summary = summarize_vsphere_templates_for_user(
            &[
                VSphereTemplateRef {
                    machine_config_name: "eksa-powens-controlplane".to_string(),
                    template: Some("/dc/vm/Templates/template-a".to_string()),
                    os_family: Some("bottlerocket".to_string()),
                    datastore: None,
                    resource_pool: None,
                    folder: None,
                },
                VSphereTemplateRef {
                    machine_config_name: "eksa-powens-nodes".to_string(),
                    template: Some("/dc/vm/Templates/template-a".to_string()),
                    os_family: Some("bottlerocket".to_string()),
                    datastore: None,
                    resource_pool: None,
                    folder: None,
                },
                VSphereTemplateRef {
                    machine_config_name: "eksa-powens-etcd".to_string(),
                    template: Some("/dc/vm/Templates/template-a".to_string()),
                    os_family: Some("bottlerocket".to_string()),
                    datastore: None,
                    resource_pool: None,
                    folder: None,
                },
            ],
            "cluster-eksa-powens.yaml".to_string(),
        );

        assert_eq!(
            summary,
            vec![
                "🖼️ vSphere machine configs in `cluster-eksa-powens.yaml`: `eksa-powens-controlplane`, `eksa-powens-nodes`, `eksa-powens-etcd`".to_string(),
                "🖼️ vSphere templates (1 unique): `template-a`".to_string(),
                "🧩 Template usage: `template-a` <- [`eksa-powens-controlplane`, `eksa-powens-etcd`, `eksa-powens-nodes`]".to_string(),
            ]
        );
    }

    #[test]
    fn should_extract_cluster_metadata() {
        let yaml = r#"
kind: Cluster
spec:
  kubernetesVersion: "1.32"
---
kind: VSphereDatacenterConfig
spec:
  server: "pcc.example.local"
  insecure: true
  network: "my-network"
"#;

        let metadata = extract_vsphere_cluster_metadata_from_yaml(yaml).expect("YAML should parse");
        assert_eq!(
            metadata,
            VSphereClusterMetadata {
                kubernetes_version: Some("1.32".to_string()),
                vcenter_server: Some("pcc.example.local".to_string()),
                insecure: Some(true),
                network: Some("my-network".to_string()),
            }
        );
    }

    #[test]
    fn should_build_expected_eksd_release_fragment_from_kubernetes_version() {
        assert_eq!(
            expected_eksd_release_fragment_from_kubernetes_version("1.32"),
            Some("kubernetes-1-32".to_string())
        );
        assert_eq!(
            expected_eksd_release_fragment_from_kubernetes_version("v1.29.4"),
            Some("kubernetes-1-29".to_string())
        );
    }

    #[test]
    fn should_detect_expected_tags() {
        let tags = vec![
            "os/bottlerocket".to_string(),
            "eksdRelease/kubernetes-1-32-eks-1-32-29".to_string(),
        ];

        assert!(has_expected_os_tag(&tags, "bottlerocket"));
        assert!(has_expected_eksd_release_tag(&tags, Some("kubernetes-1-32")));
    }

    #[test]
    fn should_detect_expected_tags_when_attached_output_is_unqualified() {
        let tags = vec!["bottlerocket".to_string(), "kubernetes-1-32-eks-33".to_string()];

        assert!(has_expected_os_tag(&tags, "bottlerocket"));
        assert!(has_expected_eksd_release_tag(&tags, Some("kubernetes-1-32")));
        assert!(has_exact_eksd_release_tag(&tags, "kubernetes-1-32-eks-33"));
    }

    #[test]
    fn should_extract_attached_tag_value_from_slash_format() {
        let value = attached_tag_value_for_category("eksdRelease/kubernetes-1-32-eks-33", "eksdRelease");
        assert_eq!(value.as_deref(), Some("kubernetes-1-32-eks-33"));
    }

    #[test]
    fn should_extract_attached_tag_value_from_colon_format() {
        let value = attached_tag_value_for_category("os:bottlerocket", "os");
        assert_eq!(value.as_deref(), Some("bottlerocket"));
    }

    #[test]
    fn should_extract_attached_tag_value_when_line_contains_inherited_annotation() {
        let value = attached_tag_value_for_category(
            "eksdRelease/kubernetes-1-32-eks-29 (inherited from /dc/vm/Templates)",
            "eksdRelease",
        );
        assert_eq!(value.as_deref(), Some("kubernetes-1-32-eks-29"));
    }

    #[test]
    fn should_ignore_other_categories_when_extracting_attached_tag_value() {
        let value = attached_tag_value_for_category("other/something", "eksdRelease");
        assert!(value.is_none());
    }

    #[test]
    fn should_extract_and_dedup_attached_tag_values_for_category() {
        let values = attached_tag_values_for_category(
            &[
                "eksdRelease/kubernetes-1-32-eks-29".to_string(),
                "eksdRelease:kubernetes-1-32-eks-29".to_string(),
                "eksdRelease/kubernetes-1-32-eks-33".to_string(),
                "os/bottlerocket".to_string(),
            ],
            "eksdRelease",
        );

        assert_eq!(
            values,
            vec![
                "kubernetes-1-32-eks-29".to_string(),
                "kubernetes-1-32-eks-33".to_string()
            ]
        );
    }

    #[test]
    fn should_extract_unqualified_attached_tag_values_for_category() {
        let values = attached_tag_values_for_category(
            &["kubernetes-1-32-eks-33".to_string(), "os:bottlerocket".to_string()],
            "eksdRelease",
        );

        assert_eq!(values, vec!["kubernetes-1-32-eks-33".to_string()]);
    }

    #[test]
    fn should_normalize_inventory_object_path_from_plain_line() {
        let normalized = normalize_inventory_object_path("/dc/vm/Templates/template-a");
        assert_eq!(normalized.as_deref(), Some("/dc/vm/Templates/template-a"));
    }

    #[test]
    fn should_normalize_inventory_object_path_from_annotated_line() {
        let normalized = normalize_inventory_object_path(
            "VirtualMachine /dc/vm/Templates/template-a (inherited from /dc/vm/Templates)",
        );
        assert_eq!(normalized.as_deref(), Some("/dc/vm/Templates/template-a"));
    }

    #[test]
    fn should_detect_same_or_ancestor_inventory_path() {
        assert!(is_same_or_ancestor_inventory_path(
            "/dc/vm/Templates/template-a",
            "/dc/vm/Templates"
        ));
        assert!(is_same_or_ancestor_inventory_path(
            "/dc/vm/Templates/template-a",
            "/dc/vm/Templates/template-a"
        ));
        assert!(!is_same_or_ancestor_inventory_path(
            "/dc/vm/Templates/template-a",
            "/dc/vm/OtherFolder"
        ));
    }

    #[test]
    fn should_detect_tag_not_found_in_category_error() {
        let err = CommandError::new(
            "Cannot run govc".to_string(),
            Some("govc: tag \"abc\" not found in category \"eksdRelease\"".to_string()),
            None,
        );
        assert!(is_tag_not_found_in_category(&err));
    }

    #[test]
    fn should_detect_flag_not_defined_error() {
        let err = CommandError::new(
            "Cannot run govc".to_string(),
            Some("govc: flag provided but not defined: -c".to_string()),
            None,
        );
        assert!(is_flag_not_defined_error(&err));
    }

    #[test]
    fn should_detect_tag_identifier_not_found_error() {
        let err = CommandError::new(
            "Cannot run govc".to_string(),
            Some("GET ... /rest/com/vmware/cis/tagging/tag/id:kubernetes-1-32-eks-29: 404 Not Found".to_string()),
            None,
        );
        assert!(is_tag_identifier_not_found_error(&err));
    }

    #[test]
    fn should_detect_tag_name_not_found_error() {
        let err = CommandError::new(
            "Cannot run govc".to_string(),
            Some("govc: tag \"kubernetes-1-32-eks-29\" not found".to_string()),
            None,
        );
        assert!(is_tag_name_not_found(&err));
    }

    #[test]
    fn should_detect_category_prefixed_tag_name() {
        assert!(is_category_prefixed_tag_name(
            "eksdRelease",
            "eksdRelease:kubernetes-1-32-eks-29"
        ));
        assert!(!is_category_prefixed_tag_name("eksdRelease", "kubernetes-1-32-eks-29"));
    }

    #[test]
    fn should_build_tag_name_candidates_for_prefixed_and_unprefixed_values() {
        assert_eq!(
            tag_name_candidates_for_category("eksdRelease", "kubernetes-1-32-eks-29"),
            vec![
                "eksdRelease:kubernetes-1-32-eks-29".to_string(),
                "kubernetes-1-32-eks-29".to_string()
            ]
        );
        assert_eq!(
            tag_name_candidates_for_category("eksdRelease", "eksdRelease:kubernetes-1-32-eks-29"),
            vec![
                "eksdRelease:kubernetes-1-32-eks-29".to_string(),
                "kubernetes-1-32-eks-29".to_string()
            ]
        );
    }

    #[test]
    fn should_extract_vm_moid_from_vm_info_json_fallback() {
        let moid = extract_vm_moid_from_vm_info_json(
            r#"{
  "VirtualMachines": [
    {
      "Self": {
        "Type": "VirtualMachine",
        "Value": "vm-18578:f6e479e9-33ff-4ea0-b55f-96f3fa7929c9"
      }
    }
  ]
}"#,
        );

        assert_eq!(moid.as_deref(), Some("vm-18578"));
    }

    #[test]
    fn should_normalize_vm_moid_with_suffix() {
        assert_eq!(
            normalize_vm_moid("vm-18578:f6e479e9-33ff-4ea0-b55f-96f3fa7929c9").as_deref(),
            Some("vm-18578")
        );
        assert_eq!(normalize_vm_moid("vm-42").as_deref(), Some("vm-42"));
        assert!(normalize_vm_moid("host-123").is_none());
    }

    #[test]
    fn should_parse_tag_id_from_tags_info_output() {
        let id = parse_tag_id_from_tags_info_output(&[
            "Name: kubernetes-1-32-eks-29".to_string(),
            "Category: eksdRelease".to_string(),
            "ID: urn:vmomi:InventoryServiceTag:c9e4114c-11b4-422f-86c8-79bacdd45c60:GLOBAL".to_string(),
        ]);

        assert_eq!(
            id.as_deref(),
            Some("urn:vmomi:InventoryServiceTag:c9e4114c-11b4-422f-86c8-79bacdd45c60:GLOBAL")
        );
    }

    #[test]
    fn should_detect_tagging_cardinality_violation_error() {
        let err = CommandError::new(
            "Cannot run govc".to_string(),
            Some("govc: 400 Bad Request: Tagging cardinality violation".to_string()),
            None,
        );
        assert!(is_tagging_cardinality_violation(&err));
    }

    #[test]
    fn should_detect_tag_already_attached_error() {
        let err = CommandError::new(
            "Cannot run govc".to_string(),
            Some("govc: tag is already attached".to_string()),
            None,
        );
        assert!(is_tag_already_attached(&err));
    }

    #[test]
    fn should_reject_missing_govc_credentials() {
        let err = validate_govc_auth_envs(&[("GOVC_URL".to_string(), "https://vcenter.local".to_string())])
            .expect_err("auth should be rejected");
        assert!(err.to_string().contains("Missing vSphere credentials"));
    }

    #[test]
    fn should_accept_govc_username_password() {
        validate_govc_auth_envs(&[
            ("GOVC_URL".to_string(), "https://vcenter.local".to_string()),
            ("GOVC_USERNAME".to_string(), "svc_vsphere".to_string()),
            ("GOVC_PASSWORD".to_string(), "secret".to_string()),
        ])
        .expect("auth should be accepted");
    }
}
