//! Workspace chart preparation and parsing of the Kubernetes resources Helm plans to install.

use super::model::{
    FactState, HELM_HOOK_ANNOTATION, PlannedApiResource, PlannedCustomResourceDefinition, PlannedHelmHook,
    PlannedKubernetesResource, PlannedRoleReference, evidence, target_namespaces,
};
use super::ownership::InstalledHelmRelease;
use super::runtime::block_on_timeout;
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{Helm, HelmInventoryError};
use crate::events::EventDetails;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::platform_components::{
    download_platform_chart, write_platform_values_to_temporary_file,
};
use crate::infrastructure::infrastructure_context::{InfrastructureContext, KubeClientAuthMode};
use crate::io_models::platform_components::{PlatformHelmUnit, PlatformPreflightCheckId, PlatformPreflightRequest};
use kube::core::GroupVersionKind;
use kube::discovery::Discovery;
use semver::Version;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tempfile::NamedTempFile;

pub(super) const HELM_LIST_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) const HELM_TEMPLATE_TIMEOUT: Duration = Duration::from_secs(120);

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

const HELM_HOOK_DELETE_POLICY_ANNOTATION: &str = "helm.sh/hook-delete-policy";

const HELM_DELETE_BEFORE_HOOK_CREATION: &str = "before-hook-creation";

pub(in super::super) struct PreparedPlatformUnit {
    pub chart_dir: String,
    pub values_file: NamedTempFile,
    pub(super) resources: Vec<PlannedKubernetesResource>,
    pub(super) app_version: Option<Version>,
}

/// Why the Helm release inventory is missing from the prepared plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HelmInventoryFailure {
    /// The inventory reached the hard cap for the given scope; the cluster holds more releases
    /// than preflight reads, so absence cannot be proven.
    TooLarge { limit: usize, scope: String },
    /// Helm failed, timed out, or was canceled before returning the inventory.
    Unavailable,
}

#[derive(Default)]
pub(in super::super) struct PreparedPlatformPlan {
    pub(super) units: BTreeMap<String, PreparedPlatformUnit>,
    pub(super) failed_units: BTreeSet<String>,
    pub(super) discovery: Option<Discovery>,
    pub(super) installed_releases: Option<Vec<InstalledHelmRelease>>,
    pub(super) inventory_failure: Option<HelmInventoryFailure>,
}

impl PreparedPlatformPlan {
    pub(in super::super) fn take(&mut self, unit_key: &str) -> Option<PreparedPlatformUnit> {
        self.units.remove(unit_key)
    }

    pub(super) fn inventory_too_large(&self) -> bool {
        matches!(self.inventory_failure, Some(HelmInventoryFailure::TooLarge { .. }))
    }

    /// Evidence for checks that cannot run without the release inventory.
    pub(super) fn inventory_evidence(&self) -> BTreeMap<String, String> {
        match &self.inventory_failure {
            Some(HelmInventoryFailure::TooLarge { limit, scope }) => evidence([
                ("reason", "release-inventory-too-large"),
                ("limit", limit.to_string().as_str()),
                ("scope", scope.as_str()),
            ]),
            Some(HelmInventoryFailure::Unavailable) | None => BTreeMap::new(),
        }
    }

    pub(super) fn resources(&self) -> impl Iterator<Item = &PlannedKubernetesResource> {
        self.units.values().flat_map(|unit| unit.resources.iter())
    }

    pub(super) fn completeness(&self, include_unit: impl Fn(&str) -> bool) -> FactState {
        match self.failed_units.iter().find(|key| include_unit(key)) {
            Some(key) => FactState::Unavailable(evidence([("component", key.as_str())])),
            None => FactState::Pass,
        }
    }
}

/// Selects the same execution branch as `helm upgrade --install`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HelmRenderMode {
    Install,
    Upgrade,
}

impl HelmRenderMode {
    pub(super) fn for_release(releases: &[InstalledHelmRelease], unit: &PlatformHelmUnit) -> Self {
        if releases.iter().any(|release| {
            release.name == unit.release_name && release.namespace == unit.namespace && release.is_present()
        }) {
            Self::Upgrade
        } else {
            Self::Install
        }
    }

    fn runs_hook(self, events: &BTreeSet<String>) -> bool {
        let phases = match self {
            Self::Install => ["pre-install", "post-install"],
            Self::Upgrade => ["pre-upgrade", "post-upgrade"],
        };
        phases.iter().any(|phase| events.contains(*phase))
    }
}

/// Only cert-manager discovery needs releases outside the planned namespaces.
fn inventory_namespaces<'a>(request: &PlatformPreflightRequest, units: &'a [PlatformHelmUnit]) -> Vec<Option<&'a str>> {
    if request
        .checks
        .iter()
        .any(|check| check.id == PlatformPreflightCheckId::IncompatibleCertManager)
        && units.iter().any(is_cert_manager_unit)
    {
        vec![None]
    } else {
        target_namespaces(units).into_iter().map(Some).collect()
    }
}

pub(super) fn is_cert_manager_unit(unit: &PlatformHelmUnit) -> bool {
    unit.key == "cert-manager" || unit.chart.name == "cert-manager"
}

#[derive(Deserialize)]
struct ChartAppMetadata {
    #[serde(rename = "appVersion")]
    app_version: Option<String>,
}

/// Unreadable or non-semantic chart metadata is not fatal: the cert-manager check already falls
/// back to comparing chart versions, and no other check reads this value.
fn read_chart_app_version(chart_dir: &Path) -> Option<Version> {
    let yaml = fs::read_to_string(chart_dir.join("Chart.yaml")).ok()?;
    let metadata: ChartAppMetadata = serde_yaml::from_str(&yaml).ok()?;
    Version::parse(metadata.app_version?.trim_start_matches('v')).ok()
}

pub(super) struct HelmKubernetesCapabilities {
    pub(super) kube_version: String,
    pub(super) api_versions: BTreeSet<String>,
}

impl HelmKubernetesCapabilities {
    pub(super) fn from_discovery(kube_version: String, discovery: &Discovery) -> Self {
        let mut api_versions = BTreeSet::new();
        for group in discovery.groups() {
            for version in group.versions() {
                let api_version = if group.name().is_empty() {
                    version.to_string()
                } else {
                    format!("{}/{version}", group.name())
                };
                api_versions.insert(api_version.clone());
                for (resource, _) in group.versioned_resources(version) {
                    api_versions.insert(format!("{api_version}/{}", resource.kind));
                }
            }
        }
        Self {
            kube_version,
            api_versions,
        }
    }

    pub(super) fn template_args(&self, mode: HelmRenderMode) -> Vec<&str> {
        let mut args = vec!["--include-crds", "--kube-version", self.kube_version.as_str()];
        if mode == HelmRenderMode::Upgrade {
            args.push("--is-upgrade");
        }
        for api_version in &self.api_versions {
            args.extend(["--api-versions", api_version.as_str()]);
        }
        args
    }
}

pub(super) fn discover_kubernetes_capabilities(
    client: kube::Client,
) -> Option<(HelmKubernetesCapabilities, Discovery)> {
    block_on_timeout(DISCOVERY_TIMEOUT, async {
        let discover = async {
            match Discovery::new(client.clone()).run_aggregated().await {
                Ok(discovery) if discovery.has_group("") => Ok(discovery),
                // Older servers may return legacy discovery JSON with HTTP 200; kube decodes
                // that as an empty aggregated result instead of an error. Require the core API.
                Ok(_) | Err(_) => Discovery::new(client.clone()).run().await,
            }
        };
        let (version, discovery) = tokio::try_join!(client.apiserver_version(), discover)?;
        let capabilities = HelmKubernetesCapabilities::from_discovery(version.git_version, &discovery);
        Ok::<_, kube::Error>((capabilities, discovery))
    })
    .ok()
    .and_then(Result::ok)
}

/// Downloads and renders charts only when a requested check needs the concrete Kubernetes plan.
/// These operations write only to the worker workspace and never mutate the customer cluster.
pub(in super::super) fn prepare_platform_preflight_plan(
    infra_ctx: &InfrastructureContext,
    helm: &Helm,
    logger: &impl InfraLogger,
    event_details: &EventDetails,
    request: &PlatformPreflightRequest,
    units: &[PlatformHelmUnit],
) -> PreparedPlatformPlan {
    let needs_rendering = request.checks.iter().any(|check| {
        matches!(
            check.id,
            PlatformPreflightCheckId::RbacInsufficient
                | PlatformPreflightCheckId::CrdOwnershipConflict
                | PlatformPreflightCheckId::IncompatibleCertManager
        )
    });
    let mut plan = PreparedPlatformPlan::default();
    if needs_rendering
        || request
            .checks
            .iter()
            .any(|check| check.id == PlatformPreflightCheckId::ReleaseOwnershipConflict)
    {
        let deadline = CommandKiller::from_timeout(HELM_LIST_TIMEOUT);
        let mut releases = Vec::new();
        for namespace in inventory_namespaces(request, units) {
            match helm.list_all_releases_raw(namespace, &[], &deadline) {
                Ok(batch) => releases.extend(batch.into_iter().map(InstalledHelmRelease::from)),
                Err(HelmInventoryError::Saturated { limit }) => {
                    let scope = namespace.unwrap_or("cluster").to_string();
                    logger.warn(format!(
                        "⚠️ Preflight stopped reading the Helm release inventory for scope `{scope}`: the cluster holds at least {limit} releases, which exceeds the preflight inventory limit; release state is unavailable"
                    ));
                    plan.inventory_failure = Some(HelmInventoryFailure::TooLarge { limit, scope });
                    break;
                }
                Err(HelmInventoryError::Helm(_)) => {
                    logger.warn("⚠️ Preflight could not read the Helm release inventory within its budget; release state is unavailable");
                    plan.inventory_failure = Some(HelmInventoryFailure::Unavailable);
                    break;
                }
            }
        }
        if plan.inventory_failure.is_none() {
            plan.installed_releases = Some(releases);
        }
    }
    if !needs_rendering {
        return plan;
    }
    let cluster_capabilities = infra_ctx
        .mk_kube_client_with_auth_mode(KubeClientAuthMode::AllowInCluster)
        .ok()
        .and_then(|client| discover_kubernetes_capabilities(client.client()));
    let Some((capabilities, discovery)) = cluster_capabilities else {
        logger.warn("⚠️ Preflight could not discover Kubernetes capabilities for chart rendering");
        plan.failed_units.extend(units.iter().map(|unit| unit.key.clone()));
        return plan;
    };
    // Reuse the same discovery snapshot for rendering and resource checks, including partial plans.
    plan.discovery = Some(discovery);
    let Some(releases) = plan.installed_releases.as_deref() else {
        plan.failed_units.extend(units.iter().map(|unit| unit.key.clone()));
        return plan;
    };
    for unit in units {
        let render_mode = HelmRenderMode::for_release(releases, unit);
        let chart_dir = match download_platform_chart(infra_ctx, helm, logger, event_details, unit) {
            Ok(chart_dir) => chart_dir,
            Err(_) => {
                logger.warn(format!(
                    "⚠️ Preflight could not download the chart for platform component `{}`",
                    unit.key
                ));
                plan.failed_units.insert(unit.key.clone());
                continue;
            }
        };
        let app_version = read_chart_app_version(Path::new(&chart_dir));
        if app_version.is_none() && is_cert_manager_unit(unit) {
            logger.warn(format!(
                "⚠️ Preflight could not read a usable chart appVersion for platform component `{}`; cert-manager compatibility will compare chart versions",
                unit.key
            ));
        }
        let values_file = match write_platform_values_to_temporary_file(unit) {
            Ok(values_file) => values_file,
            Err(_) => {
                logger.warn(format!(
                    "⚠️ Preflight could not prepare values for platform component `{}`",
                    unit.key
                ));
                plan.failed_units.insert(unit.key.clone());
                continue;
            }
        };
        let values_arg = values_file.path().to_string_lossy().into_owned();
        let mut template_args = capabilities.template_args(render_mode);
        template_args.extend(["-f", values_arg.as_str()]);
        let rendered = helm.template_raw_silent(
            &unit.release_name,
            Path::new(&chart_dir),
            &unit.namespace,
            &template_args,
            &[],
            &CommandKiller::from_timeout(HELM_TEMPLATE_TIMEOUT),
            &mut |_| {},
        );
        let resources = match rendered {
            Ok(yaml) => match parse_rendered_resources(unit, &yaml, render_mode) {
                Ok(resources) => resources,
                Err(error) => {
                    logger.warn(format!(
                        "⚠️ Preflight could not read the rendered Kubernetes plan for platform component `{}`: {error}",
                        unit.key,
                    ));
                    plan.failed_units.insert(unit.key.clone());
                    continue;
                }
            },
            Err(_) => {
                logger.warn(format!(
                    "⚠️ Preflight could not render the chart for platform component `{}`",
                    unit.key
                ));
                plan.failed_units.insert(unit.key.clone());
                continue;
            }
        };
        plan.units.insert(
            unit.key.clone(),
            PreparedPlatformUnit {
                chart_dir,
                values_file,
                resources,
                app_version,
            },
        );
    }
    plan
}

pub(super) fn resolve_planned_custom_resource(
    plan: &PreparedPlatformPlan,
    gvk: &GroupVersionKind,
) -> Option<PlannedApiResource> {
    plan.resources()
        .filter_map(|resource| resource.custom_resource_definition.as_ref())
        .find(|definition| {
            definition.group == gvk.group && definition.kind == gvk.kind && definition.versions.contains(&gvk.version)
        })
        .map(|definition| PlannedApiResource {
            group: definition.group.clone(),
            version: gvk.version.clone(),
            plural: definition.plural.clone(),
            namespaced: definition.namespaced,
        })
}

pub(super) fn parse_rendered_resources(
    unit: &PlatformHelmUnit,
    yaml: &str,
    mode: HelmRenderMode,
) -> Result<Vec<PlannedKubernetesResource>, String> {
    let mut resources = Vec::new();
    // Helm emits one Source comment per raw crds/ file, which can contain several YAML documents,
    // but one per rendered templates/ document. Keep the CRD origin until the next Source comment.
    let mut installed_without_helm_ownership = false;
    for document in yaml.split("\n---") {
        if document.trim().is_empty() {
            continue;
        }
        if let Some(source) = document
            .lines()
            .find_map(|line| line.trim_start().strip_prefix("# Source:").map(str::trim))
        {
            installed_without_helm_ownership = source_is_crds_directory(source);
        }
        let value = serde_yaml::from_str::<serde_yaml::Value>(document)
            .map_err(|_| "cannot parse rendered chart".to_string())?;
        collect_rendered_value(unit, &value, installed_without_helm_ownership, &mut resources)?;
    }
    // Helm does not execute test/opposite-phase hooks, or install raw CRDs during upgrades.
    resources.retain(|resource| {
        !(mode == HelmRenderMode::Upgrade && resource.installed_without_helm_ownership)
            && resource
                .helm_hook
                .as_ref()
                .is_none_or(|hook| mode.runs_hook(&hook.events))
    });
    resources.sort();
    resources.dedup();
    Ok(resources)
}

fn source_is_crds_directory(source: &str) -> bool {
    let segments: Vec<_> = source.split('/').collect();
    !segments.contains(&"templates") && segments.contains(&"crds")
}

fn collect_rendered_value(
    unit: &PlatformHelmUnit,
    value: &serde_yaml::Value,
    installed_without_helm_ownership: bool,
    resources: &mut Vec<PlannedKubernetesResource>,
) -> Result<(), String> {
    if let Some(sequence) = value.as_sequence() {
        for item in sequence {
            collect_rendered_value(unit, item, installed_without_helm_ownership, resources)?;
        }
        return Ok(());
    }
    let Some(mapping) = value.as_mapping() else {
        return Ok(());
    };
    let string = |key: &str| {
        mapping
            .get(serde_yaml::Value::String(key.to_string()))
            .and_then(serde_yaml::Value::as_str)
    };
    if string("kind") == Some("List") {
        if let Some(items) = mapping
            .get(serde_yaml::Value::String("items".to_string()))
            .and_then(serde_yaml::Value::as_sequence)
        {
            for item in items {
                collect_rendered_value(unit, item, installed_without_helm_ownership, resources)?;
            }
        }
        return Ok(());
    }
    let (Some(api_version), Some(kind)) = (string("apiVersion"), string("kind")) else {
        return Ok(());
    };
    let Some(metadata) = mapping
        .get(serde_yaml::Value::String("metadata".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
    else {
        return Ok(());
    };
    let metadata_string = |key: &str| {
        metadata
            .get(serde_yaml::Value::String(key.to_string()))
            .and_then(serde_yaml::Value::as_str)
    };
    let name = metadata_string("name").map(str::to_string);
    let generate_name = metadata_string("generateName").map(str::to_string);
    if name.is_none() && generate_name.is_none() {
        return Ok(());
    }
    let custom_resource_definition = if kind == "CustomResourceDefinition" {
        Some(parse_custom_resource_definition(mapping)?)
    } else {
        None
    };
    let role_reference = if matches!(kind, "RoleBinding" | "ClusterRoleBinding") {
        parse_role_reference(mapping)
    } else {
        None
    };
    let helm_hook = parse_helm_hook(metadata);
    resources.push(PlannedKubernetesResource {
        unit_key: unit.key.clone(),
        release_name: unit.release_name.clone(),
        release_namespace: unit.namespace.clone(),
        api_version: api_version.to_string(),
        kind: kind.to_string(),
        name,
        generate_name,
        namespace: metadata_string("namespace").map(str::to_string),
        installed_without_helm_ownership,
        helm_hook,
        role_reference,
        custom_resource_definition,
    });
    Ok(())
}

fn parse_helm_hook(metadata: &serde_yaml::Mapping) -> Option<PlannedHelmHook> {
    let annotations = yaml_mapping_value(metadata, "annotations")?.as_mapping()?;
    let events = comma_separated_values(yaml_mapping_string(annotations, HELM_HOOK_ANNOTATION)?);
    if events.is_empty() {
        return None;
    }
    let deletes_before_creation = yaml_mapping_string(annotations, HELM_HOOK_DELETE_POLICY_ANNOTATION)
        .map(comma_separated_values)
        .is_none_or(|policies| policies.contains(HELM_DELETE_BEFORE_HOOK_CREATION));
    Some(PlannedHelmHook {
        events,
        deletes_before_creation,
    })
}

pub(super) fn comma_separated_values(raw: &str) -> BTreeSet<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_role_reference(resource: &serde_yaml::Mapping) -> Option<PlannedRoleReference> {
    let reference = yaml_mapping_value(resource, "roleRef")?.as_mapping()?;
    Some(PlannedRoleReference {
        api_group: yaml_mapping_string(reference, "apiGroup")?.to_string(),
        kind: yaml_mapping_string(reference, "kind")?.to_string(),
        name: yaml_mapping_string(reference, "name")?.to_string(),
    })
}

fn parse_custom_resource_definition(resource: &serde_yaml::Mapping) -> Result<PlannedCustomResourceDefinition, String> {
    let spec = yaml_mapping_value(resource, "spec")
        .and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| "rendered CustomResourceDefinition has no spec".to_string())?;
    let names = yaml_mapping_value(spec, "names")
        .and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| "rendered CustomResourceDefinition has no names".to_string())?;
    let group = yaml_mapping_string(spec, "group")
        .ok_or_else(|| "rendered CustomResourceDefinition has no group".to_string())?;
    let kind = yaml_mapping_string(names, "kind")
        .ok_or_else(|| "rendered CustomResourceDefinition has no kind".to_string())?;
    let plural = yaml_mapping_string(names, "plural")
        .ok_or_else(|| "rendered CustomResourceDefinition has no plural name".to_string())?;
    let namespaced = match yaml_mapping_string(spec, "scope") {
        Some("Namespaced") => true,
        Some("Cluster") => false,
        _ => return Err("rendered CustomResourceDefinition has an invalid scope".to_string()),
    };
    let mut versions = BTreeSet::new();
    if let Some(items) = yaml_mapping_value(spec, "versions").and_then(serde_yaml::Value::as_sequence) {
        for item in items.iter().filter_map(serde_yaml::Value::as_mapping) {
            let served = yaml_mapping_value(item, "served")
                .and_then(serde_yaml::Value::as_bool)
                .unwrap_or(true);
            if served && let Some(version) = yaml_mapping_string(item, "name") {
                versions.insert(version.to_string());
            }
        }
    }
    if versions.is_empty()
        && let Some(version) = yaml_mapping_string(spec, "version")
    {
        versions.insert(version.to_string());
    }
    if versions.is_empty() {
        return Err("rendered CustomResourceDefinition has no served version".to_string());
    }
    Ok(PlannedCustomResourceDefinition {
        group: group.to_string(),
        kind: kind.to_string(),
        plural: plural.to_string(),
        namespaced,
        versions,
    })
}

fn yaml_mapping_value<'a>(mapping: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    mapping.get(serde_yaml::Value::String(key.to_string()))
}

fn yaml_mapping_string<'a>(mapping: &'a serde_yaml::Mapping, key: &str) -> Option<&'a str> {
    yaml_mapping_value(mapping, key).and_then(serde_yaml::Value::as_str)
}

pub(super) fn plan_failure_evidence(plan: &PreparedPlatformPlan) -> BTreeMap<String, String> {
    plan.failed_units
        .iter()
        .next()
        .map(|unit| evidence([("component", unit.as_str())]))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::model::PlannedRoleReference;
    use super::super::test_support::{mock_kubernetes_client, unit};
    use super::{
        HelmRenderMode, PreparedPlatformPlan, PreparedPlatformUnit, discover_kubernetes_capabilities,
        parse_rendered_resources, resolve_planned_custom_resource,
    };
    use std::collections::BTreeSet;
    use tempfile::NamedTempFile;

    #[test]
    fn inventory_scope_is_global_only_for_an_applicable_cert_manager_check() {
        use super::super::test_support::{check, request};
        use super::inventory_namespaces;
        use crate::io_models::platform_components::{
            PlatformPreflightCheckId, PlatformPreflightCheckSeverity, PlatformPreflightMode,
        };
        let mut units = vec![unit("qovery"), unit("qovery")];
        let ownership = request(
            PlatformPreflightMode::Enforce,
            check(
                PlatformPreflightCheckId::ReleaseOwnershipConflict,
                PlatformPreflightCheckSeverity::Mandatory,
            ),
        );
        assert_eq!(inventory_namespaces(&ownership, &units), [Some("qovery")]);
        let cert_manager = request(
            PlatformPreflightMode::Enforce,
            check(
                PlatformPreflightCheckId::IncompatibleCertManager,
                PlatformPreflightCheckSeverity::Mandatory,
            ),
        );
        assert_eq!(inventory_namespaces(&cert_manager, &units), [Some("qovery")]);
        units[1].key = "cert-manager".to_string();
        assert_eq!(inventory_namespaces(&cert_manager, &units), [None]);
        assert_eq!(inventory_namespaces(&ownership, &units), [Some("qovery")]);
    }

    #[test]
    fn saturated_inventory_is_reported_with_its_limit_and_scope() {
        use super::HelmInventoryFailure;
        let mut plan = PreparedPlatformPlan::default();
        assert!(!plan.inventory_too_large());
        assert!(plan.inventory_evidence().is_empty());

        plan.inventory_failure = Some(HelmInventoryFailure::Unavailable);
        assert!(!plan.inventory_too_large());
        assert!(plan.inventory_evidence().is_empty());

        plan.inventory_failure = Some(HelmInventoryFailure::TooLarge {
            limit: 1_000,
            scope: "cluster".to_string(),
        });
        assert!(plan.inventory_too_large());
        let evidence = plan.inventory_evidence();
        assert_eq!(evidence.get("limit").map(String::as_str), Some("1000"));
        assert_eq!(evidence.get("scope").map(String::as_str), Some("cluster"));
    }

    #[test]
    fn render_mode_matches_upgrade_install_release_status() {
        use super::super::ownership::InstalledHelmRelease;
        use crate::cmd::structs::HelmListItem;
        let unit = unit("qovery");
        assert_eq!(HelmRenderMode::for_release(&[], &unit), HelmRenderMode::Install);
        for status in ["uninstalled", "deployed", "failed", "pending-upgrade", "superseded"] {
            let releases = [InstalledHelmRelease::from(HelmListItem {
                name: unit.release_name.clone(),
                namespace: unit.namespace.clone(),
                status: status.to_string(),
                ..Default::default()
            })];
            let expected = if status == "uninstalled" {
                HelmRenderMode::Install
            } else {
                HelmRenderMode::Upgrade
            };
            assert_eq!(HelmRenderMode::for_release(&releases, &unit), expected, "{status}");
        }
    }

    #[test]
    fn chart_app_version_is_read_independently_from_chart_version() {
        use super::read_chart_app_version;
        use semver::Version;
        use std::fs;
        let fixture = tempfile::tempdir().unwrap();
        for (app_field, expected) in [
            ("appVersion: v1.17.2", Some(Version::new(1, 17, 2))),
            ("appVersion: custom", None),
            ("appVersion: 1", None),
            ("", None),
        ] {
            fs::write(
                fixture.path().join("Chart.yaml"),
                format!("apiVersion: v2\nname: cert-manager\nversion: 0.3.0\n{app_field}\n"),
            )
            .unwrap();
            assert_eq!(read_chart_app_version(fixture.path()), expected);
        }
    }

    #[test]
    fn rendered_plan_checks_only_hooks_for_the_actual_operation() {
        let unit = unit("qovery");
        for (mode, expected_hook) in [
            (HelmRenderMode::Install, "install-hook"),
            (HelmRenderMode::Upgrade, "upgrade-hook"),
        ] {
            let yaml = [
                ("pre-install", "install-hook"), ("post-upgrade", "upgrade-hook"), ("test", "test-hook")
            ].into_iter().map(|(event,name)| format!("apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  name: {name}\n  annotations:\n    helm.sh/hook: {event}\n")).collect::<Vec<_>>().join("\n---\n");
            let resources = parse_rendered_resources(&unit, &yaml, mode).unwrap();
            assert_eq!(resources.len(), 1);
            assert_eq!(resources[0].name.as_deref(), Some(expected_hook));
        }
    }

    #[test]
    fn discovered_capabilities_include_server_version_all_api_versions_and_kinds() {
        let (capabilities, _) = discover_kubernetes_capabilities(mock_kubernetes_client(true, false)).unwrap();
        assert_eq!(capabilities.kube_version, "v1.34.3");
        for api in [
            "v1",
            "v1/ConfigMap",
            "apiregistration.k8s.io/v1",
            "apiregistration.k8s.io/v1/APIService",
            "example.com/v1beta1/Example",
        ] {
            assert!(capabilities.api_versions.contains(api), "{api}");
        }
    }

    #[test]
    fn rendered_plan_extracts_kubernetes_resources_without_values() {
        let resources = parse_rendered_resources(
            &unit("qovery"),
            r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: cluster-agent
  namespace: qovery
---
apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: examples.qovery.com
spec:
  group: qovery.com
  scope: Namespaced
  names:
    kind: Example
    plural: examples
  versions:
    - name: v1
      served: true
      storage: true
---
apiVersion: qovery.com/v1
kind: Example
metadata:
  name: example
  namespace: qovery
"#,
            HelmRenderMode::Install,
        )
        .unwrap();
        assert_eq!(resources.len(), 3);
        let definition = resources
            .iter()
            .find_map(|resource| resource.custom_resource_definition.as_ref())
            .unwrap();
        assert_eq!(definition.group, "qovery.com");
        assert_eq!(definition.kind, "Example");
        assert_eq!(definition.plural, "examples");
        assert!(definition.namespaced);
        assert_eq!(definition.versions, BTreeSet::from(["v1".to_string()]));

        let mut plan = PreparedPlatformPlan::default();
        let custom_gvk = resources
            .iter()
            .find(|resource| resource.kind == "Example")
            .unwrap()
            .gvk();
        plan.units.insert(
            "cluster-agent".to_string(),
            PreparedPlatformUnit {
                chart_dir: String::new(),
                values_file: NamedTempFile::new().unwrap(),
                app_version: None,
                resources,
            },
        );
        let resolved = resolve_planned_custom_resource(&plan, &custom_gvk).unwrap();
        assert_eq!(resolved.group, "qovery.com");
        assert_eq!(resolved.version, "v1");
        assert_eq!(resolved.plural, "examples");
        assert!(resolved.namespaced);
    }

    #[test]
    fn rendered_plan_supports_root_sequences_generated_names_and_role_references() {
        let resources = parse_rendered_resources(
            &unit("qovery"),
            r#"
- apiVersion: batch/v1
  kind: Job
  metadata:
    generateName: migration-
    namespace: qovery
- apiVersion: rbac.authorization.k8s.io/v1
  kind: RoleBinding
  metadata:
    name: agent-reader
    namespace: qovery
  roleRef:
    apiGroup: rbac.authorization.k8s.io
    kind: Role
    name: agent-reader
"#,
            HelmRenderMode::Install,
        )
        .unwrap();

        assert_eq!(resources.len(), 2);
        let job = resources.iter().find(|resource| resource.kind == "Job").unwrap();
        assert_eq!(job.name, None);
        assert_eq!(job.generate_name.as_deref(), Some("migration-"));
        let binding = resources
            .iter()
            .find(|resource| resource.kind == "RoleBinding")
            .unwrap();
        assert_eq!(
            binding.role_reference,
            Some(PlannedRoleReference {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "Role".to_string(),
                name: "agent-reader".to_string(),
            })
        );
    }
}
