//! Shared fixtures for preflight unit and integration tests.

use super::plan::{HelmRenderMode, PreparedPlatformPlan, PreparedPlatformUnit, parse_rendered_resources};
use crate::io_models::platform_components::{
    PlatformHelmChartSource, PlatformHelmUnit, PlatformHelmUnitAction, PlatformPreflightCheckId,
    PlatformPreflightCheckRequest, PlatformPreflightCheckSeverity, PlatformPreflightMode, PlatformPreflightRequest,
};
use crate::runtime::block_on;
use http::{Request, Response};
use k8s_openapi::apimachinery::pkg::version::Info;
use kube::client::Body;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use tempfile::NamedTempFile;
use tower::service_fn;

pub(super) fn mock_kubernetes_client(allow_rbac: bool, existing_cluster_role: bool) -> kube::Client {
    let groups = [
        ("apiregistration.k8s.io", "APIService", "apiservices"),
        ("rbac.authorization.k8s.io", "ClusterRole", "clusterroles"),
        ("example.com", "Example", "examples"),
    ];
    let service = service_fn(move |request: Request<Body>| async move {
        let path = request.uri().path();
        let (status, body) = match path {
            "/version" => (
                200,
                serde_json::to_value(Info {
                    git_version: "v1.34.3".to_string(),
                    major: "1".to_string(),
                    minor: "34".to_string(),
                    ..Default::default()
                })
                .unwrap(),
            ),
            "/apis" => (
                200,
                json!({
                    "kind": "APIGroupList", "apiVersion": "v1",
                    "groups": groups.iter().map(|(group, _, _)| json!({
                        "name": group,
                        "versions": [{"groupVersion": format!("{group}/v1"), "version": "v1"},
                                     {"groupVersion": format!("{group}/v1beta1"), "version": "v1beta1"}],
                        "preferredVersion": {"groupVersion": format!("{group}/v1"), "version": "v1"}
                    })).collect::<Vec<_>>()
                }),
            ),
            "/api" => (200, json!({"kind": "APIVersions", "apiVersion": "v1", "versions": ["v1"]})),
            "/api/v1" => (
                200,
                json!({"kind": "APIResourceList", "apiVersion": "v1", "groupVersion": "v1",
                    "resources": [{"name": "configmaps", "kind": "ConfigMap", "namespaced": true, "verbs": ["get", "create"]}]
                }),
            ),
            "/apis/authorization.k8s.io/v1/selfsubjectaccessreviews" => (
                200,
                json!({
                    "kind": "SelfSubjectAccessReview", "apiVersion": "authorization.k8s.io/v1",
                    "spec": {}, "status": {"allowed": allow_rbac}
                }),
            ),
            "/apis/rbac.authorization.k8s.io/v1/clusterroles/agent" if existing_cluster_role => (
                200,
                json!({
                    "kind": "ClusterRole", "apiVersion": "rbac.authorization.k8s.io/v1",
                    "metadata": {"name": "agent", "annotations": {"meta.helm.sh/release-name": "foreign"}}
                }),
            ),
            _ => {
                let group_resource = groups.iter().find_map(|(group, kind, plural)| {
                        ["v1", "v1beta1"].into_iter().find_map(|version| {
                            (path == format!("/apis/{group}/{version}")).then(|| json!({
                                "kind": "APIResourceList", "apiVersion": "v1", "groupVersion": format!("{group}/{version}"),
                                "resources": [{"name": plural, "kind": kind, "namespaced": false, "verbs": ["get", "create"]}]
                            }))
                        })
                    });
                match group_resource {
                    Some(body) => (200, body),
                    None => (
                        404,
                        json!({"kind": "Status", "apiVersion": "v1", "status": "Failure", "message": "not found", "reason": "NotFound", "code": 404}),
                    ),
                }
            }
        };
        Ok::<_, Infallible>(
            Response::builder()
                .status(status)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
    });
    block_on(async { kube::Client::new(service, "qovery") })
}

pub(super) fn plan_with_cluster_role(unit: &PlatformHelmUnit) -> PreparedPlatformPlan {
    let resources = parse_rendered_resources(
        unit,
        "apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  name: agent\n",
        HelmRenderMode::Install,
    )
    .unwrap();
    PreparedPlatformPlan {
        units: BTreeMap::from([(
            unit.key.clone(),
            PreparedPlatformUnit {
                chart_dir: String::new(),
                values_file: NamedTempFile::new().unwrap(),
                app_version: None,
                resources,
            },
        )]),
        failed_units: BTreeSet::from(["unrelated-chart".to_string()]),
        ..Default::default()
    }
}

pub(super) fn unit(namespace: &str) -> PlatformHelmUnit {
    PlatformHelmUnit {
        key: "cluster-agent".to_string(),
        action: PlatformHelmUnitAction::Create,
        release_name: "cluster-agent".to_string(),
        namespace: namespace.to_string(),
        chart: PlatformHelmChartSource {
            repository: "https://helm.qovery.com".to_string(),
            name: "qovery-cluster-agent".to_string(),
            version: "1.0.0".to_string(),
        },
        values_yaml: "{}".to_string(),
        images: Vec::new(),
        preflight_requirements: BTreeSet::new(),
    }
}

pub(super) fn check(
    id: PlatformPreflightCheckId,
    severity: PlatformPreflightCheckSeverity,
) -> PlatformPreflightCheckRequest {
    PlatformPreflightCheckRequest { id, severity }
}

pub(super) fn request(mode: PlatformPreflightMode, check: PlatformPreflightCheckRequest) -> PlatformPreflightRequest {
    PlatformPreflightRequest {
        mode,
        checks: vec![check],
        qovery_endpoints: Vec::new(),
    }
}
