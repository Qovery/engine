//! Internal facts and resource descriptions shared by preparation, collectors, and pure policy evaluation.

use crate::io_models::platform_components::{PlatformHelmUnit, PlatformPreflightCheckResult};
use kube::core::GroupVersionKind;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const HELM_RELEASE_NAME_ANNOTATION: &str = "meta.helm.sh/release-name";

pub(super) const HELM_RELEASE_NAMESPACE_ANNOTATION: &str = "meta.helm.sh/release-namespace";

pub(super) const HELM_MANAGED_BY_LABEL: &str = "app.kubernetes.io/managed-by";

pub(super) const HELM_HOOK_ANNOTATION: &str = "helm.sh/hook";

/// Result of the non-mutating preflight phase, kept independent from the Helm executor.
pub(in super::super) struct PlatformPreflightOutcome {
    pub results: Vec<PlatformPreflightCheckResult>,
    pub blocks_execution: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct PlannedKubernetesResource {
    pub(super) unit_key: String,
    pub(super) release_name: String,
    pub(super) release_namespace: String,
    pub(super) api_version: String,
    pub(super) kind: String,
    pub(super) name: Option<String>,
    pub(super) generate_name: Option<String>,
    pub(super) namespace: Option<String>,
    pub(super) installed_without_helm_ownership: bool,
    pub(super) helm_hook: Option<PlannedHelmHook>,
    pub(super) role_reference: Option<PlannedRoleReference>,
    pub(super) custom_resource_definition: Option<PlannedCustomResourceDefinition>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct PlannedHelmHook {
    pub(super) events: BTreeSet<String>,
    pub(super) deletes_before_creation: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct PlannedRoleReference {
    pub(super) api_group: String,
    pub(super) kind: String,
    pub(super) name: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct PlannedCustomResourceDefinition {
    pub(super) group: String,
    pub(super) kind: String,
    pub(super) plural: String,
    pub(super) namespaced: bool,
    pub(super) versions: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PlannedApiResource {
    pub(super) group: String,
    pub(super) version: String,
    pub(super) plural: String,
    pub(super) namespaced: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NamespaceState {
    Available,
    Terminating,
    Forbidden,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FactState {
    Pass,
    Fail(BTreeMap<String, String>),
    Unavailable(BTreeMap<String, String>),
    NotApplicable,
}

impl Default for FactState {
    fn default() -> Self {
        Self::Unavailable(BTreeMap::new())
    }
}

#[derive(Default)]
pub(super) struct PlatformPreflightFacts {
    pub(super) kubernetes_api_reachable: Option<bool>,
    pub(super) namespaces: BTreeMap<String, NamespaceState>,
    pub(super) rbac: FactState,
    pub(super) cluster_dns: FactState,
    pub(super) qovery_connectivity: FactState,
    pub(super) chart_registries: FactState,
    pub(super) container_registries: FactState,
    pub(super) release_ownership: FactState,
    pub(super) cluster_resource_ownership: FactState,
    pub(super) cert_manager: FactState,
    pub(super) default_storage_class: FactState,
    /// The Helm inventory hit its hard cap: release-derived checks are unavailable because of
    /// cluster size, not because of a transient read failure.
    pub(super) release_inventory_too_large: bool,
}

impl PlannedKubernetesResource {
    pub(super) fn gvk(&self) -> GroupVersionKind {
        let (group, version) = self
            .api_version
            .split_once('/')
            .map_or(("", self.api_version.as_str()), |(group, version)| (group, version));
        GroupVersionKind::gvk(group, version, &self.kind)
    }

    pub(super) fn display_name(&self) -> String {
        let name = self
            .name
            .as_deref()
            .or(self.generate_name.as_deref())
            .unwrap_or("<generated>");
        format!("{}/{name}", self.kind)
    }

    pub(super) fn expected_owner(&self) -> String {
        format!("{}/{}", self.release_namespace, self.release_name)
    }
}

pub(super) fn target_namespaces(units: &[PlatformHelmUnit]) -> BTreeSet<&str> {
    units.iter().map(|unit| unit.namespace.as_str()).collect()
}

pub(super) fn evidence<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.chars().take(256).collect()))
        .collect()
}
