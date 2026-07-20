use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::eksanywhere::provider::{EksAnywhereClusterName, EksAnywhereProviderMode};
use crate::runtime::block_on;
use kube::Client;
use kube::api::{Api, DynamicObject, ListParams};
use kube::discovery::{Discovery, verbs};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use tokio::time::timeout;

const CAPI_CLUSTER_NAME_LABEL: &str = "cluster.x-k8s.io/cluster-name";
const EKSA_API_GROUP: &str = "anywhere.eks.amazonaws.com";
const CORE_DIAGNOSTIC_API_GROUPS: [&str; 3] = [EKSA_API_GROUP, "cluster.x-k8s.io", "controlplane.cluster.x-k8s.io"];
const VSPHERE_DIAGNOSTIC_API_GROUPS: [&str; 4] = [
    EKSA_API_GROUP,
    "cluster.x-k8s.io",
    "controlplane.cluster.x-k8s.io",
    "infrastructure.cluster.x-k8s.io",
];
const CORE_DIAGNOSTIC_RESOURCE_KINDS: [DiagnosticResourceKind; 4] = [
    DiagnosticResourceKind::EksAnywhereCluster,
    DiagnosticResourceKind::KubeadmControlPlane,
    DiagnosticResourceKind::MachineDeployment,
    DiagnosticResourceKind::Machine,
];
const VSPHERE_DIAGNOSTIC_RESOURCE_KINDS: [DiagnosticResourceKind; 5] = [
    DiagnosticResourceKind::EksAnywhereCluster,
    DiagnosticResourceKind::KubeadmControlPlane,
    DiagnosticResourceKind::MachineDeployment,
    DiagnosticResourceKind::Machine,
    DiagnosticResourceKind::VSphereMachine,
];
const EKSA_WAIT_CONDITION_TYPES: [&str; 4] = ["ControlPlaneReady", "DefaultCNIConfigured", "WorkersReady", "Ready"];
const KUBERNETES_API_DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CONDITION_MESSAGE_LENGTH: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticResourceKind {
    EksAnywhereCluster,
    KubeadmControlPlane,
    MachineDeployment,
    Machine,
    VSphereMachine,
}

impl DiagnosticResourceKind {
    fn api_group(self) -> &'static str {
        match self {
            Self::EksAnywhereCluster => EKSA_API_GROUP,
            Self::KubeadmControlPlane => "controlplane.cluster.x-k8s.io",
            Self::MachineDeployment | Self::Machine => "cluster.x-k8s.io",
            Self::VSphereMachine => "infrastructure.cluster.x-k8s.io",
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::EksAnywhereCluster => "Cluster",
            Self::KubeadmControlPlane => "KubeadmControlPlane",
            Self::MachineDeployment => "MachineDeployment",
            Self::Machine => "Machine",
            Self::VSphereMachine => "VSphereMachine",
        }
    }

    fn diagnostic_kind(self) -> &'static str {
        match self {
            Self::EksAnywhereCluster => "EKSAnywhereCluster",
            _ => self.kind(),
        }
    }

    fn list_params(self, cluster_name: &str) -> ListParams {
        match self {
            Self::EksAnywhereCluster => ListParams::default().fields(&format!("metadata.name={cluster_name}")),
            _ => ListParams::default().labels(&format!("{CAPI_CLUSTER_NAME_LABEL}={cluster_name}")),
        }
    }
}

fn diagnostic_api_groups(provider_mode: EksAnywhereProviderMode) -> &'static [&'static str] {
    match provider_mode {
        EksAnywhereProviderMode::VSphere => &VSPHERE_DIAGNOSTIC_API_GROUPS,
        EksAnywhereProviderMode::Unknown => &CORE_DIAGNOSTIC_API_GROUPS,
    }
}

fn diagnostic_resource_kinds(provider_mode: EksAnywhereProviderMode) -> &'static [DiagnosticResourceKind] {
    match provider_mode {
        EksAnywhereProviderMode::VSphere => &VSPHERE_DIAGNOSTIC_RESOURCE_KINDS,
        EksAnywhereProviderMode::Unknown => &CORE_DIAGNOSTIC_RESOURCE_KINDS,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpgradeDiagnosticsTrigger {
    Periodic,
    CommandFailure,
    WorkflowCompletion,
}

#[derive(Clone, Copy)]
pub(super) struct CapiDiagnosticsContext<'a> {
    kube_client: &'a Client,
    cluster_name: &'a EksAnywhereClusterName,
    provider_mode: EksAnywhereProviderMode,
}

impl<'a> CapiDiagnosticsContext<'a> {
    pub fn new(
        kube_client: &'a Client,
        cluster_name: &'a EksAnywhereClusterName,
        provider_mode: EksAnywhereProviderMode,
    ) -> Self {
        Self {
            kube_client,
            cluster_name,
            provider_mode,
        }
    }
}

#[derive(Debug, Error)]
enum UpgradeDiagnosticsError {
    #[error("Kubernetes API discovery failed: {source}")]
    Discovery {
        #[source]
        source: Box<kube::Error>,
    },
    #[error("Kubernetes API group `{api_group}` required for upgrade diagnostics is not served")]
    MissingApiGroup { api_group: &'static str },
    #[error("Upgrade diagnostic kind `{kind}` was not found in API group `{api_group}`")]
    MissingKind {
        api_group: &'static str,
        kind: &'static str,
    },
    #[error("Upgrade diagnostic resource `{api_version}/{kind}` does not support list operations")]
    ListNotSupported { api_version: String, kind: &'static str },
    #[error("cannot list upgrade diagnostic resource `{api_version}/{kind}`: {source}")]
    List {
        api_version: String,
        kind: &'static str,
        #[source]
        source: Box<kube::Error>,
    },
    #[error("cannot decode upgrade diagnostic resource `{kind}`: {source}")]
    InvalidResource {
        kind: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("Kubernetes API diagnostic request timed out after {timeout_seconds}s")]
    Timeout { timeout_seconds: u64 },
}

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct KubernetesResourceList {
    #[serde(default)]
    items: Vec<KubernetesResource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubernetesResource {
    #[serde(default)]
    kind: String,
    metadata: KubernetesMetadata,
    #[serde(default)]
    spec: KubernetesResourceSpec,
    #[serde(default)]
    status: KubernetesResourceStatus,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubernetesMetadata {
    #[serde(default)]
    name: String,
    namespace: Option<String>,
    #[serde(default)]
    labels: HashMap<String, String>,
    generation: Option<i64>,
    deletion_timestamp: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubernetesResourceSpec {
    cluster_name: Option<String>,
    replicas: Option<i64>,
    version: Option<String>,
    #[serde(rename = "providerID")]
    provider_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubernetesResourceStatus {
    observed_generation: Option<i64>,
    reconciled_generation: Option<i64>,
    ready: Option<bool>,
    phase: Option<String>,
    replicas: Option<i64>,
    ready_replicas: Option<i64>,
    updated_replicas: Option<i64>,
    up_to_date_replicas: Option<i64>,
    unavailable_replicas: Option<i64>,
    node_ref: Option<KubernetesNodeReference>,
    failure_reason: Option<String>,
    failure_message: Option<String>,
    #[serde(default)]
    conditions: Vec<KubernetesResourceCondition>,
    v1beta2: Option<KubernetesV1Beta2Status>,
}

#[derive(Debug, Deserialize)]
struct KubernetesNodeReference {
    name: String,
}

#[derive(Debug, Deserialize)]
struct KubernetesV1Beta2Status {
    #[serde(default)]
    conditions: Vec<KubernetesResourceCondition>,
}

#[derive(Debug, Deserialize)]
struct KubernetesResourceCondition {
    #[serde(rename = "type")]
    condition_type: String,
    status: String,
    reason: Option<String>,
    message: Option<String>,
}

impl KubernetesResource {
    fn belongs_to_cluster(&self, cluster_name: &str) -> bool {
        (self.kind == DiagnosticResourceKind::EksAnywhereCluster.diagnostic_kind()
            && self.metadata.name == cluster_name)
            || self
                .metadata
                .labels
                .get(CAPI_CLUSTER_NAME_LABEL)
                .is_some_and(|value| value == cluster_name)
            || self.spec.cluster_name.as_deref() == Some(cluster_name)
    }

    fn display_name(&self) -> String {
        match self.metadata.namespace.as_deref() {
            Some(namespace) => format!("{namespace}/{}", self.metadata.name),
            None => self.metadata.name.clone(),
        }
    }

    fn conditions(&self) -> impl Iterator<Item = &KubernetesResourceCondition> {
        self.status
            .conditions
            .iter()
            .chain(self.status.v1beta2.iter().flat_map(|status| status.conditions.iter()))
    }

    fn up_to_date_replicas(&self) -> Option<i64> {
        self.status.up_to_date_replicas.or(self.status.updated_replicas)
    }

    fn rollout_converged(&self) -> bool {
        let Some(desired) = self.spec.replicas else {
            return false;
        };

        self.status.replicas == Some(desired)
            && self.status.ready_replicas == Some(desired)
            && self.up_to_date_replicas().is_none_or(|replicas| replicas == desired)
            && self.status.unavailable_replicas.unwrap_or_default() == 0
    }

    fn reconciliation_pending(&self) -> bool {
        let Some(generation) = self.metadata.generation else {
            return false;
        };

        self.status.observed_generation != Some(generation)
            || self
                .status
                .reconciled_generation
                .is_some_and(|reconciled_generation| reconciled_generation != generation)
    }
}

pub(super) fn log_capi_upgrade_diagnostics(
    context: CapiDiagnosticsContext<'_>,
    logger: &impl InfraLogger,
    trigger: UpgradeDiagnosticsTrigger,
) {
    let header = match trigger {
        UpgradeDiagnosticsTrigger::Periodic => "🔎 CAPI upgrade status snapshot",
        UpgradeDiagnosticsTrigger::CommandFailure => "🔎 CAPI status after EKS Anywhere upgrade failure",
        UpgradeDiagnosticsTrigger::WorkflowCompletion => "🔎 CAPI cluster status snapshot",
    };

    let cluster_name = context.cluster_name.as_str();
    let lines = match collect_upgrade_diagnostic_resources(context.kube_client, cluster_name, context.provider_mode) {
        Ok(resources) => render_upgrade_diagnostics(cluster_name, &resources),
        Err(error) => vec![format!(
            "⚠️ CAPI status unavailable: {}",
            compact_message(&error.to_string())
        )],
    };
    log_message(logger, trigger, render_diagnostics_log_block(header, &lines));
}

fn render_diagnostics_log_block(header: &str, lines: &[String]) -> String {
    let mut block = Vec::with_capacity(lines.len() + 2);
    block.push(format!("CAPI╭─ {header}"));
    block.extend(lines.iter().map(|line| format!("CAPI│ {line}")));
    block.push("CAPI╰─ End of CAPI status snapshot".to_string());
    format!("\n{}", block.join("\n"))
}

fn log_message(logger: &impl InfraLogger, trigger: UpgradeDiagnosticsTrigger, message: String) {
    match trigger {
        UpgradeDiagnosticsTrigger::Periodic | UpgradeDiagnosticsTrigger::WorkflowCompletion => logger.info(message),
        UpgradeDiagnosticsTrigger::CommandFailure => logger.warn(message),
    }
}

fn collect_upgrade_diagnostic_resources(
    kube_client: &Client,
    cluster_name: &str,
    provider_mode: EksAnywhereProviderMode,
) -> Result<Vec<KubernetesResource>, UpgradeDiagnosticsError> {
    match block_on(timeout(
        KUBERNETES_API_DIAGNOSTICS_TIMEOUT,
        collect_upgrade_diagnostic_resources_async(kube_client, cluster_name, provider_mode),
    )) {
        Ok(result) => result,
        Err(_) => Err(UpgradeDiagnosticsError::Timeout {
            timeout_seconds: KUBERNETES_API_DIAGNOSTICS_TIMEOUT.as_secs(),
        }),
    }
}

async fn collect_upgrade_diagnostic_resources_async(
    kube_client: &Client,
    cluster_name: &str,
    provider_mode: EksAnywhereProviderMode,
) -> Result<Vec<KubernetesResource>, UpgradeDiagnosticsError> {
    let discovery = Discovery::new(kube_client.clone())
        .filter(diagnostic_api_groups(provider_mode))
        .run()
        .await
        .map_err(|source| UpgradeDiagnosticsError::Discovery {
            source: Box::new(source),
        })?;
    let mut resources = Vec::new();

    for &resource_kind in diagnostic_resource_kinds(provider_mode) {
        let api_group = discovery
            .get(resource_kind.api_group())
            .ok_or(UpgradeDiagnosticsError::MissingApiGroup {
                api_group: resource_kind.api_group(),
            })?;
        let (api_resource, capabilities) =
            api_group
                .recommended_kind(resource_kind.kind())
                .ok_or(UpgradeDiagnosticsError::MissingKind {
                    api_group: resource_kind.api_group(),
                    kind: resource_kind.kind(),
                })?;
        if !capabilities.supports_operation(verbs::LIST) {
            return Err(UpgradeDiagnosticsError::ListNotSupported {
                api_version: api_resource.api_version,
                kind: resource_kind.kind(),
            });
        }

        let api_version = api_resource.api_version.clone();
        let api: Api<DynamicObject> = Api::all_with(kube_client.clone(), &api_resource);
        let list_params = resource_kind.list_params(cluster_name);
        let resource_list = api
            .list(&list_params)
            .await
            .map_err(|source| UpgradeDiagnosticsError::List {
                api_version,
                kind: resource_kind.kind(),
                source: Box::new(source),
            })?;

        for resource in resource_list.items {
            resources.push(parse_dynamic_resource(resource, resource_kind)?);
        }
    }

    Ok(resources)
}

fn parse_dynamic_resource(
    resource: DynamicObject,
    resource_kind: DiagnosticResourceKind,
) -> Result<KubernetesResource, UpgradeDiagnosticsError> {
    let raw_resource = serde_json::to_value(resource).map_err(|source| UpgradeDiagnosticsError::InvalidResource {
        kind: resource_kind.kind(),
        source,
    })?;
    let mut resource = serde_json::from_value::<KubernetesResource>(raw_resource).map_err(|source| {
        UpgradeDiagnosticsError::InvalidResource {
            kind: resource_kind.kind(),
            source,
        }
    })?;
    resource.kind = resource_kind.diagnostic_kind().to_string();
    Ok(resource)
}

#[cfg(test)]
fn parse_capi_resources(raw_json: &str) -> Result<Vec<KubernetesResource>, UpgradeDiagnosticsError> {
    serde_json::from_str::<KubernetesResourceList>(raw_json)
        .map(|list| list.items)
        .map_err(|source| UpgradeDiagnosticsError::InvalidResource {
            kind: "test fixture",
            source,
        })
}

fn render_upgrade_diagnostics(cluster_name: &str, resources: &[KubernetesResource]) -> Vec<String> {
    let mut target_resources = resources
        .iter()
        .filter(|resource| resource.belongs_to_cluster(cluster_name))
        .collect::<Vec<_>>();
    target_resources.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.metadata.name.cmp(&right.metadata.name))
    });

    if target_resources.is_empty() {
        return vec![format!(
            "No EKS-A or CAPI resources found for cluster `{cluster_name}`."
        )];
    }

    let mut lines = Vec::new();
    let reconciliation_pending = target_resources.iter().any(|resource| {
        resource.kind == DiagnosticResourceKind::EksAnywhereCluster.diagnostic_kind()
            && resource.reconciliation_pending()
    });
    append_eksa_cluster_status(&mut lines, &target_resources);
    if reconciliation_pending {
        lines.push("ℹ️ Inspect `eksa-controller-manager` logs for the exact reconciliation failure.".to_string());
        lines.push(
            "⚠️ CAPI resources below reflect the last reconciled state and do not prove that the current EKS-A Cluster generation has converged."
                .to_string(),
        );
    }
    append_rollout_status(&mut lines, &target_resources, "KubeadmControlPlane");
    append_rollout_status(&mut lines, &target_resources, "MachineDeployment");
    append_machine_status(&mut lines, &target_resources);
    append_vsphere_machine_status(&mut lines, &target_resources);
    lines
}

fn append_eksa_cluster_status(lines: &mut Vec<String>, resources: &[&KubernetesResource]) {
    let clusters = resources
        .iter()
        .copied()
        .filter(|resource| resource.kind == DiagnosticResourceKind::EksAnywhereCluster.diagnostic_kind())
        .collect::<Vec<_>>();
    if clusters.is_empty() {
        lines.push("⚠️ EKS-A Cluster status is unavailable.".to_string());
        return;
    }

    for cluster in clusters {
        lines.push(format!(
            "EKS-A Cluster `{}`: generation={}, observed={}, reconciled={}.",
            cluster.display_name(),
            optional_number(cluster.metadata.generation),
            optional_number(cluster.status.observed_generation),
            optional_number(cluster.status.reconciled_generation),
        ));

        if cluster.reconciliation_pending() {
            lines.push(format!(
                "❌ EKS-A Cluster reconciliation is pending: desired generation {} has not been fully reconciled (observed {}, reconciled {}).",
                optional_number(cluster.metadata.generation),
                optional_number(cluster.status.observed_generation),
                optional_number(cluster.status.reconciled_generation),
            ));
            lines.push(
                "⚠️ EKS-A wait conditions are stale and describe the previously observed generation.".to_string(),
            );
        }

        let condition_summaries = EKSA_WAIT_CONDITION_TYPES.map(|condition_type| {
            cluster
                .conditions()
                .find(|condition| condition.condition_type == condition_type)
                .map_or_else(
                    || format!("{condition_type}=missing"),
                    |condition| format!("{condition_type}={}", condition.status),
                )
        });
        lines.push(format!("EKS-A wait conditions: {}.", condition_summaries.join(", ")));

        for condition in cluster.conditions().filter(|condition| {
            condition.status != "True" && EKSA_WAIT_CONDITION_TYPES.contains(&condition.condition_type.as_str())
        }) {
            let reason = condition.reason.as_deref().unwrap_or("unknown reason");
            let message = condition.message.as_deref().map(compact_message).unwrap_or_default();
            let message_suffix = if message.is_empty() {
                String::new()
            } else {
                format!(": {message}")
            };
            lines.push(format!(
                "⚠️ EKS-A condition `{}`={} ({reason}){message_suffix}",
                condition.condition_type, condition.status,
            ));
        }

        if cluster.status.failure_reason.is_some() || cluster.status.failure_message.is_some() {
            let reason = cluster.status.failure_reason.as_deref().unwrap_or("unknown reason");
            let message = cluster
                .status
                .failure_message
                .as_deref()
                .map(compact_message)
                .unwrap_or_default();
            lines.push(format!("⚠️ EKS-A Cluster failure ({reason}): {message}"));
        }
    }
}

fn append_rollout_status(lines: &mut Vec<String>, resources: &[&KubernetesResource], kind: &str) {
    let rollouts = resources
        .iter()
        .copied()
        .filter(|resource| resource.kind == kind)
        .collect::<Vec<_>>();
    if rollouts.is_empty() {
        return;
    }

    let converged_count = rollouts.iter().filter(|resource| resource.rollout_converged()).count();
    lines.push(format!("{kind}: {converged_count}/{} rollout(s) converged.", rollouts.len()));

    for resource in rollouts.into_iter().filter(|resource| !resource.rollout_converged()) {
        lines.push(format!(
            "⏳ {kind} `{}`: desired={}, current={}, ready={}, up-to-date={}, unavailable={}, phase={}.",
            resource.display_name(),
            optional_number(resource.spec.replicas),
            optional_number(resource.status.replicas),
            optional_number(resource.status.ready_replicas),
            optional_number(resource.up_to_date_replicas()),
            optional_number(resource.status.unavailable_replicas),
            resource.status.phase.as_deref().unwrap_or("unknown"),
        ));
    }
}

fn append_machine_status(lines: &mut Vec<String>, resources: &[&KubernetesResource]) {
    let machines = resources
        .iter()
        .copied()
        .filter(|resource| resource.kind == "Machine")
        .collect::<Vec<_>>();

    let deleting_machines = machines
        .iter()
        .copied()
        .filter(|machine| machine.metadata.deletion_timestamp.is_some())
        .collect::<Vec<_>>();
    lines.push(format!(
        "Machines: {} total, {} deleting.",
        machines.len(),
        deleting_machines.len()
    ));

    for machine in deleting_machines {
        let node_name = machine
            .status
            .node_ref
            .as_ref()
            .map(|node_ref| node_ref.name.as_str())
            .unwrap_or("not registered");
        lines.push(format!(
            "⏳ Machine `{}` is deleting node `{node_name}` since {}.",
            machine.display_name(),
            machine.metadata.deletion_timestamp.as_deref().unwrap_or("unknown time"),
        ));

        let blocking_conditions = machine.conditions().filter(|condition| {
            condition.status != "True"
                && matches!(
                    condition.condition_type.as_str(),
                    "DrainingSucceeded" | "VolumeDetachSucceeded" | "Deleting"
                )
        });
        for condition in blocking_conditions {
            let reason = condition.reason.as_deref().unwrap_or("unknown reason");
            let message = condition.message.as_deref().map(compact_message).unwrap_or_default();
            lines.push(format!(
                "⚠️ `{}`={} ({reason}): {message}",
                condition.condition_type, condition.status
            ));
        }
    }

    for machine in machines.into_iter().filter(|machine| {
        machine.metadata.deletion_timestamp.is_none()
            && machine.status.phase.as_deref().is_some_and(|phase| phase != "Running")
    }) {
        lines.push(format!(
            "⏳ Machine `{}`: phase={}, node={}, version={}.",
            machine.display_name(),
            machine.status.phase.as_deref().unwrap_or("unknown"),
            machine
                .status
                .node_ref
                .as_ref()
                .map(|node_ref| node_ref.name.as_str())
                .unwrap_or("not registered"),
            machine.spec.version.as_deref().unwrap_or("unknown"),
        ));
    }
}

fn append_vsphere_machine_status(lines: &mut Vec<String>, resources: &[&KubernetesResource]) {
    let machines = resources
        .iter()
        .copied()
        .filter(|resource| resource.kind == "VSphereMachine")
        .collect::<Vec<_>>();
    if machines.is_empty() {
        return;
    }

    let ready_count = machines
        .iter()
        .filter(|machine| machine.status.ready == Some(true))
        .count();
    let deleting_count = machines
        .iter()
        .filter(|machine| machine.metadata.deletion_timestamp.is_some())
        .count();
    lines.push(format!(
        "VSphereMachine: {ready_count}/{} ready, {deleting_count} deleting.",
        machines.len()
    ));

    for machine in machines.into_iter().filter(|machine| {
        machine.status.ready != Some(true)
            || machine.metadata.deletion_timestamp.is_some()
            || machine.status.failure_reason.is_some()
            || machine.status.failure_message.is_some()
    }) {
        lines.push(format!(
            "⏳ VSphereMachine `{}`: ready={}, provider-id={}, deleting={}.",
            machine.display_name(),
            optional_bool(machine.status.ready),
            machine.spec.provider_id.as_deref().unwrap_or("unknown"),
            machine.metadata.deletion_timestamp.is_some(),
        ));

        if machine.status.failure_reason.is_some() || machine.status.failure_message.is_some() {
            let reason = machine.status.failure_reason.as_deref().unwrap_or("unknown reason");
            let message = machine
                .status
                .failure_message
                .as_deref()
                .map(compact_message)
                .unwrap_or_default();
            lines.push(format!(
                "⚠️ VSphereMachine `{}` failure ({reason}): {message}",
                machine.display_name()
            ));
        }

        for condition in machine.conditions().filter(|condition| condition.status != "True") {
            let reason = condition.reason.as_deref().unwrap_or("unknown reason");
            let message = condition.message.as_deref().map(compact_message).unwrap_or_default();
            lines.push(format!(
                "⚠️ VSphereMachine `{}` condition `{}`={} ({reason}): {message}",
                machine.display_name(),
                condition.condition_type,
                condition.status,
            ));
        }
    }
}

fn optional_number(value: Option<i64>) -> String {
    value.map_or_else(|| "unknown".to_string(), |number| number.to_string())
}

fn optional_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

fn compact_message(message: &str) -> String {
    let compact = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_CONDITION_MESSAGE_LENGTH {
        return compact;
    }

    let truncated = compact.chars().take(MAX_CONDITION_MESSAGE_LENGTH).collect::<String>();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::{
        DiagnosticResourceKind, diagnostic_api_groups, diagnostic_resource_kinds, parse_capi_resources,
        parse_dynamic_resource, render_diagnostics_log_block, render_upgrade_diagnostics,
    };
    use crate::infrastructure::action::eksanywhere::provider::EksAnywhereProviderMode;
    use kube::api::DynamicObject;

    #[test]
    fn should_render_diagnostics_as_a_single_delimited_log_block() {
        let lines = vec![
            "MachineDeployment: 1/2 rollout(s) converged.".to_string(),
            "Machines: 3 total, 1 deleting.".to_string(),
        ];

        let output = render_diagnostics_log_block("🔎 Periodic CAPI status snapshot", &lines);

        assert_eq!(
            output,
            "\nCAPI╭─ 🔎 Periodic CAPI status snapshot\n\
CAPI│ MachineDeployment: 1/2 rollout(s) converged.\n\
CAPI│ Machines: 3 total, 1 deleting.\n\
CAPI╰─ End of CAPI status snapshot"
        );
    }

    #[test]
    fn should_only_query_vsphere_resources_for_vsphere_clusters() {
        assert!(diagnostic_api_groups(EksAnywhereProviderMode::Unknown).contains(&"anywhere.eks.amazonaws.com"));
        assert!(
            diagnostic_resource_kinds(EksAnywhereProviderMode::Unknown)
                .contains(&DiagnosticResourceKind::EksAnywhereCluster)
        );
        assert!(!diagnostic_api_groups(EksAnywhereProviderMode::Unknown).contains(&"infrastructure.cluster.x-k8s.io"));
        assert!(
            !diagnostic_resource_kinds(EksAnywhereProviderMode::Unknown)
                .contains(&DiagnosticResourceKind::VSphereMachine)
        );

        assert!(diagnostic_api_groups(EksAnywhereProviderMode::VSphere).contains(&"infrastructure.cluster.x-k8s.io"));
        assert!(
            diagnostic_resource_kinds(EksAnywhereProviderMode::VSphere)
                .contains(&DiagnosticResourceKind::VSphereMachine)
        );
    }

    #[test]
    fn should_parse_dynamic_capi_resource_using_discovered_kind() {
        let resource = serde_json::from_value::<DynamicObject>(serde_json::json!({
            "apiVersion": "cluster.x-k8s.io/v1beta1",
            "metadata": {
                "name": "eksa-bcef-system",
                "namespace": "eksa-system",
                "labels": {"cluster.x-k8s.io/cluster-name": "eksa-bcef"}
            },
            "spec": {"clusterName": "eksa-bcef", "replicas": 2},
            "status": {"replicas": 3, "readyReplicas": 2, "updatedReplicas": 1}
        }))
        .expect("dynamic CAPI fixture should parse");

        let resource = parse_dynamic_resource(resource, DiagnosticResourceKind::MachineDeployment)
            .expect("dynamic CAPI resource should convert");

        assert_eq!(resource.kind, "MachineDeployment");
        assert_eq!(resource.metadata.name, "eksa-bcef-system");
        assert_eq!(resource.spec.replicas, Some(2));
        assert_eq!(resource.status.updated_replicas, Some(1));
    }

    #[test]
    fn should_report_eksa_generation_and_wait_condition_blocker() {
        let resources = parse_capi_resources(
            r#"{
              "items": [{
                "kind": "EKSAnywhereCluster",
                "metadata": {
                  "name": "eksa-bcef",
                  "namespace": "default",
                  "generation": 12
                },
                "status": {
                  "observedGeneration": 11,
                  "reconciledGeneration": 11,
                  "conditions": [
                    {"type": "ControlPlaneReady", "status": "True"},
                    {"type": "DefaultCNIConfigured", "status": "True"},
                    {
                      "type": "WorkersReady",
                      "status": "False",
                      "reason": "RollingUpgradeInProgress",
                      "message": "Worker nodes not up-to-date yet, 3 upgrading (1 up to date)"
                    },
                    {"type": "Ready", "status": "False", "reason": "WorkersNotReady"}
                  ]
                }
              }]
            }"#,
        )
        .expect("fixture should parse");

        let output = render_upgrade_diagnostics("eksa-bcef", &resources).join("\n");

        assert!(output.contains("EKS-A Cluster `default/eksa-bcef`: generation=12, observed=11, reconciled=11"));
        assert!(output.contains("EKS-A Cluster reconciliation is pending"));
        assert!(output.contains("EKS-A wait conditions are stale"));
        assert!(output.contains("ControlPlaneReady=True, DefaultCNIConfigured=True, WorkersReady=False, Ready=False"));
        assert!(output.contains("EKS-A condition `WorkersReady`=False (RollingUpgradeInProgress)"));
        assert!(output.contains("Worker nodes not up-to-date yet, 3 upgrading (1 up to date)"));
    }

    #[test]
    fn should_report_stale_capi_state_and_controller_log_hint() {
        let resources = parse_capi_resources(
            r#"{
              "items": [
                {
                  "kind": "EKSAnywhereCluster",
                  "metadata": {
                    "name": "eksa-powens",
                    "namespace": "default",
                    "generation": 7
                  },
                  "status": {
                    "observedGeneration": 6,
                    "reconciledGeneration": 6,
                    "conditions": [
                      {"type": "ControlPlaneReady", "status": "True"},
                      {"type": "DefaultCNIConfigured", "status": "True"},
                      {"type": "WorkersReady", "status": "True"},
                      {"type": "Ready", "status": "True"}
                    ]
                  }
                },
                {
                  "kind": "KubeadmControlPlane",
                  "metadata": {
                    "name": "eksa-powens",
                    "namespace": "eksa-system",
                    "labels": {"cluster.x-k8s.io/cluster-name": "eksa-powens"}
                  },
                  "spec": {"replicas": 1},
                  "status": {
                    "replicas": 1,
                    "readyReplicas": 1,
                    "updatedReplicas": 1,
                    "unavailableReplicas": 0
                  }
                }
              ]
            }"#,
        )
        .expect("fixture should parse");

        let lines = render_upgrade_diagnostics("eksa-powens", &resources);
        let output = lines.join("\n");

        assert!(output.contains("generation=7, observed=6, reconciled=6"));
        assert!(output.contains("wait conditions are stale"));
        assert!(output.contains("Inspect `eksa-controller-manager` logs for the exact reconciliation failure"));
        assert!(output.contains("CAPI resources below reflect the last reconciled state"));
        assert!(output.contains("KubeadmControlPlane: 1/1 rollout(s) converged"));
        let stale_warning_position = lines
            .iter()
            .position(|line| line.contains("CAPI resources below reflect"))
            .expect("stale CAPI warning should be rendered");
        let rollout_position = lines
            .iter()
            .position(|line| line.starts_with("KubeadmControlPlane:"))
            .expect("rollout status should be rendered");
        assert!(stale_warning_position < rollout_position);
    }

    #[test]
    fn should_not_render_stale_warning_for_reconciled_eksa_generation() {
        let resources = parse_capi_resources(
            r#"{
              "items": [{
                "kind": "EKSAnywhereCluster",
                "metadata": {
                  "name": "eksa-powens",
                  "namespace": "default",
                  "generation": 8
                },
                "status": {
                  "observedGeneration": 8,
                  "reconciledGeneration": 8,
                  "conditions": [
                    {"type": "ControlPlaneReady", "status": "True"},
                    {"type": "DefaultCNIConfigured", "status": "True"},
                    {"type": "WorkersReady", "status": "True"},
                    {"type": "Ready", "status": "True"}
                  ]
                }
              }]
            }"#,
        )
        .expect("fixture should parse");

        let output = render_upgrade_diagnostics("eksa-powens", &resources).join("\n");

        assert!(output.contains("generation=8, observed=8, reconciled=8"));
        assert!(!output.contains("reconciliation is pending"));
        assert!(!output.contains("conditions are stale"));
        assert!(!output.contains("eksa-controller-manager"));
    }

    #[test]
    fn should_report_non_converged_rollout_and_pods_blocking_machine_drain() {
        let resources = parse_capi_resources(
            r#"{
              "items": [
                {
                  "kind": "MachineDeployment",
                  "metadata": {
                    "name": "eksa-bcef-system",
                    "namespace": "eksa-system",
                    "labels": {"cluster.x-k8s.io/cluster-name": "eksa-bcef"}
                  },
                  "spec": {"clusterName": "eksa-bcef", "replicas": 2},
                  "status": {
                    "phase": "ScalingDown",
                    "replicas": 3,
                    "readyReplicas": 2,
                    "upToDateReplicas": 1,
                    "unavailableReplicas": 0
                  }
                },
                {
                  "kind": "Machine",
                  "metadata": {
                    "name": "eksa-bcef-system-old",
                    "namespace": "eksa-system",
                    "labels": {"cluster.x-k8s.io/cluster-name": "eksa-bcef"},
                    "deletionTimestamp": "2026-07-16T15:05:00Z"
                  },
                  "spec": {"clusterName": "eksa-bcef", "version": "v1.34.1"},
                  "status": {
                    "phase": "Deleting",
                    "nodeRef": {"name": "eksa-bcef-system-old"},
                    "v1beta2": {
                      "conditions": [{
                        "type": "DrainingSucceeded",
                        "status": "False",
                        "reason": "Draining",
                        "message": "Drain not completed yet:\n* Pods with deletionTimestamp that still exist: apps/api-123"
                      }]
                    }
                  }
                },
                {
                  "kind": "MachineDeployment",
                  "metadata": {
                    "name": "other-cluster-workers",
                    "namespace": "eksa-system",
                    "labels": {"cluster.x-k8s.io/cluster-name": "other-cluster"}
                  },
                  "spec": {"clusterName": "other-cluster", "replicas": 1},
                  "status": {"replicas": 0, "readyReplicas": 0, "upToDateReplicas": 0}
                }
              ]
            }"#,
        )
        .expect("fixture should parse");

        let lines = render_upgrade_diagnostics("eksa-bcef", &resources);
        let output = lines.join("\n");

        assert!(output.contains("MachineDeployment: 0/1 rollout(s) converged"));
        assert!(output.contains("desired=2, current=3, ready=2, up-to-date=1"));
        assert!(output.contains("Machine `eksa-system/eksa-bcef-system-old` is deleting"));
        assert!(output.contains("Pods with deletionTimestamp that still exist: apps/api-123"));
        assert!(!output.contains("other-cluster-workers"));
    }

    #[test]
    fn should_report_converged_rollout_and_provisioning_machine() {
        let resources = parse_capi_resources(
            r#"{
              "items": [
                {
                  "kind": "KubeadmControlPlane",
                  "metadata": {
                    "name": "eksa-bcef",
                    "namespace": "eksa-system",
                    "labels": {"cluster.x-k8s.io/cluster-name": "eksa-bcef"}
                  },
                  "spec": {"replicas": 3},
                  "status": {
                    "replicas": 3,
                    "readyReplicas": 3,
                    "updatedReplicas": 3,
                    "unavailableReplicas": 0
                  }
                },
                {
                  "kind": "Machine",
                  "metadata": {
                    "name": "eksa-bcef-worker-new",
                    "namespace": "eksa-system",
                    "labels": {"cluster.x-k8s.io/cluster-name": "eksa-bcef"}
                  },
                  "spec": {"clusterName": "eksa-bcef", "version": "v1.35.0"},
                  "status": {"phase": "Provisioning"}
                }
              ]
            }"#,
        )
        .expect("fixture should parse");

        let output = render_upgrade_diagnostics("eksa-bcef", &resources).join("\n");

        assert!(output.contains("KubeadmControlPlane: 1/1 rollout(s) converged"));
        assert!(output.contains("phase=Provisioning, node=not registered, version=v1.35.0"));
    }

    #[test]
    fn should_report_vsphere_machine_provisioning_failure() {
        let resources = parse_capi_resources(
            r#"{
              "items": [{
                "kind": "VSphereMachine",
                "metadata": {
                  "name": "eksa-bcef-system-new",
                  "namespace": "eksa-system",
                  "labels": {"cluster.x-k8s.io/cluster-name": "eksa-bcef"}
                },
                "spec": {
                  "providerID": "vsphere://1234"
                },
                "status": {
                  "ready": false,
                  "failureReason": "CreateError",
                  "failureMessage": "cannot clone VM from the configured template",
                  "v1beta2": {
                    "conditions": [{
                      "type": "VirtualMachineProvisioned",
                      "status": "False",
                      "reason": "WaitingForNetworkAddress",
                      "message": "waiting for the VM network address"
                    }]
                  }
                }
              }]
            }"#,
        )
        .expect("fixture should parse");

        let output = render_upgrade_diagnostics("eksa-bcef", &resources).join("\n");

        assert!(output.contains("VSphereMachine: 0/1 ready, 0 deleting"));
        assert!(output.contains("provider-id=vsphere://1234"));
        assert!(output.contains("failure (CreateError): cannot clone VM"));
        assert!(output.contains("VirtualMachineProvisioned`=False (WaitingForNetworkAddress)"));
    }
}
