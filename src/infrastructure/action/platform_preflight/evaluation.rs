//! Pure conversion of collected facts into check results, remediations, and execution policy.

use super::model::{
    FactState, NamespaceState, PlatformPreflightFacts, PlatformPreflightOutcome, evidence, target_namespaces,
};
use crate::io_models::platform_components::{
    PlatformHelmUnit, PlatformPreflightCheckId, PlatformPreflightCheckRequest, PlatformPreflightCheckResult,
    PlatformPreflightCheckSeverity, PlatformPreflightCheckStatus, PlatformPreflightMode, PlatformPreflightReasonCode,
    PlatformPreflightRemediation, PlatformPreflightRemediationKey, PlatformPreflightRequest,
};
use std::collections::BTreeMap;

pub(super) fn evaluate(
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
        PlatformPreflightCheckId::RbacInsufficient => evaluate_fact(
            check,
            &facts.rbac,
            PlatformPreflightReasonCode::RbacSufficient,
            PlatformPreflightReasonCode::RbacInsufficient,
            inventory_unavailable_reason(facts, PlatformPreflightReasonCode::RbacStateUnavailable),
            PlatformPreflightReasonCode::RbacStateUnavailable,
        ),
        PlatformPreflightCheckId::ClusterDnsUnhealthy => evaluate_fact(
            check,
            &facts.cluster_dns,
            PlatformPreflightReasonCode::ClusterDnsHealthy,
            PlatformPreflightReasonCode::ClusterDnsUnhealthy,
            PlatformPreflightReasonCode::ClusterDnsStateUnavailable,
            PlatformPreflightReasonCode::ClusterDnsStateUnavailable,
        ),
        PlatformPreflightCheckId::QoveryEndpointUnresolved => evaluate_fact(
            check,
            &facts.qovery_connectivity,
            PlatformPreflightReasonCode::QoveryEndpointsReachable,
            PlatformPreflightReasonCode::QoveryEndpointUnreachable,
            PlatformPreflightReasonCode::QoveryEndpointNotConfigured,
            PlatformPreflightReasonCode::QoveryEndpointNotConfigured,
        ),
        PlatformPreflightCheckId::ChartRegistryUnreachable => evaluate_fact(
            check,
            &facts.chart_registries,
            PlatformPreflightReasonCode::ChartRegistriesReachable,
            PlatformPreflightReasonCode::ChartRegistryUnreachable,
            PlatformPreflightReasonCode::ChartRegistryStateUnavailable,
            PlatformPreflightReasonCode::ChartRegistryStateUnavailable,
        ),
        PlatformPreflightCheckId::ContainerRegistryUnreachable => evaluate_fact(
            check,
            &facts.container_registries,
            PlatformPreflightReasonCode::ContainerRegistriesReachable,
            PlatformPreflightReasonCode::ContainerRegistryUnreachable,
            PlatformPreflightReasonCode::ContainerRegistryNotConfigured,
            PlatformPreflightReasonCode::ContainerRegistryNotConfigured,
        ),
        PlatformPreflightCheckId::ReleaseOwnershipConflict => evaluate_fact(
            check,
            &facts.release_ownership,
            PlatformPreflightReasonCode::ReleasesOwnedByPlan,
            PlatformPreflightReasonCode::ReleaseOwnershipConflict,
            inventory_unavailable_reason(facts, PlatformPreflightReasonCode::ReleaseStateUnavailable),
            PlatformPreflightReasonCode::ReleaseStateUnavailable,
        ),
        PlatformPreflightCheckId::CrdOwnershipConflict => evaluate_fact(
            check,
            &facts.cluster_resource_ownership,
            PlatformPreflightReasonCode::ClusterResourcesOwnedByPlan,
            PlatformPreflightReasonCode::CrdOwnershipConflict,
            inventory_unavailable_reason(facts, PlatformPreflightReasonCode::ClusterResourceStateUnavailable),
            PlatformPreflightReasonCode::ClusterResourceStateUnavailable,
        ),
        PlatformPreflightCheckId::IncompatibleCertManager => evaluate_fact(
            check,
            &facts.cert_manager,
            PlatformPreflightReasonCode::CertManagerCompatible,
            PlatformPreflightReasonCode::IncompatibleCertManager,
            inventory_unavailable_reason(facts, PlatformPreflightReasonCode::ClusterResourceStateUnavailable),
            PlatformPreflightReasonCode::CertManagerNotPlanned,
        ),
        PlatformPreflightCheckId::DefaultStorageClassMissing => evaluate_fact(
            check,
            &facts.default_storage_class,
            PlatformPreflightReasonCode::DefaultStorageClassAvailable,
            PlatformPreflightReasonCode::DefaultStorageClassMissing,
            PlatformPreflightReasonCode::StorageClassStateUnavailable,
            PlatformPreflightReasonCode::DefaultStorageClassNotRequired,
        ),
        PlatformPreflightCheckId::AcmeEndpointUnreachable | PlatformPreflightCheckId::DnsProviderApiUnreachable => {
            result(
                check,
                PlatformPreflightCheckStatus::NotEvaluated,
                PlatformPreflightReasonCode::CheckNotImplemented,
            )
        }
        PlatformPreflightCheckId::Unknown => result(
            check,
            PlatformPreflightCheckStatus::NotEvaluated,
            PlatformPreflightReasonCode::UnknownCheckId,
        ),
    }
}

/// Release-derived checks share one root cause when the inventory hit its cap: report the limit,
/// not a transient read failure, so operators can size the rollout instead of retrying.
fn inventory_unavailable_reason(
    facts: &PlatformPreflightFacts,
    default: PlatformPreflightReasonCode,
) -> PlatformPreflightReasonCode {
    if facts.release_inventory_too_large {
        PlatformPreflightReasonCode::ReleaseInventoryTooLarge
    } else {
        default
    }
}

fn evaluate_fact(
    check: &PlatformPreflightCheckRequest,
    fact: &FactState,
    pass_reason: PlatformPreflightReasonCode,
    fail_reason: PlatformPreflightReasonCode,
    unavailable_reason: PlatformPreflightReasonCode,
    not_applicable_reason: PlatformPreflightReasonCode,
) -> PlatformPreflightCheckResult {
    let (status, reason, details) = match fact {
        FactState::Pass => (PlatformPreflightCheckStatus::Pass, pass_reason, BTreeMap::new()),
        FactState::Fail(details) => (PlatformPreflightCheckStatus::Fail, fail_reason, details.clone()),
        FactState::Unavailable(details) => {
            (PlatformPreflightCheckStatus::NotEvaluated, unavailable_reason, details.clone())
        }
        FactState::NotApplicable => (PlatformPreflightCheckStatus::Pass, not_applicable_reason, BTreeMap::new()),
    };
    result_with_evidence(check, status, reason, details)
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
    for namespace in target_namespaces(units) {
        match facts.namespaces.get(namespace) {
            Some(NamespaceState::Terminating) => {
                return result_with_evidence(
                    check,
                    PlatformPreflightCheckStatus::Fail,
                    PlatformPreflightReasonCode::NamespaceTerminating,
                    evidence([("namespace", namespace)]),
                );
            }
            Some(NamespaceState::Forbidden) => {
                return result_with_evidence(
                    check,
                    PlatformPreflightCheckStatus::NotEvaluated,
                    PlatformPreflightReasonCode::NamespaceAccessForbidden,
                    evidence([("namespace", namespace)]),
                );
            }
            Some(NamespaceState::Unavailable) | None => {
                return result_with_evidence(
                    check,
                    PlatformPreflightCheckStatus::NotEvaluated,
                    PlatformPreflightReasonCode::NamespaceStateUnavailable,
                    evidence([("namespace", namespace)]),
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
    result_with_evidence(check, status, reason_code, BTreeMap::new())
}

fn result_with_evidence(
    check: &PlatformPreflightCheckRequest,
    status: PlatformPreflightCheckStatus,
    reason_code: PlatformPreflightReasonCode,
    evidence: BTreeMap<String, String>,
) -> PlatformPreflightCheckResult {
    PlatformPreflightCheckResult {
        id: check.id,
        status,
        severity: check.severity,
        reason_code,
        evidence,
        remediation: remediation_for(check.id, reason_code),
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
        PlatformPreflightReasonCode::QoveryEndpointNotConfigured
        | PlatformPreflightReasonCode::ContainerRegistryNotConfigured => (
            UpgradeEnginePreflight,
            "Retry the deployment; if the problem persists, upgrade the Engine worker or contact Qovery support.",
        ),
        PlatformPreflightReasonCode::ReleaseInventoryTooLarge => (
            ReportReleaseInventoryLimit,
            "The cluster holds more Helm releases than the preflight inventory reads; contact Qovery support before enforcing release-derived checks.",
        ),
        PlatformPreflightReasonCode::RbacStateUnavailable
        | PlatformPreflightReasonCode::ClusterDnsStateUnavailable
        | PlatformPreflightReasonCode::ChartRegistryStateUnavailable
        | PlatformPreflightReasonCode::ReleaseStateUnavailable
        | PlatformPreflightReasonCode::ClusterResourceStateUnavailable
        | PlatformPreflightReasonCode::StorageClassStateUnavailable => (
            RetryPreflightCheck,
            "Retry the deployment; if this check remains unavailable, contact Qovery support.",
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
    use super::super::model::{FactState, NamespaceState, PlatformPreflightFacts, evidence};
    use super::super::test_support::{check, request, unit};
    use super::evaluate;
    use crate::io_models::platform_components::{
        PlatformPreflightCheckId, PlatformPreflightCheckSeverity, PlatformPreflightCheckStatus, PlatformPreflightMode,
        PlatformPreflightReasonCode,
    };
    use std::collections::BTreeMap;

    #[test]
    fn observe_reports_a_terminating_namespace_without_blocking() {
        let request = request(
            PlatformPreflightMode::Observe,
            check(
                PlatformPreflightCheckId::NamespaceTerminating,
                PlatformPreflightCheckSeverity::Mandatory,
            ),
        );
        let facts = PlatformPreflightFacts {
            kubernetes_api_reachable: Some(true),
            namespaces: BTreeMap::from([("qovery".to_string(), NamespaceState::Terminating)]),
            ..Default::default()
        };
        let outcome = evaluate(&request, &[unit("qovery")], &facts);
        assert!(!outcome.blocks_execution);
        assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::Fail);
        assert_eq!(
            outcome.results[0].reason_code,
            PlatformPreflightReasonCode::NamespaceTerminating
        );
    }

    #[test]
    fn enforce_blocks_when_a_mandatory_rbac_check_cannot_be_evaluated() {
        let request = request(
            PlatformPreflightMode::Enforce,
            check(
                PlatformPreflightCheckId::RbacInsufficient,
                PlatformPreflightCheckSeverity::Mandatory,
            ),
        );
        let facts = PlatformPreflightFacts {
            rbac: FactState::Unavailable(BTreeMap::new()),
            ..Default::default()
        };
        let outcome = evaluate(&request, &[unit("qovery")], &facts);
        assert!(outcome.blocks_execution);
        assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::NotEvaluated);
        assert_eq!(
            outcome.results[0].reason_code,
            PlatformPreflightReasonCode::RbacStateUnavailable
        );
    }

    #[test]
    fn saturated_inventory_reports_its_own_reason_code_for_release_derived_checks() {
        use crate::io_models::platform_components::PlatformPreflightRemediationKey;
        let inventory_checks = [
            (PlatformPreflightCheckId::RbacInsufficient, "rbac"),
            (PlatformPreflightCheckId::ReleaseOwnershipConflict, "release_ownership"),
            (PlatformPreflightCheckId::CrdOwnershipConflict, "cluster_resource_ownership"),
            (PlatformPreflightCheckId::IncompatibleCertManager, "cert_manager"),
        ];
        for (id, _) in inventory_checks {
            let request = request(
                PlatformPreflightMode::Enforce,
                check(id, PlatformPreflightCheckSeverity::Mandatory),
            );
            let details = evidence([("reason", "release-inventory-too-large"), ("limit", "1000")]);
            let facts = PlatformPreflightFacts {
                rbac: FactState::Unavailable(details.clone()),
                release_ownership: FactState::Unavailable(details.clone()),
                cluster_resource_ownership: FactState::Unavailable(details.clone()),
                cert_manager: FactState::Unavailable(details.clone()),
                release_inventory_too_large: true,
                ..Default::default()
            };
            let outcome = evaluate(&request, &[unit("qovery")], &facts);
            assert!(outcome.blocks_execution, "{id:?}");
            assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::NotEvaluated, "{id:?}");
            assert_eq!(
                outcome.results[0].reason_code,
                PlatformPreflightReasonCode::ReleaseInventoryTooLarge,
                "{id:?}"
            );
            assert_eq!(
                outcome.results[0].remediation.key,
                PlatformPreflightRemediationKey::ReportReleaseInventoryLimit,
                "{id:?}"
            );
            assert_eq!(outcome.results[0].evidence.get("limit").map(String::as_str), Some("1000"));
        }

        // A DNS check does not depend on the inventory and keeps its own reason code.
        let request = request(
            PlatformPreflightMode::Observe,
            check(
                PlatformPreflightCheckId::ClusterDnsUnhealthy,
                PlatformPreflightCheckSeverity::Advisory,
            ),
        );
        let facts = PlatformPreflightFacts {
            release_inventory_too_large: true,
            ..Default::default()
        };
        assert_eq!(
            evaluate(&request, &[unit("qovery")], &facts).results[0].reason_code,
            PlatformPreflightReasonCode::ClusterDnsStateUnavailable
        );
    }

    #[test]
    fn storage_class_check_is_plan_derived() {
        let request = request(
            PlatformPreflightMode::Observe,
            check(
                PlatformPreflightCheckId::DefaultStorageClassMissing,
                PlatformPreflightCheckSeverity::Advisory,
            ),
        );
        let missing = PlatformPreflightFacts {
            default_storage_class: FactState::Fail(evidence([("component", "cluster-agent")])),
            ..Default::default()
        };
        let outcome = evaluate(&request, &[unit("qovery")], &missing);
        assert_eq!(
            outcome.results[0].reason_code,
            PlatformPreflightReasonCode::DefaultStorageClassMissing
        );

        let not_required = PlatformPreflightFacts {
            default_storage_class: FactState::NotApplicable,
            ..Default::default()
        };
        let outcome = evaluate(&request, &[unit("qovery")], &not_required);
        assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::Pass);
        assert_eq!(
            outcome.results[0].reason_code,
            PlatformPreflightReasonCode::DefaultStorageClassNotRequired
        );
    }

    #[test]
    fn foreign_release_and_cluster_resource_are_reported() {
        let release_request = request(
            PlatformPreflightMode::Observe,
            check(
                PlatformPreflightCheckId::ReleaseOwnershipConflict,
                PlatformPreflightCheckSeverity::Mandatory,
            ),
        );
        let release_facts = PlatformPreflightFacts {
            release_ownership: FactState::Fail(evidence([("release", "cluster-agent")])),
            ..Default::default()
        };
        assert_eq!(
            evaluate(&release_request, &[unit("qovery")], &release_facts).results[0].reason_code,
            PlatformPreflightReasonCode::ReleaseOwnershipConflict
        );

        let crd_request = request(
            PlatformPreflightMode::Observe,
            check(
                PlatformPreflightCheckId::CrdOwnershipConflict,
                PlatformPreflightCheckSeverity::Mandatory,
            ),
        );
        let crd_facts = PlatformPreflightFacts {
            cluster_resource_ownership: FactState::Fail(evidence([("resource", "CustomResourceDefinition/test")])),
            ..Default::default()
        };
        assert_eq!(
            evaluate(&crd_request, &[unit("qovery")], &crd_facts).results[0].status,
            PlatformPreflightCheckStatus::Fail
        );
    }

    #[test]
    fn chart_registry_failure_uses_the_unreachable_reason_code() {
        let request = request(
            PlatformPreflightMode::Observe,
            check(
                PlatformPreflightCheckId::ChartRegistryUnreachable,
                PlatformPreflightCheckSeverity::Advisory,
            ),
        );
        let outcome = evaluate(
            &request,
            &[unit("qovery")],
            &PlatformPreflightFacts {
                chart_registries: FactState::Fail(BTreeMap::new()),
                ..Default::default()
            },
        );

        assert_eq!(outcome.results[0].status, PlatformPreflightCheckStatus::Fail);
        assert_eq!(
            outcome.results[0].reason_code,
            PlatformPreflightReasonCode::ChartRegistryUnreachable
        );
    }
}
