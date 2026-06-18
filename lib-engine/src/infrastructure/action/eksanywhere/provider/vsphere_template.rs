use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::errors::{CommandError, ErrorMessageVerbosity};
use crate::infrastructure::action::InfraLogger;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

use super::govc::{run_govc_command, run_govc_command_with_timeout_logged};
use super::tags::{
    attached_tag_values_for_category, canonical_tag_name_for_category, ensure_tag_and_attach,
    has_exact_eksd_release_tag, has_expected_eksd_release_tag, has_expected_os_tag,
    is_inventory_object_not_found_error, is_tag_directly_attached_to_object, is_tagging_cardinality_violation,
};
use super::{VSphereClusterMetadata, VSphereTemplateInstallConfig, VSphereTemplateRef};

type TemplateIndexEntry = (BTreeSet<String>, BTreeSet<String>, Vec<VSphereTemplateRef>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum RetagEligibility {
    Eligible,
    NotEligible(String),
}

pub(super) fn check_templates_with_govc(
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
                .map(|name| super::backtick(&name))
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
        let template_exists = is_template_present_for_inventory_path(template.as_str(), govc_env, logger)?;
        if !template_exists {
            if !install_missing {
                let install_config = build_template_install_config(template.as_str(), &refs)?;
                let ova_url = resolve_ova_url_for_template_with_logging(
                    cluster_config_path,
                    install_config.template_name.as_str(),
                    logger,
                )?;
                logger.warn(format!(
                    "⚠️ Template `{}` not found in vSphere during dry-run. Apply mode will automatically import it from `{ova_url}`.",
                    super::template_label(template.as_str())
                ));
                logger.info(format!(
                    "ℹ️ Dry-run: skipping vSphere tag checks for missing template `{}`.",
                    super::template_label(template.as_str())
                ));
                continue;
            }

            logger.warn(format!(
                "⚠️ Template `{}` not found. Non dry-run mode: automatic OVA import will be attempted.",
                super::template_label(template.as_str())
            ));
            install_missing_template_and_verify(
                cluster_config_path,
                template.as_str(),
                &refs,
                expected_eksd_tag.as_deref(),
                metadata,
                govc_env,
                logger,
            )?;
        }

        let attached_tags = match run_govc_command(&["tags.attached.ls", "-r", template.as_str()], govc_env) {
            Ok(tags) => tags,
            Err(err) if !install_missing && is_inventory_object_not_found_error(&err) => {
                let install_config = build_template_install_config(template.as_str(), &refs)?;
                let ova_url = resolve_ova_url_for_template_with_logging(
                    cluster_config_path,
                    install_config.template_name.as_str(),
                    logger,
                )?;
                logger.warn(format!(
                    "⚠️ Template `{}` cannot be resolved for tag inspection during dry-run. Apply mode will automatically import it from `{ova_url}`.",
                    super::template_label(template.as_str())
                ));
                logger.info(format!(
                    "ℹ️ Dry-run: skipping vSphere tag checks for template `{}`.",
                    super::template_label(template.as_str())
                ));
                continue;
            }
            Err(err) if install_missing && is_inventory_object_not_found_error(&err) => {
                // `govc vm.info` resolved the template but `tags.attached.ls` reports it as not found.
                // This can happen while vSphere tagging inventory is still catching up after an import,
                // so retry the tag listing before deciding whether the template is actually missing.
                logger.warn(format!(
                    "⚠️ Template `{}` was reported by `govc vm.info` but cannot be resolved for tag inspection yet. \
This is usually vSphere inventory lag; retrying tag inspection before taking action. Tag inspection error: {}",
                    super::template_label(template.as_str()),
                    err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)
                ));

                match list_attached_tags_after_transient_not_found(template.as_str(), govc_env, logger) {
                    Ok(tags) => tags,
                    Err(retry_err) if is_inventory_object_not_found_error(&retry_err) => {
                        let template_exists_after_retries =
                            is_template_present_for_inventory_path(template.as_str(), govc_env, logger).map_err(
                                |presence_error| {
                                    CommandError::new(
                                        format!(
                                            "Unable to list tags attached to template `{template}` and failed to re-check template presence"
                                        ),
                                        Some(format!(
                                            "Tag inspection error after retries: {}\nTemplate presence re-check error: {}",
                                            retry_err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars),
                                            presence_error.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)
                                        )),
                                        None,
                                    )
                                },
                            )?;

                        if template_exists_after_retries {
                            return Err(CommandError::new(
                                format!(
                                    "Template `{template}` is present in vSphere but cannot be resolved by vSphere tagging inventory after retries"
                                ),
                                Some(format!(
                                    "Last tag inspection error: {}",
                                    retry_err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)
                                )),
                                None,
                            ));
                        }

                        logger.warn(format!(
                            "⚠️ Template `{}` no longer resolves with `govc vm.info` after tag inspection retries. Automatic OVA import will be attempted.",
                            super::template_label(template.as_str())
                        ));
                        install_missing_template_and_verify(
                            cluster_config_path,
                            template.as_str(),
                            &refs,
                            expected_eksd_tag.as_deref(),
                            metadata,
                            govc_env,
                            logger,
                        )?;

                        list_attached_tags_after_import_attempt(template.as_str(), govc_env)?
                    }
                    Err(retry_err) => {
                        return Err(CommandError::new(
                            format!("Unable to list tags attached to template `{template}`"),
                            Some(retry_err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                            None,
                        ));
                    }
                }
            }
            Err(e) => {
                return Err(CommandError::new(
                    format!("Unable to list tags attached to template `{template}`"),
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    None,
                ));
            }
        };
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
                        super::template_label(template.as_str())
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
                        super::template_label(template.as_str()),
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

        if os_families.len() > 1 {
            return Err(CommandError::new_from_safe_message(format!(
                "Template `{}` is shared by machine configs with conflicting `osFamily` values: [{}]. \
Each osFamily must use a dedicated template.",
                super::template_label(template.as_str()),
                os_families.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            )));
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
            super::template_label(template.as_str())
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
                super::template_label(template.as_str())
            ));
        }
    }

    Ok(())
}

fn install_missing_template_and_verify(
    cluster_config_path: &Path,
    template: &str,
    refs: &[VSphereTemplateRef],
    expected_eksd_tag: Option<&str>,
    metadata: &VSphereClusterMetadata,
    govc_env: &[(String, String)],
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    let install_config = build_template_install_config(template, refs)?;
    install_missing_template(
        cluster_config_path,
        &install_config,
        expected_eksd_tag,
        metadata,
        govc_env,
        logger,
    )?;

    if !is_template_present_for_inventory_path(template, govc_env, logger)? {
        return Err(CommandError::new_from_safe_message(format!(
            "Template `{template}` is still not available in vSphere after import attempt"
        )));
    }

    Ok(())
}

fn is_template_present_for_inventory_path(
    template_path: &str,
    govc_env: &[(String, String)],
    logger: &impl InfraLogger,
) -> Result<bool, CommandError> {
    match run_govc_command(&["vm.info", "-json", "-vm.ipath", template_path], govc_env) {
        Ok(lines) => match template_presence_from_vm_info_output(template_path, &lines)? {
            TemplatePresence::Present { count } => {
                if count > 1 {
                    logger.warn(format!(
                        "⚠️ `govc vm.info -vm.ipath` returned {count} objects for template `{template_path}`. \
Continuing because the inventory path resolved, but vSphere inventory may be ambiguous."
                    ));
                }
                Ok(true)
            }
            TemplatePresence::Missing => {
                logger.warn(format!(
                    "⚠️ `govc vm.info -vm.ipath` returned no VirtualMachines entry for template `{template_path}`."
                ));
                Ok(false)
            }
        },
        Err(err) if is_inventory_object_not_found_error(&err) => Ok(false),
        Err(err) => Err(CommandError::new(
            format!("Unable to verify template presence for `{template_path}` with `govc vm.info -vm.ipath`"),
            Some(err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
            None,
        )),
    }
}

/// Number of times to retry `govc tags.attached.ls` when the template resolves via `govc vm.info`
/// but tag inspection transiently reports it as not found (vSphere inventory eventual consistency
/// right after an import). Kept small so a genuinely-missing template still reaches the OVA re-import
/// fallback quickly.
const TAGS_ATTACHED_LS_RETRY_ATTEMPTS: usize = 3;
const TAGS_ATTACHED_LS_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Retry `govc tags.attached.ls` after the caller already observed a transient "not found" error,
/// waiting `TAGS_ATTACHED_LS_RETRY_DELAY` before each attempt to give vSphere inventory time to settle.
///
/// Returns the attached tags on success. On a persistent not-found, returns the last not-found error
/// so the caller can decide to fall back to an OVA re-import; any other error is returned immediately.
fn list_attached_tags_after_transient_not_found(
    template_path: &str,
    govc_env: &[(String, String)],
    logger: &impl InfraLogger,
) -> Result<Vec<String>, CommandError> {
    let mut last_error = None;
    for attempt in 1..=TAGS_ATTACHED_LS_RETRY_ATTEMPTS {
        logger.info(format!(
            "⏳ Waiting {}s for vSphere inventory to settle, then retrying tag inspection for template `{}` (attempt {attempt}/{TAGS_ATTACHED_LS_RETRY_ATTEMPTS}).",
            TAGS_ATTACHED_LS_RETRY_DELAY.as_secs(),
            super::template_label(template_path),
        ));
        std::thread::sleep(TAGS_ATTACHED_LS_RETRY_DELAY);
        match run_govc_command(&["tags.attached.ls", "-r", template_path], govc_env) {
            Ok(tags) => return Ok(tags),
            Err(err) if is_inventory_object_not_found_error(&err) => last_error = Some(err),
            Err(err) => return Err(err),
        }
    }

    Err(last_error.expect("retry loop records an error before exhausting attempts"))
}

fn list_attached_tags_after_import_attempt(
    template_path: &str,
    govc_env: &[(String, String)],
) -> Result<Vec<String>, CommandError> {
    run_govc_command(&["tags.attached.ls", "-r", template_path], govc_env).map_err(|retry_error| {
        CommandError::new(
            format!("Unable to list tags attached to template `{template_path}` after import attempt"),
            Some(retry_error.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
            None,
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemplatePresence {
    Present { count: usize },
    Missing,
}

fn template_presence_from_vm_info_output(
    template_path: &str,
    output_lines: &[String],
) -> Result<TemplatePresence, CommandError> {
    let vm_info_json = output_lines.join("\n");
    let vm_info: JsonValue = serde_json::from_str(vm_info_json.as_str()).map_err(|e| {
        CommandError::new(
            format!("Cannot parse `govc vm.info -json -vm.ipath` output for `{template_path}`"),
            Some(format!("{e}. Output excerpt: {}", output_excerpt(vm_info_json.as_str()))),
            None,
        )
    })?;

    let Some(virtual_machines) = vm_info
        .get("VirtualMachines")
        .or_else(|| vm_info.get("virtualMachines"))
    else {
        return Err(CommandError::new(
            format!("Cannot determine whether vSphere template `{template_path}` exists"),
            Some(format!(
                "`govc vm.info -json -vm.ipath` output does not contain `VirtualMachines` or `virtualMachines`. Output excerpt: {}",
                output_excerpt(vm_info_json.as_str())
            )),
            None,
        ));
    };

    match virtual_machines {
        JsonValue::Null => Ok(TemplatePresence::Missing),
        JsonValue::Array(vms) if vms.is_empty() => Ok(TemplatePresence::Missing),
        JsonValue::Array(vms) => Ok(TemplatePresence::Present { count: vms.len() }),
        other => Err(CommandError::new(
            format!("Cannot determine whether vSphere template `{template_path}` exists"),
            Some(format!(
                "`VirtualMachines` field has unexpected JSON type `{}`. Output excerpt: {}",
                json_type_name(other),
                output_excerpt(vm_info_json.as_str())
            )),
            None,
        )),
    }
}

fn json_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

fn output_excerpt(output: &str) -> String {
    const MAX_EXCERPT_CHARS: usize = 500;
    let mut excerpt = output.chars().take(MAX_EXCERPT_CHARS).collect::<String>();
    if output.chars().count() > MAX_EXCERPT_CHARS {
        excerpt.push_str("...");
    }
    excerpt
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

    let template_name = super::template_name_from_path(template_path).ok_or_else(|| {
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

pub(super) fn build_template_install_config(
    template_path: &str,
    refs: &[VSphereTemplateRef],
) -> Result<VSphereTemplateInstallConfig, CommandError> {
    let template_name = super::template_name_from_path(template_path).ok_or_else(|| {
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
    std::fs::create_dir_all(&destination_dir).map_err(|e| {
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
            Some(super::stderr_or_error(&stderr, e.to_string())),
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

// ── OVA URL resolution ───────────────────────────────────────────────────────

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
            Some(super::stderr_or_error(&stderr, e.to_string())),
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

// ── Template name parsing ────────────────────────────────────────────────────

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

pub(super) fn expected_eksd_release_fragment_from_kubernetes_version(kubernetes_version: &str) -> Option<String> {
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

// ── Path utilities (template-specific) ──────────────────────────────────────

fn template_folder_from_path(template_path: &str) -> Option<String> {
    Path::new(template_path)
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{TemplatePresence, template_presence_from_vm_info_output};

    fn vm_info_output(json: &str) -> Vec<String> {
        json.lines().map(str::to_string).collect()
    }

    #[test]
    fn should_treat_lowercase_null_virtual_machines_as_missing_template() {
        let presence = template_presence_from_vm_info_output(
            "/dc/vm/Templates/missing-template",
            &vm_info_output(r#"{"virtualMachines": null}"#),
        )
        .expect("vm.info output should parse");

        assert_eq!(presence, TemplatePresence::Missing);
    }

    #[test]
    fn should_treat_empty_virtual_machines_array_as_missing_template() {
        let presence = template_presence_from_vm_info_output(
            "/dc/vm/Templates/missing-template",
            &vm_info_output(r#"{"VirtualMachines": []}"#),
        )
        .expect("vm.info output should parse");

        assert_eq!(presence, TemplatePresence::Missing);
    }

    #[test]
    fn should_treat_non_empty_pascalcase_virtual_machines_array_as_present_template() {
        let presence = template_presence_from_vm_info_output(
            "/dc/vm/Templates/template-a",
            &vm_info_output(
                r#"{
  "VirtualMachines": [
    {
      "Self": {
        "Type": "VirtualMachine",
        "Value": "vm-42"
      }
    }
  ]
}"#,
            ),
        )
        .expect("vm.info output should parse");

        assert_eq!(presence, TemplatePresence::Present { count: 1 });
    }

    #[test]
    fn should_treat_non_empty_lowercase_virtual_machines_array_as_present_template() {
        let presence = template_presence_from_vm_info_output(
            "/dc/vm/Templates/template-a",
            &vm_info_output(
                r#"{
  "virtualMachines": [
    {
      "Self": {
        "Type": "VirtualMachine",
        "Value": "vm-42"
      }
    }
  ]
}"#,
            ),
        )
        .expect("vm.info output should parse");

        assert_eq!(presence, TemplatePresence::Present { count: 1 });
    }

    #[test]
    fn should_reject_vm_info_without_virtual_machines_field() {
        let error = template_presence_from_vm_info_output(
            "/dc/vm/Templates/template-a",
            &vm_info_output(r#"{"kind": "unexpected"}"#),
        )
        .expect_err("vm.info output should be rejected");

        assert!(error.to_string().contains("Cannot determine whether vSphere template"));
    }
}
