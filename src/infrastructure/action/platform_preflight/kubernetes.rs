//! Read-only Kubernetes probes and access reviews for the prepared platform plan.

use super::connectivity::resolve_socket_addresses;
use super::model::{
    FactState, NamespaceState, PlannedApiResource, PlannedKubernetesResource, evidence, target_namespaces,
};
use super::plan::{PreparedPlatformPlan, plan_failure_evidence, resolve_planned_custom_resource};
use super::runtime::{KUBERNETES_READ_TIMEOUT, block_on_timeout};
use crate::io_models::platform_components::{PlatformHelmUnit, PlatformPreflightRequirement};
use futures::stream::{self, StreamExt};
use k8s_openapi::api::authorization::v1::{ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec};
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::api::storage::v1::StorageClass;
use kube::api::{Api, ListParams, PostParams};
use kube::discovery::{Discovery, Scope};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const RBAC_CHECK_BUDGET: Duration = Duration::from_secs(15);

const RBAC_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

const RBAC_REVIEW_CONCURRENCY: usize = 16;

const MAX_RBAC_REQUIREMENTS: usize = 256;

const DEFAULT_STORAGE_CLASS_ANNOTATION: &str = "storageclass.kubernetes.io/is-default-class";

const BETA_DEFAULT_STORAGE_CLASS_ANNOTATION: &str = "storageclass.beta.kubernetes.io/is-default-class";

const KUBERNETES_SERVICE_DNS_ADDRESS: &str = "kubernetes.default:443";

const KUBERNETES_SERVICE_DNS_NAME: &str = "kubernetes.default";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RbacRequirement {
    group: String,
    version: String,
    resource: String,
    namespace: Option<String>,
    name: Option<String>,
    verb: String,
}

pub(super) fn namespace_state_from_lookup(result: Result<Option<Namespace>, kube::Error>) -> NamespaceState {
    match result {
        Ok(Some(resource)) if resource.metadata.deletion_timestamp.is_some() => NamespaceState::Terminating,
        Ok(_) => NamespaceState::Available,
        Err(kube::Error::Api(error)) if error.code == 403 => NamespaceState::Forbidden,
        Err(_) => NamespaceState::Unavailable,
    }
}

pub(super) fn collect_default_storage_class(client: kube::Client, units: &[PlatformHelmUnit]) -> FactState {
    let required_by = units.iter().find(|unit| {
        unit.preflight_requirements
            .contains(&PlatformPreflightRequirement::DefaultStorageClass)
    });
    let Some(required_by) = required_by else {
        return FactState::NotApplicable;
    };
    let api: Api<StorageClass> = Api::all(client);
    match block_on_timeout(KUBERNETES_READ_TIMEOUT, api.list(&ListParams::default())) {
        Ok(Ok(classes)) => classes
            .items
            .iter()
            .find(|storage_class| is_default_storage_class(storage_class))
            .map(|_| FactState::Pass)
            .unwrap_or_else(|| {
                FactState::Fail(evidence([
                    ("component", required_by.key.as_str()),
                    ("requirement", "DEFAULT_STORAGE_CLASS"),
                ]))
            }),
        Ok(Err(_)) | Err(_) => FactState::Unavailable(evidence([("component", required_by.key.as_str())])),
    }
}

fn is_default_storage_class(storage_class: &StorageClass) -> bool {
    storage_class.metadata.annotations.as_ref().is_some_and(|annotations| {
        [DEFAULT_STORAGE_CLASS_ANNOTATION, BETA_DEFAULT_STORAGE_CLASS_ANNOTATION]
            .iter()
            .any(|key| {
                annotations
                    .get(*key)
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
            })
    })
}

pub(super) fn collect_cluster_dns(client: kube::Client, runs_inside_cluster: bool) -> FactState {
    let endpoint_slices: Api<EndpointSlice> = Api::namespaced(client, "kube-system");
    let slices = match block_on_timeout(
        KUBERNETES_READ_TIMEOUT,
        endpoint_slices.list(&ListParams::default().labels("kubernetes.io/service-name=kube-dns")),
    ) {
        Ok(Ok(slices)) => slices,
        Ok(Err(_)) | Err(_) => return FactState::Unavailable(BTreeMap::new()),
    };
    let endpoint_state = cluster_dns_endpoint_state(&slices.items);
    if !matches!(endpoint_state, FactState::Pass) {
        return endpoint_state;
    }
    if !runs_inside_cluster {
        return FactState::Unavailable(evidence([("vantage", "outside-cluster")]));
    }
    let resolved =
        resolve_socket_addresses(KUBERNETES_SERVICE_DNS_ADDRESS).is_some_and(|addresses| !addresses.is_empty());
    if resolved {
        FactState::Pass
    } else {
        FactState::Fail(evidence([("lookup", KUBERNETES_SERVICE_DNS_NAME)]))
    }
}

pub(super) fn runs_inside_kubernetes() -> bool {
    std::env::var_os("KUBERNETES_SERVICE_HOST").is_some() && std::env::var_os("KUBERNETES_SERVICE_PORT").is_some()
}

fn cluster_dns_endpoint_state(slices: &[EndpointSlice]) -> FactState {
    if slices.is_empty() {
        return FactState::Unavailable(evidence([("service", "kube-system/kube-dns")]));
    }
    let ready_endpoint = slices.iter().flat_map(|slice| &slice.endpoints).any(|endpoint| {
        endpoint
            .conditions
            .as_ref()
            .and_then(|conditions| conditions.ready)
            .unwrap_or(true)
    });
    if !ready_endpoint {
        return FactState::Fail(evidence([("service", "kube-system/kube-dns")]));
    }
    FactState::Pass
}

pub(super) fn collect_rbac(
    client: kube::Client,
    discovery: Option<&Discovery>,
    plan: &PreparedPlatformPlan,
    units: &[PlatformHelmUnit],
) -> FactState {
    let Some(discovery) = discovery else {
        return FactState::Unavailable(plan_failure_evidence(plan));
    };
    let reviews: Api<SelfSubjectAccessReview> = Api::all(client);
    let mut requirements = BTreeSet::new();
    let mut completion = plan.completeness(|_| true);
    for planned in plan.resources() {
        let Some(resource) = resolve_planned_api_resource(discovery, plan, planned) else {
            completion = FactState::Unavailable(evidence([("resource", planned.display_name().as_str())]));
            continue;
        };
        let namespace = if resource.namespaced {
            planned
                .namespace
                .as_deref()
                .or(Some(planned.release_namespace.as_str()))
        } else {
            None
        };
        for verb in ["get", "create", "update", "patch", "delete"] {
            requirements.insert(RbacRequirement {
                group: resource.group.clone(),
                version: resource.version.clone(),
                resource: resource.plural.clone(),
                namespace: namespace.map(str::to_string),
                name: None,
                verb: verb.to_string(),
            });
        }
        if matches!(planned.kind.as_str(), "Role" | "ClusterRole") {
            requirements.insert(RbacRequirement {
                group: resource.group.clone(),
                version: resource.version.clone(),
                resource: resource.plural.clone(),
                namespace: namespace.map(str::to_string),
                name: planned.name.clone(),
                verb: "escalate".to_string(),
            });
        }
        if let Some(role_reference) = &planned.role_reference {
            let (resource, namespace) = match role_reference.kind.as_str() {
                "Role" => (
                    "roles",
                    planned
                        .namespace
                        .as_deref()
                        .or(Some(planned.release_namespace.as_str()))
                        .map(str::to_string),
                ),
                "ClusterRole" => ("clusterroles", None),
                _ => continue,
            };
            requirements.insert(RbacRequirement {
                group: role_reference.api_group.clone(),
                version: "v1".to_string(),
                resource: resource.to_string(),
                namespace,
                name: Some(role_reference.name.clone()),
                verb: "bind".to_string(),
            });
        }
    }
    for namespace in target_namespaces(units) {
        for verb in ["get", "list", "create", "update", "patch", "delete"] {
            requirements.insert(RbacRequirement {
                group: String::new(),
                version: "v1".to_string(),
                resource: "secrets".to_string(),
                namespace: Some(namespace.to_string()),
                name: None,
                verb: verb.to_string(),
            });
        }
        requirements.insert(RbacRequirement {
            group: String::new(),
            version: "v1".to_string(),
            resource: "namespaces".to_string(),
            namespace: None,
            name: Some(namespace.to_string()),
            verb: "get".to_string(),
        });
    }
    if requirements.len() > MAX_RBAC_REQUIREMENTS {
        return FactState::Unavailable(evidence([
            ("requirementCount", requirements.len().to_string().as_str()),
            ("limit", MAX_RBAC_REQUIREMENTS.to_string().as_str()),
        ]));
    }
    let reviews_stream = stream::iter(requirements.into_iter().map(|requirement| {
        let reviews = reviews.clone();
        async move {
            let review = SelfSubjectAccessReview {
                spec: SelfSubjectAccessReviewSpec {
                    resource_attributes: Some(ResourceAttributes {
                        group: Some(requirement.group.clone()),
                        version: Some(requirement.version.clone()),
                        resource: Some(requirement.resource.clone()),
                        namespace: requirement.namespace.clone(),
                        name: requirement.name.clone(),
                        verb: Some(requirement.verb.clone()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            };
            let outcome =
                tokio::time::timeout(RBAC_REQUEST_TIMEOUT, reviews.create(&PostParams::default(), &review)).await;
            (requirement, outcome)
        }
    }))
    .buffer_unordered(RBAC_REVIEW_CONCURRENCY)
    .collect::<Vec<_>>();
    let outcomes = match block_on_timeout(RBAC_CHECK_BUDGET, reviews_stream) {
        Ok(outcomes) => outcomes,
        Err(_) => return FactState::Unavailable(evidence([("reason", "time-budget-exhausted")])),
    };
    let decisions = outcomes
        .into_iter()
        .map(|(requirement, outcome)| {
            let decision = match outcome {
                Ok(Ok(answer)) => match answer.status {
                    Some(status) if status.allowed => RbacDecision::Allowed,
                    Some(_) => RbacDecision::Denied,
                    None => RbacDecision::Unavailable,
                },
                _ => RbacDecision::Unavailable,
            };
            (requirement, decision)
        })
        .collect();
    evaluate_rbac_decisions(decisions, completion)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RbacDecision {
    Allowed,
    Denied,
    Unavailable,
}

fn evaluate_rbac_decisions(mut decisions: Vec<(RbacRequirement, RbacDecision)>, completion: FactState) -> FactState {
    decisions.sort_by(|(left, _), (right, _)| left.cmp(right));
    // A definite denial takes precedence over ambiguous bind/escalate answers, regardless of API group order.
    if let Some((requirement, _)) = decisions
        .iter()
        .filter(|(_, decision)| *decision == RbacDecision::Denied)
        .min_by_key(|(requirement, _)| matches!(requirement.verb.as_str(), "bind" | "escalate"))
    {
        let namespace = requirement.namespace.as_deref().unwrap_or("<cluster>");
        let details = evidence([
            ("verb", requirement.verb.as_str()),
            ("resource", requirement.resource.as_str()),
            ("apiGroup", requirement.group.as_str()),
            ("namespace", namespace),
        ]);
        // These verbs are alternatives to already holding the permissions carried by the role.
        if matches!(requirement.verb.as_str(), "bind" | "escalate") {
            return FactState::Unavailable(details);
        }
        return FactState::Fail(details);
    }
    if decisions
        .iter()
        .any(|(_, decision)| *decision == RbacDecision::Unavailable)
    {
        return FactState::Unavailable(BTreeMap::new());
    }
    completion
}

fn resolve_planned_api_resource(
    discovery: &Discovery,
    plan: &PreparedPlatformPlan,
    planned: &PlannedKubernetesResource,
) -> Option<PlannedApiResource> {
    if let Some((resource, capabilities)) = discovery.resolve_gvk(&planned.gvk()) {
        return Some(PlannedApiResource {
            group: resource.group.clone(),
            version: resource.version.clone(),
            plural: resource.plural.clone(),
            namespaced: capabilities.scope == Scope::Namespaced,
        });
    }
    let gvk = planned.gvk();
    resolve_planned_custom_resource(plan, &gvk)
}

#[cfg(test)]
mod tests {
    use super::super::model::{FactState, NamespaceState};
    use super::super::plan::discover_kubernetes_capabilities;
    use super::super::test_support::{mock_kubernetes_client, plan_with_cluster_role, unit};
    use super::{
        BETA_DEFAULT_STORAGE_CLASS_ANNOTATION, DEFAULT_STORAGE_CLASS_ANNOTATION, KUBERNETES_SERVICE_DNS_ADDRESS,
        cluster_dns_endpoint_state, collect_rbac, is_default_storage_class, namespace_state_from_lookup,
    };
    use k8s_openapi::api::storage::v1::StorageClass;
    use std::collections::BTreeMap;

    #[test]
    fn definite_rbac_denials_take_precedence_over_bind_and_escalate() {
        use super::{RbacDecision, RbacRequirement, evaluate_rbac_decisions};
        let requirement = |group: &str, resource: &str, verb: &str| RbacRequirement {
            group: group.to_string(),
            version: "v1".to_string(),
            resource: resource.to_string(),
            namespace: None,
            name: None,
            verb: verb.to_string(),
        };
        for ambiguous_verb in ["bind", "escalate"] {
            let ambiguous = requirement("rbac.authorization.k8s.io", "clusterroles", ambiguous_verb);
            assert!(matches!(
                evaluate_rbac_decisions(vec![(ambiguous.clone(), RbacDecision::Denied)], FactState::Pass),
                FactState::Unavailable(_)
            ));
            let denied = requirement("storage.k8s.io", "storageclasses", "create");
            for reverse in [false, true] {
                let mut decisions = vec![
                    (ambiguous.clone(), RbacDecision::Denied),
                    (denied.clone(), RbacDecision::Denied),
                ];
                if reverse {
                    decisions.reverse();
                }
                let FactState::Fail(details) = evaluate_rbac_decisions(decisions, FactState::Pass) else {
                    panic!("an explicit create denial must not be masked");
                };
                assert_eq!(details.get("resource").map(String::as_str), Some("storageclasses"));
                assert_eq!(details.get("verb").map(String::as_str), Some("create"));
            }
        }
    }

    #[test]
    fn partial_plans_still_report_denied_rbac() {
        let client = mock_kubernetes_client(false, false);
        let (_, discovery) = discover_kubernetes_capabilities(client.clone()).unwrap();
        let units = [unit("qovery")];
        let plan = plan_with_cluster_role(&units[0]);
        assert!(matches!(
            collect_rbac(client, Some(&discovery), &plan, &units),
            FactState::Fail(_)
        ));
    }

    #[test]
    fn kubernetes_service_dns_lookup_uses_the_pod_search_domains() {
        assert_eq!(KUBERNETES_SERVICE_DNS_ADDRESS, "kubernetes.default:443");
        assert!(!KUBERNETES_SERVICE_DNS_ADDRESS.contains("cluster.local"));
    }

    #[test]
    fn default_storage_class_accepts_stable_and_beta_annotations() {
        for annotation in [DEFAULT_STORAGE_CLASS_ANNOTATION, BETA_DEFAULT_STORAGE_CLASS_ANNOTATION] {
            let storage_class = StorageClass {
                metadata: kube::core::ObjectMeta {
                    annotations: Some(BTreeMap::from([(annotation.to_string(), "TRUE".to_string())])),
                    ..Default::default()
                },
                ..Default::default()
            };

            assert!(is_default_storage_class(&storage_class));
        }
    }

    #[test]
    fn missing_cluster_dns_endpoint_slices_are_not_a_health_failure() {
        assert!(matches!(cluster_dns_endpoint_state(&[]), FactState::Unavailable(_)));
    }

    #[test]
    fn forbidden_namespace_lookup_is_distinct_from_an_unavailable_lookup() {
        assert_eq!(
            namespace_state_from_lookup(Err(kube::Error::Api(Box::new(kube::error::ErrorResponse {
                code: 403,
                ..Default::default()
            })))),
            NamespaceState::Forbidden
        );
        assert_eq!(
            namespace_state_from_lookup(Err(kube::Error::Api(Box::new(kube::error::ErrorResponse {
                code: 500,
                ..Default::default()
            })))),
            NamespaceState::Unavailable
        );
    }
}
