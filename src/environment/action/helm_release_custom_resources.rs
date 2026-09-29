// Deletes the custom resources of a Helm release before the release is uninstalled.
//
// `helm uninstall` removes every object of a release in one pass. When a chart ships an operator
// together with a custom resource that operator manages (SigNoz: the ClickHouse operator and a
// ClickHouseInstallation), the custom resource carries the operator's finalizer, the operator is
// deleted in the same pass, and nothing is left to clear the finalizer. The resource stays
// Terminating forever and `helm uninstall --wait` waits for it until the timeout.
//
// Deleting the custom resources first, while their controllers still run, lets each operator do
// its own cleanup. If some are still there after a bounded wait, the deletion fails before the
// uninstall, naming them: their operator keeps running and a retry starts clean. Finalizers are
// never removed here. Operators can manage resources outside the cluster (an ArgoCD Application's
// workloads, a Crossplane database), and stripping a finalizer would orphan them silently.
//
// Helm semantics this step must not break, and how it keeps them:
// - `helm.sh/resource-policy: keep`: Helm leaves such objects in place on uninstall. They are
//   skipped here too, finalizers included.
// - `pre-delete` hooks run before Helm deletes anything, and may need the custom resources (a
//   backup job reading a database, for instance). A release with a pre-delete hook is left
//   entirely to Helm, as before this step existed.
// - cancellation: checked before every destructive action. A canceled deletion stops here.
//
// Only resources still terminating after the wait fail the deletion. Any other problem (manifest
// or discovery unreadable, a delete call rejected) is logged and the uninstall runs as before.

use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{Helm, HelmError};
use crate::environment::models::abort::Abort;
use crate::helm::ChartInfo;
use crate::runtime::block_on;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::Client;
use kube::api::{Api, ApiResource, DeleteParams, DynamicObject};
use kube::core::GroupVersionKind;
use kube::discovery::{Scope, pinned_kind};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::time::{Duration, Instant};

/// How long operators get to clean up their resources before the deletion fails, counted from the
/// moment the last delete call returned.
///
/// This step runs before `helm uninstall` and does not eat into its timeout: the uninstall's
/// CommandKiller starts afterwards. A deletion that needs the whole step therefore takes up to
/// DISCOVERY_BUDGET + this + the Helm timeout. Releases without custom resources skip it.
pub const CUSTOM_RESOURCES_DELETION_TIMEOUT: Duration = Duration::from_secs(300);
/// How long finding the release's custom resources may take. Past it, nothing is deleted and the
/// release is left to helm uninstall as before: a partial list would still hang the uninstall.
const DISCOVERY_BUDGET: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Upper bound of any single Kubernetes call made here. kube's own read timeout is 295s, so a
/// stalled API server would otherwise make one call outlast the whole cleanup budget.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const HELM_RESOURCE_POLICY_ANNOTATION: &str = "helm.sh/resource-policy";
const HELM_HOOK_ANNOTATION: &str = "helm.sh/hook";

/// An object of the release manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseObject {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    /// Namespace written in the manifest; `None` means the release namespace.
    pub namespace: Option<String>,
    /// `helm.sh/resource-policy: keep`: Helm leaves the object in place on uninstall.
    pub kept_by_helm: bool,
}

impl ReleaseObject {
    fn gvk(&self) -> GroupVersionKind {
        let (group, version) = match self.api_version.split_once('/') {
            Some((group, version)) => (group, version),
            None => ("", self.api_version.as_str()),
        };
        GroupVersionKind::gvk(group, version, &self.kind)
    }

    fn display(&self) -> String {
        format!("{}/{}", self.kind, self.name)
    }
}

#[derive(Deserialize)]
struct ManifestDocument {
    #[serde(rename = "apiVersion")]
    api_version: Option<String>,
    kind: Option<String>,
    metadata: Option<ManifestMetadata>,
}

#[derive(Deserialize)]
struct ManifestMetadata {
    name: Option<String>,
    namespace: Option<String>,
    // `annotations:` with no value is a valid, empty map.
    #[serde(default, deserialize_with = "null_as_empty")]
    annotations: BTreeMap<String, String>,
}

fn null_as_empty<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error> {
    Ok(Option::<BTreeMap<String, String>>::deserialize(deserializer)?.unwrap_or_default())
}

/// Every object of a `helm get manifest` output that lives in an API group, that is everything
/// but the core `v1` group, which holds no custom resource. Whether an object is a custom resource
/// is only known from the cluster (see `is_served_by_a_crd`): groups ending in `k8s.io` can be
/// CRDs too (`gateway.networking.k8s.io`), so the group name alone cannot tell.
pub fn grouped_objects_from_manifest(manifest: &str) -> Vec<ReleaseObject> {
    parse_manifest(manifest)
        .into_iter()
        .map(|(object, _)| object)
        .filter(|object| object.api_version.contains('/'))
        .collect()
}

/// Whether a `helm get hooks` output declares a `pre-delete` hook, or may: a hook document this
/// parser cannot read (YAML Helm accepts but serde_yaml rejects, such as duplicate keys) counts as
/// one, so the cleanup never runs ahead of a hook it could not rule out. Hook events are compared
/// the way Helm does, trimmed and lowercased, on every document: a hook may carry
/// `metadata.generateName` instead of a name.
pub fn has_pre_delete_hook(hooks_manifest: &str) -> bool {
    split_yaml_documents(hooks_manifest).any(|document| match parse_document(document) {
        None => true,
        Some(document) => document
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.annotations.get(HELM_HOOK_ANNOTATION))
            .is_some_and(|hooks| {
                hooks
                    .split(',')
                    .any(|hook| hook.trim().eq_ignore_ascii_case("pre-delete"))
            }),
    })
}

/// Documents are split on Helm's `---` separator and parsed one by one, rather than through
/// serde_yaml's multi-document iterator: after a syntax error that iterator yields the same error
/// forever, and one broken document would make the loop never end.
///
/// Merge keys (`<<: *anchor`) are expanded first, as Helm does, so an annotation inherited that
/// way (a keep policy, a hook) is seen.
fn parse_documents(manifest: &str) -> Vec<ManifestDocument> {
    split_yaml_documents(manifest).filter_map(parse_document).collect()
}

fn parse_document(document: &str) -> Option<ManifestDocument> {
    let mut value = serde_yaml::from_str::<serde_yaml::Value>(document).ok()?;
    value.apply_merge().ok()?;
    serde_yaml::from_value::<ManifestDocument>(value).ok()
}

fn parse_manifest(manifest: &str) -> Vec<(ReleaseObject, BTreeMap<String, String>)> {
    parse_documents(manifest)
        .into_iter()
        .filter_map(|document| {
            let metadata = document.metadata?;
            let object = ReleaseObject {
                api_version: document.api_version?,
                kind: document.kind?,
                name: metadata.name?,
                // Helm installs an object with an empty namespace in the release namespace.
                namespace: metadata.namespace.filter(|namespace| !namespace.is_empty()),
                // Helm lowercases the policy before comparing it (Keep and KEEP keep too).
                kept_by_helm: metadata
                    .annotations
                    .get(HELM_RESOURCE_POLICY_ANNOTATION)
                    .is_some_and(|policy| policy.trim().eq_ignore_ascii_case("keep")),
            };
            Some((object, metadata.annotations))
        })
        .collect()
}

fn split_yaml_documents(manifest: &str) -> impl Iterator<Item = &str> {
    let mut documents = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for line in manifest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            documents.push(&manifest[start..offset]);
            start = offset + line.len();
        }
        offset += line.len();
    }
    documents.push(&manifest[start..]);
    documents.into_iter().filter(|document| !document.trim().is_empty())
}

/// Runs one Kubernetes call, bounded by `limit` (at most REQUEST_TIMEOUT). `None` means the
/// deletion was canceled before the call, or the call timed out.
fn call<F: Future>(abort: &dyn Abort, limit: Duration, request: F) -> Option<F::Output> {
    if abort.status().should_cancel() {
        return None;
    }
    let limit = limit.min(REQUEST_TIMEOUT);
    block_on(async { tokio::time::timeout(limit, request).await }).ok()
}

/// Custom resources are the objects whose type a CustomResourceDefinition serves, named
/// `<plural>.<group>`. Built-in types (apps, networking.k8s.io, ...) have no CRD.
fn is_served_by_a_crd(crds: &Api<CustomResourceDefinition>, resource: &ApiResource, abort: &dyn Abort) -> bool {
    if resource.group.is_empty() {
        return false;
    }
    matches!(
        call(
            abort,
            REQUEST_TIMEOUT,
            crds.get_opt(&format!("{}.{}", resource.plural, resource.group))
        ),
        Some(Ok(Some(_)))
    )
}

/// What the uninstall that follows should do.
#[derive(Debug, PartialEq, Eq)]
pub enum CustomResourcesCleanup {
    /// Nothing is left that would hang `helm uninstall` (or the step was skipped or canceled).
    Proceed,
    /// These objects were still terminating after the wait: uninstalling now would delete their
    /// operator and leave them stuck. The deletion must fail instead.
    Stuck(Vec<StuckResource>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StuckResource {
    pub kind: String,
    pub name: String,
    pub namespace: String,
}

/// The message of the failed deletion: which objects, why nothing was forced, how to unblock.
pub fn stuck_resources_message(release: &str, timeout: Duration, stuck: &[StuckResource]) -> String {
    let names = stuck
        .iter()
        .map(|r| format!("{}/{} (namespace {})", r.kind, r.name, r.namespace))
        .collect::<Vec<_>>()
        .join(", ");
    let example = stuck
        .first()
        .map(|r| {
            format!(
                "kubectl -n {} patch {} {} --type=merge -p '{{\"metadata\":{{\"finalizers\":null}}}}'",
                r.namespace,
                r.kind.to_lowercase(),
                r.name
            )
        })
        .unwrap_or_default();
    format!(
        "Release {release} was not uninstalled: {names} still terminating after {}s. Their operator \
         has not finished its cleanup, which may involve resources outside the cluster, so the uninstall \
         was not started and the operator keeps running. Retry the deletion once the operator is healthy. \
         To skip that cleanup and accept what it leaves behind, remove the finalizers by hand, e.g. `{example}`, \
         then retry.",
        timeout.as_secs()
    )
}

/// Deletes the release's namespaced custom resources and waits for them to go away. Reports those
/// still terminating after `timeout`; never removes a finalizer. Other problems are logged through
/// `warn` and end in `Proceed`.
pub fn delete_release_custom_resources(
    helm: &Helm,
    chart: &ChartInfo,
    client: &Client,
    abort: &dyn Abort,
    timeout: Duration,
    info: &mut dyn FnMut(String),
    warn: &mut dyn FnMut(String),
) -> CustomResourcesCleanup {
    // helm reads are bounded too: a stalled credential plugin must not hold the deletion.
    let helm_killer = CommandKiller::from(DISCOVERY_BUDGET, abort);
    if abort.status().should_cancel() {
        return CustomResourcesCleanup::Proceed;
    }
    let manifest = match helm.get_release_manifest(chart, &[], &helm_killer) {
        Ok(manifest) => manifest,
        Err(HelmError::ReleaseDoesNotExist(_)) => return CustomResourcesCleanup::Proceed,
        Err(_) if abort.status().should_cancel() => return CustomResourcesCleanup::Proceed,
        Err(err) => {
            warn(format!(
                "Cannot read the manifest of release {} to delete its custom resources first: {err}",
                chart.name
            ));
            return CustomResourcesCleanup::Proceed;
        }
    };

    let candidates = grouped_objects_from_manifest(&manifest);
    if candidates.is_empty() {
        return CustomResourcesCleanup::Proceed;
    }

    match helm.get_release_hooks(chart, &[], &helm_killer) {
        Ok(hooks) if has_pre_delete_hook(&hooks) => {
            info(format!(
                "Release {} has a pre-delete hook: leaving its custom resources to helm uninstall",
                chart.name
            ));
            return CustomResourcesCleanup::Proceed;
        }
        Ok(_) => {}
        Err(_) if abort.status().should_cancel() => return CustomResourcesCleanup::Proceed,
        Err(err) => {
            // Without the hooks, the pre-delete ordering cannot be guaranteed: do nothing.
            warn(format!(
                "Cannot read the hooks of release {}, leaving its custom resources to helm uninstall: {err}",
                chart.name
            ));
            return CustomResourcesCleanup::Proceed;
        }
    }

    let discovery_deadline = Instant::now() + DISCOVERY_BUDGET;
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let release_namespace = chart.get_namespace_string();
    // Discovery and CRD lookup once per kind, negative answers included: a chart can hold many
    // objects of the same kind, built-in ones mostly.
    let mut custom_kinds: HashMap<(String, String), Option<ApiResource>> = HashMap::new();
    let mut apis: Vec<(ReleaseObject, Api<DynamicObject>)> = Vec::new();
    for object in candidates {
        if object.kept_by_helm {
            info(format!(
                "Keeping {}: it is annotated {HELM_RESOURCE_POLICY_ANNOTATION}: keep",
                object.display()
            ));
            continue;
        }
        if abort.status().should_cancel() {
            return CustomResourcesCleanup::Proceed;
        }
        let left = discovery_deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            warn(format!(
                "Finding the custom resources of release {} took over {}s: leaving them to helm uninstall",
                chart.name,
                DISCOVERY_BUDGET.as_secs()
            ));
            return CustomResourcesCleanup::Proceed;
        }
        let key = (object.api_version.clone(), object.kind.clone());
        let resource = match custom_kinds.get(&key) {
            Some(known) => known.clone(),
            None => {
                let found = match call(abort, left, pinned_kind(client, &object.gvk())) {
                    // Cluster-scoped resources do not block the namespace; they are left to Helm.
                    Some(Ok((resource, capabilities)))
                        if capabilities.scope == Scope::Namespaced && is_served_by_a_crd(&crds, &resource, abort) =>
                    {
                        Some(resource)
                    }
                    // Built-in or cluster-scoped: nothing to do before helm uninstall.
                    Some(Ok(_)) => None,
                    // Not served, or discovery failed or timed out. Skipping keeps the previous
                    // behaviour for this kind, but it is logged: if it is a finalized custom
                    // resource, the uninstall may still hang on it.
                    Some(Err(err)) => {
                        warn(format!(
                            "Cannot resolve the API of {}, leaving it to helm uninstall: {err}",
                            object.display()
                        ));
                        None
                    }
                    None if abort.status().should_cancel() => return CustomResourcesCleanup::Proceed,
                    None => {
                        warn(format!(
                            "Resolving the API of {} timed out, leaving it to helm uninstall",
                            object.display()
                        ));
                        None
                    }
                };
                custom_kinds.insert(key, found.clone());
                found
            }
        };
        let Some(resource) = resource else {
            continue;
        };
        let namespace = object.namespace.clone().unwrap_or_else(|| release_namespace.clone());
        apis.push((object, Api::namespaced_with(client.clone(), &namespace, &resource)));
    }

    if apis.is_empty() {
        return CustomResourcesCleanup::Proceed;
    }

    for (object, api) in &apis {
        if abort.status().should_cancel() {
            return CustomResourcesCleanup::Proceed;
        }
        info(format!(
            "Deleting {} before uninstalling release {}, so its controller can clean it up",
            object.display(),
            chart.name
        ));
        // Foreground, like `helm uninstall --cascade=foreground`: the object only goes away once
        // its own dependents have, so the operator is kept until they no longer need it.
        match call(abort, REQUEST_TIMEOUT, api.delete(&object.name, &DeleteParams::foreground())) {
            Some(Err(err)) if !is_not_found(&err) => warn(format!("Cannot delete {}: {err}", object.display())),
            None if !abort.status().should_cancel() => warn(format!(
                "Deleting {} timed out after {}s",
                object.display(),
                REQUEST_TIMEOUT.as_secs()
            )),
            _ => {}
        }
    }

    // The operators' grace period starts once every delete call has returned, so slow discovery
    // or slow deletes never shorten it.
    let deadline = Instant::now() + timeout;
    let mut remaining = still_present(&apis, abort, deadline);
    while !remaining.is_empty() && Instant::now() < deadline {
        if abort.status().should_cancel() {
            return CustomResourcesCleanup::Proceed;
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
        remaining = still_present(&apis, abort, deadline);
    }

    if remaining.is_empty() || abort.status().should_cancel() {
        return CustomResourcesCleanup::Proceed;
    }
    CustomResourcesCleanup::Stuck(
        remaining
            .into_iter()
            .map(|(object, _)| StuckResource {
                kind: object.kind.clone(),
                name: object.name.clone(),
                namespace: object.namespace.clone().unwrap_or_else(|| release_namespace.clone()),
            })
            .collect(),
    )
}

/// The objects not gone yet. Each read is bounded by what is left of `deadline`; one that fails,
/// times out or is skipped because of cancellation or the deadline counts as still present.
fn still_present<'a>(
    apis: &'a [(ReleaseObject, Api<DynamicObject>)],
    abort: &dyn Abort,
    deadline: Instant,
) -> Vec<&'a (ReleaseObject, Api<DynamicObject>)> {
    apis.iter()
        .filter(|(object, api)| {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            !matches!(call(abort, left, api.get_opt(&object.name)), Some(Ok(None)))
        })
        .collect()
}

fn is_not_found(err: &kube::Error) -> bool {
    matches!(err, kube::Error::Api(response) if response.code == 404)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::models::abort::AbortStatus;

    const SIGNOZ_LIKE_MANIFEST: &str = r#"
---
# Source: signoz/charts/clickhouse/templates/clickhouse-operator/serviceaccount.yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: signoz-clickhouse-operator
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: signoz-clickhouse-operator
---
apiVersion: clickhouse.altinity.com/v1
kind: ClickHouseInstallation
metadata:
  name: signoz-clickhouse
  namespace: z1234-signoz
---
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: signoz
---
apiVersion: postgresql.cnpg.io/v1
kind: Cluster
metadata:
  name: signoz-db
  annotations:
    helm.sh/resource-policy: keep
---
# an empty document, as Helm emits for templates that render nothing
"#;

    fn object(api_version: &str, kind: &str, name: &str, namespace: Option<&str>, kept: bool) -> ReleaseObject {
        ReleaseObject {
            api_version: api_version.into(),
            kind: kind.into(),
            name: name.into(),
            namespace: namespace.map(Into::into),
            kept_by_helm: kept,
        }
    }

    #[test]
    fn keeps_every_grouped_object_as_a_candidate() {
        // Only the cluster can say which of these are custom resources; the core v1 group never is.
        assert_eq!(
            grouped_objects_from_manifest(SIGNOZ_LIKE_MANIFEST),
            vec![
                object("apps/v1", "Deployment", "signoz-clickhouse-operator", None, false),
                object(
                    "clickhouse.altinity.com/v1",
                    "ClickHouseInstallation",
                    "signoz-clickhouse",
                    Some("z1234-signoz"),
                    false
                ),
                object("gateway.networking.k8s.io/v1", "HTTPRoute", "signoz", None, false),
                object("postgresql.cnpg.io/v1", "Cluster", "signoz-db", None, true),
            ]
        );
    }

    #[test]
    fn detects_pre_delete_hooks() {
        let hooks = r#"
---
apiVersion: batch/v1
kind: Job
metadata:
  name: backup
  annotations:
    "helm.sh/hook": post-install, pre-delete
"#;
        assert!(has_pre_delete_hook(hooks));
        assert!(!has_pre_delete_hook(
            "---\napiVersion: batch/v1\nkind: Job\nmetadata:\n  name: migrate\n  annotations:\n    helm.sh/hook: pre-upgrade\n"
        ));
        assert!(!has_pre_delete_hook(""));
    }

    #[test]
    fn detects_pre_delete_hooks_the_way_helm_parses_them() {
        // generateName instead of name, and an event in another case: Helm still runs it.
        let hooks = r#"
---
apiVersion: batch/v1
kind: Job
metadata:
  generateName: backup-
  annotations:
    "helm.sh/hook": " Pre-Delete "
"#;
        assert!(has_pre_delete_hook(hooks));
    }

    #[test]
    fn a_stalled_call_is_cut_at_its_limit() {
        let started = Instant::now();
        let result = call(&|| AbortStatus::None, Duration::from_millis(200), std::future::pending::<()>());
        assert!(result.is_none());
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }

    #[test]
    fn a_canceled_deletion_makes_no_call() {
        let result = call(&|| AbortStatus::Requested, REQUEST_TIMEOUT, async { 42 });
        assert!(result.is_none());
        assert_eq!(call(&|| AbortStatus::None, REQUEST_TIMEOUT, async { 42 }), Some(42));
    }

    #[test]
    fn protections_inherited_through_merge_keys_are_seen() {
        let manifest = r#"
apiVersion: postgresql.cnpg.io/v1
kind: Cluster
metadata:
  name: db
  annotations:
    <<: &protected
      helm.sh/resource-policy: keep
    team: data
"#;
        assert!(grouped_objects_from_manifest(manifest)[0].kept_by_helm);

        let hooks = r#"
apiVersion: batch/v1
kind: Job
metadata:
  name: backup
  annotations:
    <<: &hook
      helm.sh/hook: pre-delete
"#;
        assert!(has_pre_delete_hook(hooks));
    }

    #[test]
    fn null_annotations_and_empty_namespace_are_valid() {
        let manifest = "apiVersion: clickhouse.altinity.com/v1\nkind: ClickHouseInstallation\nmetadata:\n  name: chi\n  namespace: \"\"\n  annotations:\n";
        let found = grouped_objects_from_manifest(manifest);
        assert_eq!(found.len(), 1, "a null annotations map must not drop the object");
        assert_eq!(found[0].namespace, None, "an empty namespace means the release namespace");
    }

    #[test]
    fn an_unreadable_hook_counts_as_a_pre_delete_hook() {
        // Duplicate keys: Helm renders it, serde_yaml refuses it. The cleanup must not run first.
        let hooks =
            "apiVersion: batch/v1\nkind: Job\nmetadata:\n  name: backup\nspec:\n  backoffLimit: 1\n  backoffLimit: 2\n";
        assert!(has_pre_delete_hook(hooks));
    }

    #[test]
    fn stuck_message_names_the_objects_and_the_way_out() {
        let stuck = vec![
            StuckResource {
                kind: "ClickHouseInstallation".into(),
                name: "signoz-clickhouse".into(),
                namespace: "z1234-signoz".into(),
            },
            StuckResource {
                kind: "Application".into(),
                name: "app".into(),
                namespace: "argocd".into(),
            },
        ];
        let message = stuck_resources_message("helm-z1-signoz", Duration::from_secs(300), &stuck);
        assert!(
            message.contains("ClickHouseInstallation/signoz-clickhouse (namespace z1234-signoz)"),
            "{message}"
        );
        assert!(message.contains("Application/app (namespace argocd)"), "{message}");
        assert!(message.contains("after 300s"), "{message}");
        assert!(
            message.contains(r#"kubectl -n z1234-signoz patch clickhouseinstallation signoz-clickhouse --type=merge -p '{"metadata":{"finalizers":null}}'"#),
            "{message}"
        );
    }

    #[test]
    fn keep_policy_is_case_insensitive() {
        for policy in ["keep", "Keep", "KEEP", " keep "] {
            let manifest = format!(
                "apiVersion: postgresql.cnpg.io/v1\nkind: Cluster\nmetadata:\n  name: db\n  annotations:\n    helm.sh/resource-policy: \"{policy}\"\n"
            );
            assert!(grouped_objects_from_manifest(&manifest)[0].kept_by_helm, "{policy:?}");
        }
    }

    #[test]
    fn empty_or_invalid_manifest_has_no_candidates() {
        assert!(grouped_objects_from_manifest("").is_empty());
        assert!(grouped_objects_from_manifest("not: [valid").is_empty());
        assert!(grouped_objects_from_manifest("---\nkind: Foo\n").is_empty());
    }

    #[test]
    fn a_broken_document_does_not_hide_the_next_ones() {
        let manifest = "not: [valid\n---\napiVersion: clickhouse.altinity.com/v1\nkind: ClickHouseInstallation\nmetadata:\n  name: chi\n";
        let found = grouped_objects_from_manifest(manifest);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "chi");
    }

    #[test]
    fn gvk_splits_group_and_version() {
        assert_eq!(
            object("clickhouse.altinity.com/v1", "ClickHouseInstallation", "x", None, false).gvk(),
            GroupVersionKind::gvk("clickhouse.altinity.com", "v1", "ClickHouseInstallation")
        );
    }

    fn real_cluster() -> (Helm, Client, ChartInfo, String) {
        let kubeconfig = std::path::PathBuf::from(std::env::var("QOVERY_TEST_KUBECONFIG").unwrap());
        let release = std::env::var("QOVERY_TEST_RELEASE").unwrap();
        let namespace = std::env::var("QOVERY_TEST_NAMESPACE").unwrap();
        let helm = Helm::new(Some(&kubeconfig), &[]).unwrap();
        let client = block_on(async {
            let config = kube::Config::from_custom_kubeconfig(
                kube::config::Kubeconfig::read_from(&kubeconfig).unwrap(),
                &kube::config::KubeConfigOptions::default(),
            )
            .await
            .unwrap();
            Client::try_from(config).unwrap()
        });
        let chart = ChartInfo::new_from_release_name(&release, &namespace);
        (helm, client, chart, namespace)
    }

    fn live_custom_resources(client: &Client, helm: &Helm, chart: &ChartInfo, namespace: &str) -> Vec<String> {
        let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
        grouped_objects_from_manifest(&helm.get_release_manifest(chart, &[], &CommandKiller::never()).unwrap())
            .into_iter()
            .filter_map(|object| {
                let (resource, _) = block_on(pinned_kind(client, &object.gvk())).ok()?;
                if !is_served_by_a_crd(&crds, &resource, &|| AbortStatus::None) {
                    return None;
                }
                let ns = object.namespace.clone().unwrap_or_else(|| namespace.to_string());
                let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), &ns, &resource);
                block_on(api.get_opt(&object.name)).unwrap().map(|_| object.display())
            })
            .collect()
    }

    /// Against a real cluster: `QOVERY_TEST_KUBECONFIG` must point at a cluster where release
    /// `QOVERY_TEST_RELEASE` in namespace `QOVERY_TEST_NAMESPACE` has at least one custom resource
    /// guarded by a finalizer (e.g. the SigNoz chart). A canceled run must leave everything in
    /// place; a normal run must leave no custom resource. Run with `--ignored`.
    #[test]
    #[ignore]
    fn deletes_custom_resources_of_a_real_release() {
        let (helm, client, chart, namespace) = real_cluster();
        let before = live_custom_resources(&client, &helm, &chart, &namespace);
        assert!(!before.is_empty(), "the release has no custom resource to test with");

        let canceled = || AbortStatus::Requested;
        let canceled_outcome = delete_release_custom_resources(
            &helm,
            &chart,
            &client,
            &canceled,
            Duration::from_secs(60),
            &mut |line| println!("INFO {line}"),
            &mut |line| println!("WARN {line}"),
        );
        assert_eq!(canceled_outcome, CustomResourcesCleanup::Proceed);
        assert_eq!(
            live_custom_resources(&client, &helm, &chart, &namespace),
            before,
            "a canceled run must not delete anything"
        );

        let started = Instant::now();
        let not_canceled = || AbortStatus::None;
        let outcome = delete_release_custom_resources(
            &helm,
            &chart,
            &client,
            &not_canceled,
            Duration::from_secs(60),
            &mut |line| println!("INFO {line}"),
            &mut |line| println!("WARN {line}"),
        );
        println!("took {:?}", started.elapsed());
        assert_eq!(outcome, CustomResourcesCleanup::Proceed, "a healthy operator finishes in time");
        assert!(
            live_custom_resources(&client, &helm, &chart, &namespace).is_empty(),
            "custom resources left"
        );
    }

    /// Against a real cluster: release `QOVERY_TEST_RELEASE` in `QOVERY_TEST_NAMESPACE` holds a
    /// custom resource whose finalizer nobody clears (no controller for it). The step must report
    /// it as stuck after the timeout and leave its finalizer in place. Run with `--ignored`.
    #[test]
    #[ignore]
    fn reports_a_stuck_custom_resource_without_forcing_it() {
        let (helm, client, chart, namespace) = real_cluster();
        let not_canceled = || AbortStatus::None;
        let outcome = delete_release_custom_resources(
            &helm,
            &chart,
            &client,
            &not_canceled,
            Duration::from_secs(15),
            &mut |line| println!("INFO {line}"),
            &mut |line| println!("WARN {line}"),
        );
        let CustomResourcesCleanup::Stuck(stuck) = outcome else {
            panic!("expected a stuck resource, got {outcome:?}");
        };
        println!("{}", stuck_resources_message(&chart.name, Duration::from_secs(15), &stuck));
        assert_eq!(
            live_custom_resources(&client, &helm, &chart, &namespace).len(),
            stuck.len(),
            "stuck objects must still exist, with their finalizers"
        );
    }
}
