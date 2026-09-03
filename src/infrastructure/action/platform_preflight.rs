use crate::infrastructure::infrastructure_context::{InfrastructureContext, KubeClientAuthMode};
use crate::io_models::platform_components::{
    PlatformHelmUnit, PlatformPreflightCheckId, PlatformPreflightCheckRequest, PlatformPreflightCheckResult,
    PlatformPreflightCheckSeverity, PlatformPreflightCheckStatus, PlatformPreflightMode, PlatformPreflightReasonCode,
    PlatformPreflightRemediation, PlatformPreflightRemediationKey, PlatformPreflightRequest,
};
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::Namespace;
use kube::Api;
use std::collections::{BTreeMap, BTreeSet};

/// Result of the non-mutating preflight phase, kept independent from the Helm executor.
pub(super) struct PlatformPreflightOutcome {
    pub results: Vec<PlatformPreflightCheckResult>,
    pub blocks_execution: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NamespaceState {
    Available,
    Terminating,
    Forbidden,
    Unavailable,
}

#[derive(Default)]
struct PlatformPreflightFacts {
    kubernetes_api_reachable: Option<bool>,
    namespaces: BTreeMap<String, NamespaceState>,
}

/// Runs only read operations and evaluates the requested checks in their original order.
pub(super) fn run_platform_preflight(
    infra_ctx: &InfrastructureContext,
    request: &PlatformPreflightRequest,
    units: &[PlatformHelmUnit],
) -> PlatformPreflightOutcome {
    let facts = collect_facts(infra_ctx, request, units);
    evaluate(request, units, &facts)
}

fn collect_facts(
    infra_ctx: &InfrastructureContext,
    request: &PlatformPreflightRequest,
    units: &[PlatformHelmUnit],
) -> PlatformPreflightFacts {
    let needs_kubernetes_api = request.checks.iter().any(|check| {
        matches!(
            check.id,
            PlatformPreflightCheckId::KubernetesApiUnreachable | PlatformPreflightCheckId::NamespaceTerminating
        )
    });
    if !needs_kubernetes_api {
        return PlatformPreflightFacts::default();
    }

    let kube_client = match infra_ctx.mk_kube_client_with_auth_mode(KubeClientAuthMode::AllowInCluster) {
        Ok(client) => client,
        Err(_) => {
            return PlatformPreflightFacts {
                kubernetes_api_reachable: Some(false),
                namespaces: BTreeMap::new(),
            };
        }
    };

    let mut facts = PlatformPreflightFacts {
        kubernetes_api_reachable: Some(true),
        namespaces: BTreeMap::new(),
    };
    if !request
        .checks
        .iter()
        .any(|check| check.id == PlatformPreflightCheckId::NamespaceTerminating)
    {
        return facts;
    }

    let namespace_api: Api<Namespace> = Api::all(kube_client.client());
    let target_namespaces: BTreeSet<&str> = units.iter().map(|unit| unit.namespace.as_str()).collect();
    for namespace in target_namespaces {
        let state = namespace_state_from_lookup(block_on(namespace_api.get_opt(namespace)));
        facts.namespaces.insert(namespace.to_string(), state);
    }
    facts
}

fn namespace_state_from_lookup(result: Result<Option<Namespace>, kube::Error>) -> NamespaceState {
    match result {
        Ok(Some(resource)) if resource.metadata.deletion_timestamp.is_some() => NamespaceState::Terminating,
        Ok(_) => NamespaceState::Available,
        Err(kube::Error::Api(error)) if error.code == 403 => NamespaceState::Forbidden,
        Err(_) => NamespaceState::Unavailable,
    }
}

fn evaluate(
    request: &PlatformPreflightRequest,
    units: &[PlatformHelmUnit],
    facts: &PlatformPreflightFacts,
) -> PlatformPreflightOutcome {
    let results: Vec<PlatformPreflightCheckResult> = request
        .checks
        .iter()
        .map(|check| evaluate_check(check, units, facts))
        .collect();
    let blocks_execution = request.mode == PlatformPreflightMode::Enforce
        && results.iter().any(|result| {
            result.severity == PlatformPreflightCheckSeverity::Mandatory
                && result.status != PlatformPreflightCheckStatus::Pass
        });
    PlatformPreflightOutcome {
        results,
        blocks_execution,
    }
}

fn evaluate_check(
    check: &PlatformPreflightCheckRequest,
    units: &[PlatformHelmUnit],
    facts: &PlatformPreflightFacts,
) -> PlatformPreflightCheckResult {
    match check.id {
        PlatformPreflightCheckId::KubernetesApiUnreachable => match facts.kubernetes_api_reachable {
            Some(true) => result(
                check,
                PlatformPreflightCheckStatus::Pass,
                PlatformPreflightReasonCode::KubernetesApiReachable,
            ),
            Some(false) => result(
                check,
                PlatformPreflightCheckStatus::Fail,
                PlatformPreflightReasonCode::KubernetesApiUnreachable,
            ),
            None => result(
                check,
                PlatformPreflightCheckStatus::NotEvaluated,
                PlatformPreflightReasonCode::KubernetesApiNotProbed,
            ),
        },
        PlatformPreflightCheckId::NamespaceTerminating => evaluate_namespaces(check, units, facts),
        PlatformPreflightCheckId::RbacInsufficient
        | PlatformPreflightCheckId::ClusterDnsUnhealthy
        | PlatformPreflightCheckId::QoveryEndpointUnresolved
        | PlatformPreflightCheckId::ChartRegistryUnreachable
        | PlatformPreflightCheckId::ContainerRegistryUnreachable
        | PlatformPreflightCheckId::ReleaseOwnershipConflict
        | PlatformPreflightCheckId::CrdOwnershipConflict
        | PlatformPreflightCheckId::IncompatibleCertManager
        | PlatformPreflightCheckId::AcmeEndpointUnreachable
        | PlatformPreflightCheckId::DnsProviderApiUnreachable
        | PlatformPreflightCheckId::DefaultStorageClassMissing => result(
            check,
            PlatformPreflightCheckStatus::NotEvaluated,
            PlatformPreflightReasonCode::CheckNotImplemented,
        ),
        PlatformPreflightCheckId::Unknown => result(
            check,
            PlatformPreflightCheckStatus::NotEvaluated,
            PlatformPreflightReasonCode::UnknownCheckId,
        ),
    }
}

fn evaluate_namespaces(
    check: &PlatformPreflightCheckRequest,
    units: &[PlatformHelmUnit],
    facts: &PlatformPreflightFacts,
) -> PlatformPreflightCheckResult {
    if facts.kubernetes_api_reachable == Some(false) {
        return result(
            check,
            PlatformPreflightCheckStatus::NotEvaluated,
            PlatformPreflightReasonCode::KubernetesApiUnavailable,
        );
    }

    let target_namespaces: BTreeSet<&str> = units.iter().map(|unit| unit.namespace.as_str()).collect();
    for namespace in target_namespaces {
        match facts.namespaces.get(namespace) {
            Some(NamespaceState::Terminating) => {
                return result_with_evidence(
                    check,
                    PlatformPreflightCheckStatus::Fail,
                    PlatformPreflightReasonCode::NamespaceTerminating,
                    "namespace",
                    namespace,
                );
            }
            Some(NamespaceState::Forbidden) => {
                return result_with_evidence(
                    check,
                    PlatformPreflightCheckStatus::NotEvaluated,
                    PlatformPreflightReasonCode::NamespaceAccessForbidden,
                    "namespace",
                    namespace,
                );
            }
            Some(NamespaceState::Unavailable) | None => {
                return result_with_evidence(
                    check,
                    PlatformPreflightCheckStatus::NotEvaluated,
                    PlatformPreflightReasonCode::NamespaceStateUnavailable,
                    "namespace",
                    namespace,
                );
            }
            Some(NamespaceState::Available) => {}
        }
    }

    result(
        check,
        PlatformPreflightCheckStatus::Pass,
        PlatformPreflightReasonCode::TargetNamespacesAvailable,
    )
}

fn result(
    check: &PlatformPreflightCheckRequest,
    status: PlatformPreflightCheckStatus,
    reason_code: PlatformPreflightReasonCode,
) -> PlatformPreflightCheckResult {
    PlatformPreflightCheckResult {
        id: check.id,
        status,
        severity: check.severity,
        reason_code,
        evidence: BTreeMap::new(),
        remediation: remediation_for(check.id, reason_code),
    }
}

fn result_with_evidence(
    check: &PlatformPreflightCheckRequest,
    status: PlatformPreflightCheckStatus,
    reason_code: PlatformPreflightReasonCode,
    evidence_key: &str,
    evidence_value: &str,
) -> PlatformPreflightCheckResult {
    PlatformPreflightCheckResult {
        evidence: BTreeMap::from([(evidence_key.to_string(), evidence_value.to_string())]),
        ..result(check, status, reason_code)
    }
}

fn remediation_for(
    check_id: PlatformPreflightCheckId,
    reason_code: PlatformPreflightReasonCode,
) -> PlatformPreflightRemediation {
    use PlatformPreflightCheckId::*;
    use PlatformPreflightRemediationKey::*;

    let (key, message) = match reason_code {
        PlatformPreflightReasonCode::CheckNotImplemented | PlatformPreflightReasonCode::UnknownCheckId => (
            UpgradeEnginePreflight,
            "Upgrade the Engine worker to a version that supports this preflight check.",
        ),
        PlatformPreflightReasonCode::KubernetesApiUnreachable
        | PlatformPreflightReasonCode::KubernetesApiNotProbed
        | PlatformPreflightReasonCode::KubernetesApiUnavailable => (
            RestoreKubernetesApiAccess,
            "Restore Kubernetes API access from the Engine worker, then retry.",
        ),
        PlatformPreflightReasonCode::NamespaceAccessForbidden => (
            GrantRequiredRbac,
            "Grant the worker permission to read the target Kubernetes namespace.",
        ),
        PlatformPreflightReasonCode::NamespaceStateUnavailable => (
            InspectNamespaceAccess,
            "Verify the worker's Kubernetes API and RBAC access to the target namespace, then retry.",
        ),
        _ => match check_id {
            KubernetesApiUnreachable => (
                RestoreKubernetesApiAccess,
                "Restore Kubernetes API access from the Engine worker, then retry.",
            ),
            RbacInsufficient => (
                GrantRequiredRbac,
                "Grant the worker the Kubernetes permissions required by the deployment plan.",
            ),
            NamespaceTerminating => (
                WaitForNamespaceTermination,
                "Wait for target namespace termination to finish, then retry.",
            ),
            ClusterDnsUnhealthy => (RestoreClusterDns, "Restore in-cluster DNS resolution, then retry."),
            QoveryEndpointUnresolved => (
                RestoreQoveryConnectivity,
                "Restore DNS and TLS connectivity from the cluster to the Qovery endpoints.",
            ),
            ChartRegistryUnreachable => (
                RestoreChartRegistryConnectivity,
                "Restore HTTPS connectivity from the cluster to every required chart registry.",
            ),
            ContainerRegistryUnreachable => (
                RestoreContainerRegistryConnectivity,
                "Restore connectivity from the cluster to every required container registry.",
            ),
            ReleaseOwnershipConflict => (
                ResolveReleaseOwnership,
                "Remove or migrate the foreign Helm release before retrying.",
            ),
            CrdOwnershipConflict => (
                ResolveClusterResourceOwnership,
                "Remove or migrate the conflicting cluster-scoped resources before retrying.",
            ),
            IncompatibleCertManager => (
                ResolveCertManagerCompatibility,
                "Remove or migrate the incompatible cert-manager installation before retrying.",
            ),
            AcmeEndpointUnreachable => (
                RestoreAcmeConnectivity,
                "Restore HTTPS connectivity from the cluster to the selected ACME endpoint.",
            ),
            DnsProviderApiUnreachable => (
                RestoreDnsProviderConnectivity,
                "Restore HTTPS connectivity from the cluster to the DNS provider API.",
            ),
            DefaultStorageClassMissing => (
                ConfigureDefaultStorageClass,
                "Configure a default Kubernetes StorageClass before retrying.",
            ),
            Unknown => (
                UpgradeEnginePreflight,
                "Upgrade the Engine worker to a version that supports this preflight check.",
            ),
        },
    };

    PlatformPreflightRemediation {
        key,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io_models::platform_components::{
        PlatformHelmChartSource, PlatformHelmUnitAction, PlatformPreflightCheckSeverity,
    };

    fn unit(namespace: &str) -> PlatformHelmUnit {
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
        }
    }

    fn check(id: PlatformPreflightCheckId, severity: PlatformPreflightCheckSeverity) -> PlatformPreflightCheckRequest {
        PlatformPreflightCheckRequest { id, severity }
    }

    #[test]
    fn observe_reports_a_terminating_namespace_without_blocking() {
        let request = PlatformPreflightRequest {
            mode: PlatformPreflightMode::Observe,
            checks: vec![check(
                PlatformPreflightCheckId::NamespaceTerminating,
                PlatformPreflightCheckSeverity::Mandatory,
            )],
        };
        let facts = PlatformPreflightFacts {
            kubernetes_api_reachable: Some(true),
            namespaces: BTreeMap::from([("qovery".to_string(), NamespaceState::Terminating)]),
        };

        let outcome = evaluate(&request, &[unit("qovery")], &facts);

        assert!(!outcome.blocks_execution);
        assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::Fail);
        assert_eq!(
            outcome.results[0].reason_code,
            PlatformPreflightReasonCode::NamespaceTerminating
        );
        assert_eq!(outcome.results[0].evidence.get("namespace").map(String::as_str), Some("qovery"));
        assert_eq!(
            outcome.results[0].remediation.key,
            PlatformPreflightRemediationKey::WaitForNamespaceTermination
        );
    }

    #[test]
    fn enforce_blocks_when_a_mandatory_check_cannot_be_evaluated() {
        let request = PlatformPreflightRequest {
            mode: PlatformPreflightMode::Enforce,
            checks: vec![check(
                PlatformPreflightCheckId::NamespaceTerminating,
                PlatformPreflightCheckSeverity::Mandatory,
            )],
        };
        let facts = PlatformPreflightFacts {
            kubernetes_api_reachable: Some(false),
            namespaces: BTreeMap::new(),
        };

        let outcome = evaluate(&request, &[unit("qovery")], &facts);

        assert!(outcome.blocks_execution);
        assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::NotEvaluated);
        assert_eq!(
            outcome.results[0].reason_code,
            PlatformPreflightReasonCode::KubernetesApiUnavailable
        );
        assert_eq!(
            outcome.results[0].remediation.key,
            PlatformPreflightRemediationKey::RestoreKubernetesApiAccess
        );
    }

    #[test]
    fn forbidden_namespace_lookup_reports_rbac_remediation() {
        let state = namespace_state_from_lookup(Err(kube::Error::Api(Box::new(kube::error::ErrorResponse {
            code: 403,
            ..Default::default()
        }))));
        assert_eq!(state, NamespaceState::Forbidden);

        let request = PlatformPreflightRequest {
            mode: PlatformPreflightMode::Observe,
            checks: vec![check(
                PlatformPreflightCheckId::NamespaceTerminating,
                PlatformPreflightCheckSeverity::Mandatory,
            )],
        };
        let facts = PlatformPreflightFacts {
            kubernetes_api_reachable: Some(true),
            namespaces: BTreeMap::from([("qovery".to_string(), state)]),
        };

        let outcome = evaluate(&request, &[unit("qovery")], &facts);

        assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::NotEvaluated);
        assert_eq!(
            outcome.results[0].reason_code,
            PlatformPreflightReasonCode::NamespaceAccessForbidden
        );
        assert_eq!(
            outcome.results[0].remediation.key,
            PlatformPreflightRemediationKey::GrantRequiredRbac
        );
    }

    #[test]
    fn enforce_does_not_block_an_unevaluated_advisory_check() {
        let request = PlatformPreflightRequest {
            mode: PlatformPreflightMode::Enforce,
            checks: vec![check(
                PlatformPreflightCheckId::ContainerRegistryUnreachable,
                PlatformPreflightCheckSeverity::Advisory,
            )],
        };

        let outcome = evaluate(&request, &[unit("qovery")], &PlatformPreflightFacts::default());

        assert!(!outcome.blocks_execution);
        assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::NotEvaluated);
        assert_eq!(outcome.results[0].reason_code, PlatformPreflightReasonCode::CheckNotImplemented);
        assert_eq!(
            outcome.results[0].remediation.key,
            PlatformPreflightRemediationKey::UpgradeEnginePreflight
        );
    }
}
