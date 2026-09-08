//! Regression tests across preflight boundaries, including rendering with the Helm binary.

use super::connectivity::{probe_container_registries, probe_http_endpoints};
use super::evaluation::evaluate;
use super::kubernetes::collect_rbac;
use super::model::{FactState, PlatformPreflightFacts, evidence};
use super::ownership::{
    InstalledHelmRelease, collect_cluster_resource_ownership, has_foreign_cluster_resource_owner,
    release_ownership_from,
};
use super::plan::{
    HELM_LIST_TIMEOUT, HELM_TEMPLATE_TIMEOUT, HelmRenderMode, discover_kubernetes_capabilities,
    parse_rendered_resources,
};
use super::test_support::{check, mock_kubernetes_client, plan_with_cluster_role, request, unit};
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{Helm, HelmInventoryError};
use crate::io_models::platform_components::{
    PlatformPreflightCheckId, PlatformPreflightCheckSeverity, PlatformPreflightCheckStatus, PlatformPreflightMode,
    PlatformPreflightReasonCode,
};
use serde_json::{Value, json};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

#[test]
fn preflight_render_matches_install_and_upgrade_resources_with_real_helm() {
    let fixture = tempfile::tempdir().unwrap();
    fs::create_dir(fixture.path().join("templates")).unwrap();
    fs::create_dir(fixture.path().join("crds")).unwrap();
    fs::write(
        fixture.path().join("Chart.yaml"),
        "apiVersion: v2\nname: phase-check\nversion: 1.0.0\n",
    )
    .unwrap();
    fs::write(
        fixture.path().join("templates/resource.yaml"),
        "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {{ if .Release.IsUpgrade }}upgrade-only{{ else }}install-only{{ end }}\n",
    )
    .unwrap();
    fs::write(
        fixture.path().join("crds/example.yaml"),
        "apiVersion: apiextensions.k8s.io/v1\nkind: CustomResourceDefinition\nmetadata:\n  name: examples.example.com\nspec:\n  group: example.com\n  scope: Namespaced\n  names:\n    kind: Example\n    plural: examples\n  versions:\n    - name: v1\n      served: true\n      storage: true\n",
    )
    .unwrap();
    let (capabilities, _) = discover_kubernetes_capabilities(mock_kubernetes_client(true, false)).unwrap();
    let helm = Helm::new(Option::<&Path>::None, &[]).unwrap();
    for (mode, expected, count) in [
        (HelmRenderMode::Install, "install-only", 2),
        (HelmRenderMode::Upgrade, "upgrade-only", 1),
    ] {
        let yaml = helm
            .template_raw_silent(
                "phase-check",
                fixture.path(),
                "qovery",
                &capabilities.template_args(mode),
                &[],
                &CommandKiller::from_timeout(HELM_TEMPLATE_TIMEOUT),
                &mut |_| {},
            )
            .unwrap();
        let resources = parse_rendered_resources(&unit("qovery"), &yaml, mode).unwrap();
        assert_eq!(resources.len(), count, "{mode:?}");
        assert!(
            resources
                .iter()
                .any(|resource| resource.name.as_deref() == Some(expected))
        );
    }
}

#[test]
fn prometheus_adapter_preflight_renders_the_cluster_apiservice_version() {
    let (capabilities, discovery) = discover_kubernetes_capabilities(mock_kubernetes_client(true, false)).unwrap();
    let helm = Helm::new(Option::<&Path>::None, &[]).unwrap();
    let yaml = helm
        .template_raw_silent(
            "adapter",
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("lib/common/bootstrap/charts/prometheus-adapter"),
            "qovery",
            &capabilities.template_args(HelmRenderMode::Install),
            &[],
            &CommandKiller::from_timeout(HELM_TEMPLATE_TIMEOUT),
            &mut |_| {},
        )
        .unwrap();
    let resources = parse_rendered_resources(&unit("qovery"), &yaml, HelmRenderMode::Install).unwrap();
    let api_service = resources.iter().find(|resource| resource.kind == "APIService").unwrap();
    assert_eq!(api_service.api_version, "apiregistration.k8s.io/v1");
    assert!(discovery.resolve_gvk(&api_service.gvk()).is_some());
}

#[test]
fn helm_rendering_receives_the_cluster_version_and_kind_capabilities() {
    let (capabilities, _) = discover_kubernetes_capabilities(mock_kubernetes_client(true, false)).unwrap();
    let chart = tempfile::tempdir().unwrap();
    fs::create_dir(chart.path().join("templates")).unwrap();
    fs::write(
        chart.path().join("Chart.yaml"),
        "apiVersion: v2\nname: capabilities\nversion: 1.0.0\n",
    )
    .unwrap();
    fs::write(
        chart.path().join("templates/capabilities.yaml"),
        r#"
apiVersion: v1
kind: ConfigMap
metadata:
  name: capabilities
data:
  version: {{ .Capabilities.KubeVersion.Version | quote }}
  customKind: {{ .Capabilities.APIVersions.Has "example.com/v1beta1/Example" | quote }}
  missingKind: {{ .Capabilities.APIVersions.Has "missing.example.com/v1/Example" | quote }}
"#,
    )
    .unwrap();
    let helm = Helm::new(Option::<&Path>::None, &[]).unwrap();
    let yaml = helm
        .template_raw_silent(
            "capabilities",
            chart.path(),
            "qovery",
            &capabilities.template_args(HelmRenderMode::Install),
            &[],
            &CommandKiller::from_timeout(HELM_TEMPLATE_TIMEOUT),
            &mut |_| {},
        )
        .unwrap();
    let resource: Value = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(resource["data"]["version"], "v1.34.3");
    assert_eq!(resource["data"]["customKind"], "true");
    assert_eq!(resource["data"]["missingKind"], "false");
}

#[test]
fn crd_source_is_preserved_across_documents_and_reset_for_templates() {
    let chart = tempfile::tempdir().unwrap();
    fs::create_dir(chart.path().join("crds")).unwrap();
    fs::create_dir(chart.path().join("templates")).unwrap();
    fs::write(
        chart.path().join("Chart.yaml"),
        "apiVersion: v2\nname: crd-origin\nversion: 1.0.0\n",
    )
    .unwrap();
    let crd = |name| {
        format!(
            "apiVersion: apiextensions.k8s.io/v1\nkind: CustomResourceDefinition\nmetadata:\n  name: {name}.example.com\nspec:\n  group: example.com\n  names:\n    kind: Example\n    plural: {name}\n  scope: Namespaced\n  versions:\n    - name: v1\n      served: true\n      storage: true\n"
        )
    };
    fs::write(
        chart.path().join("crds/bundle.yaml"),
        format!("---\n{}\n---\n{}", crd("first"), crd("second")),
    )
    .unwrap();
    fs::write(
        chart.path().join("templates/crds.yaml"),
        format!("{}\n---\n{}", crd("third"), crd("fourth")),
    )
    .unwrap();
    let helm = Helm::new(Option::<&Path>::None, &[]).unwrap();
    let yaml = helm
        .template_raw_silent(
            "crd-origin",
            chart.path(),
            "qovery",
            &["--include-crds"],
            &[],
            &CommandKiller::from_timeout(HELM_TEMPLATE_TIMEOUT),
            &mut |_| {},
        )
        .unwrap();
    let resources = parse_rendered_resources(&unit("qovery"), &yaml, HelmRenderMode::Install).unwrap();
    assert_eq!(resources.len(), 4);
    for resource in resources {
        let raw_crd = matches!(resource.name.as_deref(), Some("first.example.com" | "second.example.com"));
        assert_eq!(resource.installed_without_helm_ownership, raw_crd, "{:?}", resource.name);
        assert_eq!(has_foreign_cluster_resource_owner(&resource, None, None, None, None), !raw_crd);
    }
}

#[cfg(unix)]
#[test]
fn preflight_reads_large_release_inventory_once_and_preserves_legacy_lookup() {
    let fixture = tempfile::tempdir().unwrap();
    let legacy: Vec<_> = (0..256)
        .map(|index| {
            json!({
                "name": format!("app-{index}"), "namespace": "apps", "chart": "app-1.0.0",
                "app_version": "1.0.0", "revision": "1", "updated": "", "status": "deployed"
            })
        })
        .collect();
    fs::write(fixture.path().join("legacy.json"), serde_json::to_vec(&legacy).unwrap()).unwrap();
    let unit = unit("qovery");
    let mut complete = legacy.clone();
    complete.extend((256..600).map(|index| {
        json!({
            "name": format!("app-{index}"), "namespace": "apps", "chart": "app-1.0.0",
            "app_version": "1.0.0", "revision": "1", "updated": "", "status": "deployed"
        })
    }));
    complete.push(json!({
        "name": unit.release_name, "namespace": unit.namespace, "chart": "foreign-1.0.0",
        "app_version": "1.0.0", "revision": "1", "updated": "", "status": "deployed"
    }));
    fs::write(fixture.path().join("complete.json"), serde_json::to_vec(&complete).unwrap()).unwrap();
    let saturated: Vec<_> = (0..1000)
        .map(|index| {
            json!({
                "name": format!("app-{index}"), "namespace": "apps", "chart": "app-1.0.0",
                "app_version": "1.0.0", "revision": "1", "updated": "", "status": "deployed"
            })
        })
        .collect();
    fs::write(fixture.path().join("saturated.json"), serde_json::to_vec(&saturated).unwrap()).unwrap();
    let executable = fixture.path().join("helm");
    fs::write(
        &executable,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$PREFLIGHT_FIXTURE/calls"
case "$*" in
  'list -a -o json -A') cat "$PREFLIGHT_FIXTURE/legacy.json" ;;
  'list -a -o json -A --max 1000'|'list -a -o json -n qovery --max 1000')
    if [ "$FAIL_INVENTORY" = 1 ]; then exit 1; fi
    if [ "$SATURATE_INVENTORY" = 1 ]; then cat "$PREFLIGHT_FIXTURE/saturated.json"; else
    cat "$PREFLIGHT_FIXTURE/complete.json"; fi ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:/usr/bin:/bin", fixture.path().display());
    let envs = [
        ("PATH", path.as_str()),
        ("PREFLIGHT_FIXTURE", fixture.path().to_str().unwrap()),
    ];
    let helm = Helm::new(Option::<&Path>::None, &[]).unwrap();
    assert_eq!(helm.list_release(None, &envs).unwrap().len(), 256);
    let releases = helm
        .list_all_releases_raw(None, &envs, &CommandKiller::from_timeout(HELM_LIST_TIMEOUT))
        .unwrap();
    assert_eq!(releases.len(), 601);
    let releases: Vec<_> = releases.into_iter().map(InstalledHelmRelease::from).collect();
    assert!(matches!(release_ownership_from(&releases, &[unit]), FactState::Fail(_)));
    assert_eq!(
        fs::read_to_string(fixture.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert!(
        helm.list_all_releases_raw(Some("qovery"), &envs, &CommandKiller::from_timeout(HELM_LIST_TIMEOUT))
            .is_ok()
    );
    // A truncated inventory must never be reported as a complete one, and must stay
    // distinguishable from a Helm failure so the limit can be reported.
    let mut saturated_envs = envs.to_vec();
    saturated_envs.push(("SATURATE_INVENTORY", "1"));
    assert!(matches!(
        helm.list_all_releases_raw(None, &saturated_envs, &CommandKiller::from_timeout(HELM_LIST_TIMEOUT)),
        Err(HelmInventoryError::Saturated { limit: 1_000 })
    ));
    let mut failing_envs = envs.to_vec();
    failing_envs.push(("FAIL_INVENTORY", "1"));
    assert!(matches!(
        helm.list_all_releases_raw(None, &failing_envs, &CommandKiller::from_timeout(HELM_LIST_TIMEOUT)),
        Err(HelmInventoryError::Helm(_))
    ));
    assert!(matches!(
        helm.list_all_releases_raw(None, &envs, &CommandKiller::from_timeout(Duration::ZERO)),
        Err(HelmInventoryError::Helm(_))
    ));
    // An expired budget must not launch another Helm process.
    assert_eq!(
        fs::read_to_string(fixture.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        5
    );
}

#[test]
fn partial_plans_do_not_pass_global_resource_or_rbac_checks() {
    let client = mock_kubernetes_client(true, false);
    let (_, discovery) = discover_kubernetes_capabilities(client.clone()).unwrap();
    let units = [unit("qovery")];
    let plan = plan_with_cluster_role(&units[0]);
    let expected = FactState::Unavailable(evidence([("component", "unrelated-chart")]));
    assert_eq!(
        collect_cluster_resource_ownership(client.clone(), Some(&discovery), &plan, |_| true),
        expected
    );
    assert_eq!(collect_rbac(client, Some(&discovery), &plan, &units), expected);
}

#[test]
fn missing_connectivity_inputs_are_not_reported_as_success() {
    assert!(matches!(probe_http_endpoints(&[]), FactState::Unavailable(_)));
    assert!(matches!(
        probe_container_registries(&[unit("qovery")]),
        FactState::Unavailable(_)
    ));

    let request = request(
        PlatformPreflightMode::Observe,
        check(
            PlatformPreflightCheckId::ContainerRegistryUnreachable,
            PlatformPreflightCheckSeverity::Advisory,
        ),
    );
    let outcome = evaluate(
        &request,
        &[unit("qovery")],
        &PlatformPreflightFacts {
            container_registries: FactState::Unavailable(evidence([("input", "images")])),
            ..Default::default()
        },
    );
    assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::NotEvaluated);
    assert_eq!(
        outcome.results[0].reason_code,
        PlatformPreflightReasonCode::ContainerRegistryNotConfigured
    );
}
