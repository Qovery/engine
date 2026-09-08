//! Non-mutating platform preflight orchestration. Collectors produce facts; evaluation decides the policy outcome.

mod connectivity;
mod evaluation;
mod kubernetes;
mod model;
mod ownership;
mod plan;
mod runtime;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;

pub(super) use plan::{PreparedPlatformUnit, prepare_platform_preflight_plan};

use self::connectivity::{probe_chart_registries, probe_container_registries, probe_http_endpoints};
use self::evaluation::evaluate;
use self::kubernetes::{
    collect_cluster_dns, collect_default_storage_class, collect_rbac, namespace_state_from_lookup,
    runs_inside_kubernetes,
};
use self::model::{FactState, NamespaceState, PlatformPreflightFacts, PlatformPreflightOutcome, target_namespaces};
use self::ownership::{
    collect_cert_manager_compatibility, collect_cluster_resource_ownership, collect_release_ownership,
};
use self::plan::PreparedPlatformPlan;
use self::runtime::{KUBERNETES_READ_TIMEOUT, block_on_timeout};
use crate::infrastructure::infrastructure_context::{InfrastructureContext, KubeClientAuthMode};
use crate::io_models::platform_components::{PlatformHelmUnit, PlatformPreflightCheckId, PlatformPreflightRequest};
use k8s_openapi::api::core::v1::Namespace;
use kube::api::Api;

/// Runs only read operations and evaluates the requested checks in their original order.
/// Call from a synchronous worker or `spawn_blocking`: connectivity probes use `reqwest::blocking`.
pub(super) fn run_platform_preflight(
    infra_ctx: &InfrastructureContext,
    request: &PlatformPreflightRequest,
    units: &[PlatformHelmUnit],
    plan: &PreparedPlatformPlan,
) -> PlatformPreflightOutcome {
    let facts = collect_facts(infra_ctx, request, units, plan);
    evaluate(request, units, &facts)
}

fn collect_facts(
    infra_ctx: &InfrastructureContext,
    request: &PlatformPreflightRequest,
    units: &[PlatformHelmUnit],
    plan: &PreparedPlatformPlan,
) -> PlatformPreflightFacts {
    let requested = |id| request.checks.iter().any(|check| check.id == id);
    let needs_kubernetes_api = request.checks.iter().any(|check| {
        matches!(
            check.id,
            PlatformPreflightCheckId::KubernetesApiUnreachable
                | PlatformPreflightCheckId::NamespaceTerminating
                | PlatformPreflightCheckId::RbacInsufficient
                | PlatformPreflightCheckId::ClusterDnsUnhealthy
                | PlatformPreflightCheckId::CrdOwnershipConflict
                | PlatformPreflightCheckId::IncompatibleCertManager
                | PlatformPreflightCheckId::DefaultStorageClassMissing
        )
    });

    let mut facts = PlatformPreflightFacts {
        qovery_connectivity: if requested(PlatformPreflightCheckId::QoveryEndpointUnresolved) {
            probe_http_endpoints(&request.qovery_endpoints)
        } else {
            FactState::NotApplicable
        },
        chart_registries: if requested(PlatformPreflightCheckId::ChartRegistryUnreachable) {
            probe_chart_registries(units)
        } else {
            FactState::NotApplicable
        },
        container_registries: if requested(PlatformPreflightCheckId::ContainerRegistryUnreachable) {
            probe_container_registries(units)
        } else {
            FactState::NotApplicable
        },
        release_ownership: if requested(PlatformPreflightCheckId::ReleaseOwnershipConflict) {
            collect_release_ownership(plan, units)
        } else {
            FactState::NotApplicable
        },
        release_inventory_too_large: plan.inventory_too_large(),
        ..Default::default()
    };

    if !needs_kubernetes_api {
        return facts;
    }
    let kube_client = match infra_ctx.mk_kube_client_with_auth_mode(KubeClientAuthMode::AllowInCluster) {
        Ok(client) => client,
        Err(_) => {
            facts.kubernetes_api_reachable = Some(false);
            return facts;
        }
    };
    let client = kube_client.client();
    if !matches!(block_on_timeout(KUBERNETES_READ_TIMEOUT, client.apiserver_version()), Ok(Ok(_))) {
        facts.kubernetes_api_reachable = Some(false);
        return facts;
    }
    facts.kubernetes_api_reachable = Some(true);

    if requested(PlatformPreflightCheckId::NamespaceTerminating) {
        let namespace_api: Api<Namespace> = Api::all(client.clone());
        for namespace in target_namespaces(units) {
            let state = match block_on_timeout(KUBERNETES_READ_TIMEOUT, namespace_api.get_opt(namespace)) {
                Ok(result) => namespace_state_from_lookup(result),
                Err(_) => NamespaceState::Unavailable,
            };
            facts.namespaces.insert(namespace.to_string(), state);
        }
    }
    if requested(PlatformPreflightCheckId::ClusterDnsUnhealthy) {
        facts.cluster_dns = collect_cluster_dns(client.clone(), runs_inside_kubernetes());
    }
    if requested(PlatformPreflightCheckId::DefaultStorageClassMissing) {
        facts.default_storage_class = collect_default_storage_class(client.clone(), units);
    }

    let discovery = plan.discovery.as_ref();
    if requested(PlatformPreflightCheckId::RbacInsufficient) {
        facts.rbac = collect_rbac(client.clone(), discovery, plan, units);
    }
    if requested(PlatformPreflightCheckId::CrdOwnershipConflict) {
        facts.cluster_resource_ownership =
            collect_cluster_resource_ownership(client.clone(), discovery, plan, |_| true);
    }
    if requested(PlatformPreflightCheckId::IncompatibleCertManager) {
        facts.cert_manager = collect_cert_manager_compatibility(client, discovery, units, plan);
    }
    facts
}
