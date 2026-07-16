#[path = "vsphere_govc.rs"]
mod govc;
#[path = "vsphere_tags.rs"]
mod tags;
#[path = "vsphere_template.rs"]
mod template;

use super::{ParsedEksAnywhereClusterConfig, ProviderPreflightError};
#[cfg(test)]
use crate::errors::CommandError;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::models::cloud_provider::CloudProvider;
#[cfg(test)]
use serde::Deserialize;
#[cfg(test)]
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::Path;

// ── Shared types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VSphereTemplateRef {
    pub machine_config_name: String,
    pub template: Option<String>,
    pub os_family: Option<String>,
    pub datastore: Option<String>,
    pub resource_pool: Option<String>,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct VSphereClusterMetadata {
    pub kubernetes_version: Option<String>,
    pub vcenter_server: Option<String>,
    pub insecure: Option<bool>,
    pub network: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct VSphereTemplateInstallConfig {
    pub template_path: String,
    pub template_name: String,
    pub folder_path: String,
    pub datastore: String,
    pub resource_pool: String,
    pub os_family: Option<String>,
}

// ── Shared utilities used by sub-modules ─────────────────────────────────────

pub(super) fn backtick(s: &str) -> String {
    format!("`{s}`")
}

pub(super) fn template_name_from_path(template_path: &str) -> Option<String> {
    Path::new(template_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
}

pub(super) fn template_label(template_path: &str) -> String {
    template_name_from_path(template_path).unwrap_or_else(|| template_path.to_string())
}

pub(super) fn stderr_or_error(stderr: &[String], fallback: String) -> String {
    if stderr.is_empty() { fallback } else { stderr.join("\n") }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub(super) fn run_vsphere_preflight(
    parsed_cluster_config: &ParsedEksAnywhereClusterConfig,
    cluster_config_path: &Path,
    cloud_provider: &dyn CloudProvider,
    install_missing: bool,
    expected_eksd_release_tag: Option<&str>,
    logger: &impl InfraLogger,
) -> Result<(), ProviderPreflightError> {
    let templates = extract_vsphere_templates_from_parsed_config(parsed_cluster_config);
    let metadata = extract_vsphere_cluster_metadata_from_parsed_config(parsed_cluster_config);

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

    let govc_env = govc::build_govc_envs(cloud_provider, &metadata);
    govc::validate_govc_auth_envs(&govc_env)?;
    govc::log_govc_version(logger, &govc_env);
    logger.info("🔐 Validating vSphere cloud credentials with vCenter.");
    if let Err(error) = govc::validate_govc_connection(&govc_env) {
        if govc::is_invalid_login_fault(&error) {
            return Err(ProviderPreflightError::VSphereCloudCredentialsRejected(error));
        }
        return Err(error.into());
    }
    logger.info("✅ vSphere cloud credentials accepted by vCenter.");
    template::check_templates_with_govc(
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

// ── Config extraction ─────────────────────────────────────────────────────────

fn extract_vsphere_cluster_metadata_from_parsed_config(
    parsed_cluster_config: &ParsedEksAnywhereClusterConfig,
) -> VSphereClusterMetadata {
    let mut metadata = VSphereClusterMetadata {
        kubernetes_version: parsed_cluster_config
            .cluster_spec()
            .and_then(|cluster| cluster.kubernetes_version.clone()),
        ..VSphereClusterMetadata::default()
    };

    if let Some(vsphere_datacenter) = parsed_cluster_config.vsphere_datacenter_config() {
        metadata.vcenter_server = vsphere_datacenter.server.clone();
        metadata.insecure = vsphere_datacenter.insecure;
        metadata.network = vsphere_datacenter.network.clone();
    }

    metadata
}

fn extract_vsphere_templates_from_parsed_config(
    parsed_cluster_config: &ParsedEksAnywhereClusterConfig,
) -> Vec<VSphereTemplateRef> {
    parsed_cluster_config
        .vsphere_machine_configs()
        .map(|machine_config| VSphereTemplateRef {
            machine_config_name: machine_config.name.clone(),
            template: machine_config.template.clone(),
            os_family: machine_config.os_family.clone(),
            datastore: machine_config.datastore.clone(),
            resource_pool: machine_config.resource_pool.clone(),
            folder: machine_config.folder.clone(),
        })
        .collect()
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
                .map(|t| backtick(&template_label(t)))
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
        lines.push(format!("🗂️ Template full path: `{template}`"));
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

// ── Test utilities ────────────────────────────────────────────────────────────

#[cfg(test)]
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

#[cfg(test)]
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::govc::validate_govc_auth_envs;
    use super::tags::{
        attached_tag_value_for_category, attached_tag_values_for_category, extract_vm_moid_from_vm_info_json,
        has_exact_eksd_release_tag, has_expected_eksd_release_tag, has_expected_os_tag, is_category_prefixed_tag_name,
        is_flag_not_defined_error, is_same_or_ancestor_inventory_path, is_tag_already_attached,
        is_tag_identifier_not_found_error, is_tag_name_not_found, is_tag_not_found_in_category,
        is_tagging_cardinality_violation, normalize_inventory_object_path, normalize_vm_moid,
        parse_tag_id_from_tags_info_output, tag_name_candidates_for_category,
    };
    use super::template::expected_eksd_release_fragment_from_kubernetes_version;
    use super::{
        VSphereClusterMetadata, VSphereTemplateRef, extract_vsphere_cluster_metadata_from_yaml,
        extract_vsphere_templates_from_yaml, summarize_vsphere_templates_for_user,
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
                "🗂️ Template full path: `/dc/vm/Templates/template-a`".to_string(),
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
