//! Helm release identity, cluster resource ownership, and cert-manager compatibility checks.

use super::model::{
    FactState, HELM_HOOK_ANNOTATION, HELM_MANAGED_BY_LABEL, HELM_RELEASE_NAME_ANNOTATION,
    HELM_RELEASE_NAMESPACE_ANNOTATION, PlannedKubernetesResource, evidence,
};
use super::plan::{PreparedPlatformPlan, is_cert_manager_unit, plan_failure_evidence};
use super::runtime::{KUBERNETES_READ_TIMEOUT, block_on_timeout};
use crate::cmd::structs::HelmListItem;
use crate::io_models::platform_components::PlatformHelmUnit;
use kube::api::{Api, DynamicObject};
use kube::discovery::{Discovery, Scope};
use semver::Version;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const CLUSTER_RESOURCE_CHECK_BUDGET: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReleasePresence {
    Present,
    Uninstalled,
}

pub(super) struct InstalledHelmRelease {
    pub(super) name: String,
    pub(super) namespace: String,
    pub(super) chart_name: String,
    pub(super) chart_version: Option<Version>,
    pub(super) app_version: Option<Version>,
    presence: ReleasePresence,
}

impl From<HelmListItem> for InstalledHelmRelease {
    fn from(release: HelmListItem) -> Self {
        // Preflight needs the chart identity and full semantic version without changing legacy deployment decisions.
        let (chart_name, chart_version) = split_helm_chart_reference(&release.chart);
        let app_version = Version::parse(release.app_version.strip_prefix('v').unwrap_or(&release.app_version)).ok();
        Self {
            name: release.name,
            namespace: release.namespace,
            chart_name,
            chart_version,
            app_version,
            presence: if release.status == "uninstalled" {
                ReleasePresence::Uninstalled
            } else {
                ReleasePresence::Present
            },
        }
    }
}

impl InstalledHelmRelease {
    pub(super) fn is_present(&self) -> bool {
        self.presence == ReleasePresence::Present
    }
}

fn split_helm_chart_reference(reference: &str) -> (String, Option<Version>) {
    reference
        .match_indices('-')
        .rev()
        .find_map(|(index, _)| {
            let raw_version = reference[index + 1..]
                .strip_prefix('v')
                .unwrap_or(&reference[index + 1..]);
            Version::parse(raw_version)
                .ok()
                .map(|version| (reference[..index].to_string(), Some(version)))
        })
        .unwrap_or_else(|| (reference.to_string(), None))
}

pub(super) fn collect_release_ownership(plan: &PreparedPlatformPlan, units: &[PlatformHelmUnit]) -> FactState {
    let Some(releases) = plan.installed_releases.as_deref() else {
        return FactState::Unavailable(plan.inventory_evidence());
    };
    release_ownership_from(releases, units)
}

pub(super) fn release_ownership_from(releases: &[InstalledHelmRelease], units: &[PlatformHelmUnit]) -> FactState {
    for unit in units {
        if let Some(release) = releases.iter().find(|release| {
            release.is_present() && release.name == unit.release_name && release.namespace == unit.namespace
        }) {
            if release.chart_version.is_none() {
                return FactState::Unavailable(evidence([
                    ("release", unit.release_name.as_str()),
                    ("namespace", unit.namespace.as_str()),
                ]));
            }
            if release.chart_name == unit.chart.name {
                continue;
            }
            let current_owner = format!("{}/{} ({})", release.namespace, release.name, release.chart_name);
            let expected_owner = format!("{}/{} ({})", unit.namespace, unit.release_name, unit.chart.name);
            return FactState::Fail(evidence([
                ("release", unit.release_name.as_str()),
                ("namespace", unit.namespace.as_str()),
                ("currentOwner", current_owner.as_str()),
                ("expectedOwner", expected_owner.as_str()),
            ]));
        }
    }
    FactState::Pass
}

pub(super) fn collect_cluster_resource_ownership(
    client: kube::Client,
    discovery: Option<&Discovery>,
    plan: &PreparedPlatformPlan,
    include_unit: impl Fn(&str) -> bool,
) -> FactState {
    let Some(discovery) = discovery else {
        return FactState::Unavailable(plan_failure_evidence(plan));
    };
    let started_at = Instant::now();
    let mut completion = plan.completeness(&include_unit);
    for planned in plan.resources() {
        if !include_unit(&planned.unit_key) {
            continue;
        }
        let Some((resource, capabilities)) = discovery.resolve_gvk(&planned.gvk()) else {
            continue;
        };
        if capabilities.scope != Scope::Cluster || !is_owned_cluster_resource(&planned.kind) {
            continue;
        }
        let Some(name) = planned.name.as_deref() else {
            continue;
        };
        let Some(remaining) = CLUSTER_RESOURCE_CHECK_BUDGET.checked_sub(started_at.elapsed()) else {
            return FactState::Unavailable(evidence([("reason", "time-budget-exhausted")]));
        };
        let api: Api<DynamicObject> = Api::all_with(client.clone(), &resource);
        match block_on_timeout(KUBERNETES_READ_TIMEOUT.min(remaining), api.get_opt(name)) {
            Ok(Ok(None)) => {}
            Ok(Ok(Some(existing))) => {
                let annotations = existing.metadata.annotations.unwrap_or_default();
                let labels = existing.metadata.labels.unwrap_or_default();
                let current_release = annotations.get(HELM_RELEASE_NAME_ANNOTATION).map(String::as_str);
                let current_namespace = annotations.get(HELM_RELEASE_NAMESPACE_ANNOTATION).map(String::as_str);
                let managed_by = labels.get(HELM_MANAGED_BY_LABEL).map(String::as_str);
                let current_hook = annotations.get(HELM_HOOK_ANNOTATION).map(String::as_str);
                if has_foreign_cluster_resource_owner(
                    planned,
                    current_release,
                    current_namespace,
                    managed_by,
                    current_hook,
                ) {
                    let resource_name = planned.display_name();
                    let current_owner = ownership_display(current_release, current_namespace);
                    let expected_owner = planned.expected_owner();
                    return FactState::Fail(evidence([
                        ("resource", resource_name.as_str()),
                        ("currentOwner", current_owner.as_str()),
                        ("expectedOwner", expected_owner.as_str()),
                        ("component", planned.unit_key.as_str()),
                    ]));
                }
            }
            Ok(Err(_)) | Err(_) => {
                let resource_name = planned.display_name();
                completion = FactState::Unavailable(evidence([("resource", resource_name.as_str())]));
            }
        }
    }
    completion
}

pub(super) fn has_foreign_cluster_resource_owner(
    planned: &PlannedKubernetesResource,
    current_release: Option<&str>,
    current_namespace: Option<&str>,
    managed_by: Option<&str>,
    current_hook: Option<&str>,
) -> bool {
    // Helm installs files under a chart's `crds/` directory without ownership annotations.
    // Their mere presence is therefore not proof that another release owns them.
    if planned.installed_without_helm_ownership
        && current_release.is_none()
        && current_namespace.is_none()
        && managed_by.is_none()
    {
        return false;
    }
    // Helm hook resources are not tracked as part of a release and therefore do not receive the
    // regular release ownership metadata. Active hooks delete the old object regardless of its
    // previous events. Helm explicitly excludes CRDs from this deletion policy.
    if planned.helm_hook.as_ref().is_some_and(|hook| {
        hook.deletes_before_creation && planned.kind != "CustomResourceDefinition" && current_hook.is_some()
    }) && current_release.is_none()
        && current_namespace.is_none()
        && managed_by.is_none_or(|manager| manager == "Helm")
    {
        return false;
    }
    current_release != Some(planned.release_name.as_str())
        || current_namespace != Some(planned.release_namespace.as_str())
        || managed_by != Some("Helm")
}

pub(super) fn collect_cert_manager_compatibility(
    client: kube::Client,
    discovery: Option<&Discovery>,
    units: &[PlatformHelmUnit],
    plan: &PreparedPlatformPlan,
) -> FactState {
    // Same predicate as the inventory scope: the two must not drift apart.
    let cert_manager_units: Vec<&PlatformHelmUnit> = units.iter().filter(|unit| is_cert_manager_unit(unit)).collect();
    if cert_manager_units.is_empty() {
        return FactState::NotApplicable;
    }
    let cert_manager_keys: BTreeSet<&str> = cert_manager_units.iter().map(|unit| unit.key.as_str()).collect();
    let ownership = collect_cluster_resource_ownership(client, discovery, plan, |key| cert_manager_keys.contains(key));
    if !matches!(ownership, FactState::Pass) {
        return ownership;
    }
    let Some(releases) = plan.installed_releases.as_deref() else {
        return FactState::Unavailable(plan.inventory_evidence());
    };
    cert_manager_release_compatibility(releases, &cert_manager_units, plan)
}

fn cert_manager_release_compatibility(
    releases: &[InstalledHelmRelease],
    cert_manager_units: &[&PlatformHelmUnit],
    plan: &PreparedPlatformPlan,
) -> FactState {
    for unit in cert_manager_units {
        let expected = match Version::parse(unit.chart.version.trim_start_matches('v')) {
            Ok(version) => version,
            Err(_) => {
                return FactState::Unavailable(evidence([("component", unit.key.as_str())]));
            }
        };
        for release in releases.iter().filter(|release| {
            release.is_present()
                && (release.chart_name == "cert-manager"
                    || (release.name == unit.release_name && release.namespace == unit.namespace))
        }) {
            if release.name != unit.release_name
                || release.namespace != unit.namespace
                || release.chart_name != unit.chart.name
            {
                let current_owner = format!("{}/{} ({})", release.namespace, release.name, release.chart_name);
                let expected_owner = format!("{}/{} ({})", unit.namespace, unit.release_name, unit.chart.name);
                return FactState::Fail(evidence([
                    ("release", release.name.as_str()),
                    ("currentOwner", current_owner.as_str()),
                    ("expectedOwner", expected_owner.as_str()),
                ]));
            }
            // Never compare an application version with a chart version.
            let planned_app_version = plan
                .units
                .get(&unit.key)
                .and_then(|prepared| prepared.app_version.as_ref());
            let (current, expected) = match (release.app_version.as_ref(), planned_app_version) {
                (Some(current), Some(expected)) => (current, expected),
                _ => match release.chart_version.as_ref() {
                    Some(current) => (current, &expected),
                    None => return FactState::Unavailable(evidence([("release", release.name.as_str())])),
                },
            };
            if current > expected {
                return FactState::Fail(evidence([
                    ("release", unit.release_name.as_str()),
                    ("currentVersion", current.to_string().as_str()),
                    ("expectedVersion", expected.to_string().as_str()),
                ]));
            }
        }
    }
    FactState::Pass
}

fn is_owned_cluster_resource(kind: &str) -> bool {
    matches!(
        kind,
        "CustomResourceDefinition"
            | "ClusterRole"
            | "ClusterRoleBinding"
            | "PriorityClass"
            | "APIService"
            | "MutatingWebhookConfiguration"
            | "ValidatingWebhookConfiguration"
    )
}

fn ownership_display(release: Option<&str>, namespace: Option<&str>) -> String {
    match (namespace, release) {
        (Some(namespace), Some(release)) => format!("{namespace}/{release}"),
        _ => "unmanaged".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{FactState, evidence};
    use super::super::plan::{
        HelmRenderMode, PreparedPlatformPlan, discover_kubernetes_capabilities, parse_rendered_resources,
    };
    use super::super::test_support::{mock_kubernetes_client, plan_with_cluster_role, unit};
    use super::{
        InstalledHelmRelease, cert_manager_release_compatibility, collect_cert_manager_compatibility,
        collect_cluster_resource_ownership, has_foreign_cluster_resource_owner, is_owned_cluster_resource,
        release_ownership_from,
    };
    use crate::cmd::structs::HelmListItem;
    use semver::Version;
    use std::collections::BTreeMap;
    use std::slice;

    #[test]
    fn only_uninstalled_release_history_is_ignored_by_ownership_checks() {
        let mut unit = unit("qovery");
        unit.key = "cert-manager".to_string();
        unit.chart.name = "cert-manager".to_string();
        for status in [
            "uninstalled",
            "failed",
            "pending-install",
            "pending-upgrade",
            "deployed",
        ] {
            let releases = [InstalledHelmRelease::from(HelmListItem {
                name: unit.release_name.clone(),
                namespace: unit.namespace.clone(),
                chart: "foreign-1.0.0".to_string(),
                status: status.to_string(),
                ..Default::default()
            })];
            let expected_pass = status == "uninstalled";
            assert_eq!(
                release_ownership_from(&releases, slice::from_ref(&unit)) == FactState::Pass,
                expected_pass,
                "{status}"
            );
            assert_eq!(
                cert_manager_release_compatibility(&releases, &[&unit], &PreparedPlatformPlan::default())
                    == FactState::Pass,
                expected_pass,
                "{status}"
            );
        }
    }

    #[test]
    fn repackaged_cert_manager_compares_like_versions_and_detects_app_downgrades() {
        let mut unit = unit("qovery");
        unit.key = "cert-manager".to_string();
        unit.chart.name = "cert-manager".to_string();
        unit.chart.version = "0.3.0".to_string();
        let mut plan = plan_with_cluster_role(&unit);
        let releases = [InstalledHelmRelease::from(HelmListItem {
            name: unit.release_name.clone(),
            namespace: unit.namespace.clone(),
            chart: "cert-manager-0.2.0".to_string(),
            app_version: "v1.17.2".to_string(),
            status: "deployed".to_string(),
            ..Default::default()
        })];
        // Without a usable target appVersion, compare chart 0.2.0 to chart 0.3.0.
        assert_eq!(cert_manager_release_compatibility(&releases, &[&unit], &plan), FactState::Pass);
        plan.units.get_mut(&unit.key).unwrap().app_version = Some(Version::new(1, 17, 2));
        assert_eq!(cert_manager_release_compatibility(&releases, &[&unit], &plan), FactState::Pass);
        plan.units.get_mut(&unit.key).unwrap().app_version = Some(Version::new(1, 16, 0));
        let FactState::Fail(details) = cert_manager_release_compatibility(&releases, &[&unit], &plan) else {
            panic!("application downgrade must fail");
        };
        assert_eq!(details.get("currentVersion").map(String::as_str), Some("1.17.2"));
        assert_eq!(details.get("expectedVersion").map(String::as_str), Some("1.16.0"));
    }

    #[test]
    fn partial_plans_still_report_cluster_resource_conflicts() {
        let client = mock_kubernetes_client(true, true);
        let (_, discovery) = discover_kubernetes_capabilities(client.clone()).unwrap();
        let plan = plan_with_cluster_role(&unit("qovery"));
        let state = collect_cluster_resource_ownership(client, Some(&discovery), &plan, |_| true);
        assert!(matches!(state, FactState::Fail(_)), "{state:?}");
    }

    #[test]
    fn cert_manager_checks_only_require_its_own_units_to_be_prepared() {
        let client = mock_kubernetes_client(true, false);
        let (_, discovery) = discover_kubernetes_capabilities(client.clone()).unwrap();
        let mut cert_manager = unit("qovery");
        cert_manager.key = "cert-manager".to_string();
        cert_manager.chart.name = "cert-manager".to_string();
        let units = [cert_manager];
        let mut plan = plan_with_cluster_role(&units[0]);
        plan.installed_releases = Some(Vec::new());
        assert_eq!(
            collect_cert_manager_compatibility(client.clone(), Some(&discovery), &units, &plan),
            FactState::Pass
        );
        plan.units.clear();
        plan.failed_units.insert("cert-manager".to_string());
        assert_eq!(
            collect_cert_manager_compatibility(client, Some(&discovery), &units, &plan),
            FactState::Unavailable(evidence([("component", "cert-manager")]))
        );
    }

    #[test]
    fn missing_inventory_reports_the_saturation_evidence() {
        use super::super::plan::HelmInventoryFailure;
        use super::collect_release_ownership;
        let units = [unit("qovery")];
        let mut plan = PreparedPlatformPlan::default();
        assert_eq!(
            collect_release_ownership(&plan, &units),
            FactState::Unavailable(BTreeMap::new())
        );

        plan.inventory_failure = Some(HelmInventoryFailure::TooLarge {
            limit: 1_000,
            scope: "qovery".to_string(),
        });
        let FactState::Unavailable(details) = collect_release_ownership(&plan, &units) else {
            panic!("a saturated inventory must not pass or fail the ownership check");
        };
        assert_eq!(details.get("reason").map(String::as_str), Some("release-inventory-too-large"));
        assert_eq!(details.get("limit").map(String::as_str), Some("1000"));
        assert_eq!(details.get("scope").map(String::as_str), Some("qovery"));
    }

    #[test]
    fn preflight_parses_chart_names_and_full_semantic_versions() {
        for (reference, name, version) in [
            ("qovery-cert-manager-webhook-0.2.0", "qovery-cert-manager-webhook", "0.2.0"),
            ("cert-manager-v1.20.0", "cert-manager", "1.20.0"),
            ("loki-5.48.0-rc.1", "loki", "5.48.0-rc.1"),
            ("loki-v5.48.0-rc.1", "loki", "5.48.0-rc.1"),
            ("loki-5.48.0+build-1", "loki", "5.48.0+build-1"),
        ] {
            let release = InstalledHelmRelease::from(HelmListItem {
                name: "test-release".to_string(),
                namespace: "test-namespace".to_string(),
                chart: reference.to_string(),
                app_version: "v2.9.0-rc.1".to_string(),
                ..Default::default()
            });

            assert_eq!(release.name, "test-release");
            assert_eq!(release.namespace, "test-namespace");
            assert_eq!(release.chart_name, name, "{reference}");
            assert_eq!(release.chart_version, Some(Version::parse(version).unwrap()), "{reference}");
            assert_eq!(release.app_version, Some(Version::parse("2.9.0-rc.1").unwrap()));
        }
    }

    #[test]
    fn preflight_preserves_unversioned_chart_references() {
        for reference in ["chart-without-version", "chart"] {
            let release = InstalledHelmRelease::from(HelmListItem {
                chart: reference.to_string(),
                app_version: "unknown".to_string(),
                ..Default::default()
            });

            assert_eq!(release.chart_name, reference);
            assert_eq!(release.chart_version, None);
            assert_eq!(release.app_version, None);
        }
    }

    #[test]
    fn release_ownership_detects_a_foreign_chart_on_the_planned_release() {
        let releases = vec![InstalledHelmRelease::from(HelmListItem {
            name: "cluster-agent".to_string(),
            namespace: "qovery".to_string(),
            chart: "legacy-agent-1.0.0".to_string(),
            ..Default::default()
        })];

        let state = release_ownership_from(&releases, &[unit("qovery")]);

        let FactState::Fail(details) = state else {
            panic!("a foreign chart must be reported as an ownership conflict");
        };
        assert_eq!(
            details.get("currentOwner"),
            Some(&"qovery/cluster-agent (legacy-agent)".to_string())
        );
        assert_eq!(
            details.get("expectedOwner"),
            Some(&"qovery/cluster-agent (qovery-cluster-agent)".to_string())
        );
    }

    #[test]
    fn release_ownership_accepts_a_prerelease_of_the_planned_chart() {
        let releases = vec![InstalledHelmRelease::from(HelmListItem {
            name: "cluster-agent".to_string(),
            namespace: "qovery".to_string(),
            chart: "qovery-cluster-agent-1.0.0-rc.1".to_string(),
            ..Default::default()
        })];

        assert_eq!(release_ownership_from(&releases, &[unit("qovery")]), FactState::Pass);
    }

    #[test]
    fn cert_manager_prerelease_downgrade_uses_chart_version_when_app_version_is_unknown() {
        let mut cert_manager = unit("qovery");
        cert_manager.key = "cert-manager".to_string();
        cert_manager.release_name = "cert-manager".to_string();
        cert_manager.chart.name = "cert-manager".to_string();
        cert_manager.chart.version = "v1.20.0".to_string();
        let releases = [InstalledHelmRelease::from(HelmListItem {
            name: "cert-manager".to_string(),
            namespace: "qovery".to_string(),
            chart: "cert-manager-v1.21.0-rc.1".to_string(),
            ..Default::default()
        })];

        let FactState::Fail(details) =
            cert_manager_release_compatibility(&releases, &[&cert_manager], &PreparedPlatformPlan::default())
        else {
            panic!("a downgrade from a newer prerelease must fail preflight");
        };
        assert_eq!(details.get("currentVersion").map(String::as_str), Some("1.21.0-rc.1"));
        assert_eq!(details.get("expectedVersion").map(String::as_str), Some("1.20.0"));
    }

    #[test]
    fn cert_manager_check_detects_foreign_owners_and_any_downgrade() {
        let mut cert_manager = unit("qovery");
        cert_manager.key = "cert-manager".to_string();
        cert_manager.release_name = "cert-manager".to_string();
        cert_manager.chart.name = "cert-manager".to_string();
        cert_manager.chart.version = "v1.20.0".to_string();
        let units = [&cert_manager];

        let foreign = vec![InstalledHelmRelease::from(HelmListItem {
            name: "cert-manager".to_string(),
            namespace: "cert-manager".to_string(),
            chart: "cert-manager-1.19.0".to_string(),
            app_version: "v1.19.0".to_string(),
            ..Default::default()
        })];
        assert!(matches!(
            cert_manager_release_compatibility(&foreign, &units, &PreparedPlatformPlan::default()),
            FactState::Fail(_)
        ));

        let newer = vec![InstalledHelmRelease::from(HelmListItem {
            name: "cert-manager".to_string(),
            namespace: "qovery".to_string(),
            chart: "cert-manager-1.21.0".to_string(),
            app_version: "v1.21.0".to_string(),
            ..Default::default()
        })];
        assert!(matches!(
            cert_manager_release_compatibility(&newer, &units, &PreparedPlatformPlan::default()),
            FactState::Fail(_)
        ));

        let unknown_version = vec![InstalledHelmRelease::from(HelmListItem {
            name: "cert-manager".to_string(),
            namespace: "qovery".to_string(),
            chart: "cert-manager".to_string(),
            ..Default::default()
        })];
        assert!(matches!(
            cert_manager_release_compatibility(&unknown_version, &units, &PreparedPlatformPlan::default()),
            FactState::Unavailable(_)
        ));
    }

    #[test]
    fn only_crds_directory_resources_may_exist_without_helm_ownership() {
        let crds_directory_resource = parse_rendered_resources(
            &unit("qovery"),
            r#"
# Source: cert-manager/crds/crds.yaml
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
"#,
            HelmRenderMode::Install,
        )
        .unwrap()
        .remove(0);

        assert!(!has_foreign_cluster_resource_owner(
            &crds_directory_resource,
            None,
            None,
            None,
            None
        ));
        assert!(has_foreign_cluster_resource_owner(
            &crds_directory_resource,
            Some("another-release"),
            Some("qovery"),
            Some("Helm"),
            None,
        ));

        let template_resource = parse_rendered_resources(
            &unit("qovery"),
            r#"
# Source: cert-manager/templates/crds.yaml
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
"#,
            HelmRenderMode::Install,
        )
        .unwrap()
        .remove(0);
        assert!(has_foreign_cluster_resource_owner(&template_resource, None, None, None, None));

        let nested_template_resource = parse_rendered_resources(
            &unit("qovery"),
            r#"
# Source: parent/charts/crds/templates/crds.yaml
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
"#,
            HelmRenderMode::Install,
        )
        .unwrap()
        .remove(0);
        assert!(has_foreign_cluster_resource_owner(
            &nested_template_resource,
            None,
            None,
            None,
            None
        ));
    }

    #[test]
    fn retained_helm_hooks_follow_the_before_creation_deletion_policy() {
        let retained_hook = parse_rendered_resources(
            &unit("qovery"),
            r#"
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: cluster-agent-bootstrap
  annotations:
    helm.sh/hook: pre-install, pre-upgrade
rules: []
"#,
            HelmRenderMode::Install,
        )
        .unwrap()
        .remove(0);

        assert!(!has_foreign_cluster_resource_owner(
            &retained_hook,
            None,
            None,
            Some("Helm"),
            Some("pre-upgrade,pre-install"),
        ));
        assert!(!has_foreign_cluster_resource_owner(
            &retained_hook,
            None,
            None,
            None,
            Some("post-install"),
        ));
        assert!(has_foreign_cluster_resource_owner(
            &retained_hook,
            None,
            None,
            Some("another-manager"),
            Some("pre-install,pre-upgrade"),
        ));

        assert!(!has_foreign_cluster_resource_owner(
            &retained_hook,
            None,
            None,
            None,
            Some("pre-install")
        ));
        let mut crd_hook = retained_hook.clone();
        crd_hook.kind = "CustomResourceDefinition".to_string();
        assert!(has_foreign_cluster_resource_owner(
            &crd_hook,
            None,
            None,
            None,
            Some("pre-install")
        ));

        let hook_kept_before_retry = parse_rendered_resources(
            &unit("qovery"),
            r#"
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: cluster-agent-bootstrap
  annotations:
    helm.sh/hook: pre-install
    helm.sh/hook-delete-policy: hook-succeeded
rules: []
"#,
            HelmRenderMode::Install,
        )
        .unwrap()
        .remove(0);

        assert!(has_foreign_cluster_resource_owner(
            &hook_kept_before_retry,
            None,
            None,
            None,
            Some("pre-install"),
        ));
    }

    #[test]
    fn ownership_check_covers_cluster_scoped_catalog_resources() {
        for kind in ["CustomResourceDefinition", "PriorityClass", "APIService"] {
            assert!(is_owned_cluster_resource(kind));
        }
    }
}
