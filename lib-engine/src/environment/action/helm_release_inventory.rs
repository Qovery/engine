//! Inventory of the Helm releases of a namespace, read from the metadata of the Helm release Secrets.
//!
//! `helm list -a` downloads, decodes and decompresses every revision of every release in the engine
//! process, which is what exhausts its memory in a crowded namespace. Helm keeps the name, the revision
//! and the status of each revision as labels of its Secret, so listing the Secret metadata is enough to
//! know which releases are installed and costs a few hundred bytes per revision.

use crate::errors::CommandError;
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::Secret;
use kube::api::{Api, ListParams};
use std::collections::BTreeMap;
use tracing::warn;

/// Label Helm puts on every release Secret.
const OWNER_SELECTOR: &str = "owner=helm";
const NAME_LABEL: &str = "name";
const REVISION_LABEL: &str = "version";
const STATUS_LABEL: &str = "status";
/// Secrets are named `sh.helm.release.v1.<release>.v<revision>`.
const SECRET_NAME_PREFIX: &str = "sh.helm.release.v1.";
/// Pages are small on purpose: the API server only streams a page at a time.
const PAGE_SIZE: u32 = 500;
/// Release storage backends of Helm that keep the releases in Secrets (the default one).
const SECRET_STORAGE_DRIVERS: [&str; 3] = ["", "secret", "secrets"];

/// Status of a release revision, see `helm.sh/helm/v3/pkg/release/status.go`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HelmReleaseStatus {
    Unknown,
    Deployed,
    Uninstalled,
    Superseded,
    Failed,
    Uninstalling,
    PendingInstall,
    PendingUpgrade,
    PendingRollback,
    /// A status this engine does not know: the release is installed all the same, like `helm list -a` shows it.
    Other(String),
}

impl From<&str> for HelmReleaseStatus {
    fn from(status: &str) -> Self {
        match status {
            "unknown" => Self::Unknown,
            "deployed" => Self::Deployed,
            "uninstalled" => Self::Uninstalled,
            "superseded" => Self::Superseded,
            "failed" => Self::Failed,
            "uninstalling" => Self::Uninstalling,
            "pending-install" => Self::PendingInstall,
            "pending-upgrade" => Self::PendingUpgrade,
            "pending-rollback" => Self::PendingRollback,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Latest revision of a release of the namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HelmRelease {
    pub(super) name: String,
    pub(super) revision: u64,
    pub(super) status: HelmReleaseStatus,
}

impl HelmRelease {
    /// Reads a revision from the labels of its Secret.
    /// `None` for a Secret that is not a Helm release revision, even if it carries the `owner=helm` label:
    /// its name has to be the one Helm derives from the labels.
    fn from_secret_metadata(secret_name: &str, labels: &BTreeMap<String, String>) -> Option<Self> {
        let name = labels.get(NAME_LABEL)?;
        let revision: u64 = labels.get(REVISION_LABEL)?.parse().ok()?;
        let status = labels.get(STATUS_LABEL)?;
        (secret_name == format!("{SECRET_NAME_PREFIX}{name}.v{revision}")).then(|| Self {
            name: name.clone(),
            revision,
            status: HelmReleaseStatus::from(status.as_str()),
        })
    }
}

fn command_error(context: &str, error: impl std::fmt::Display) -> CommandError {
    CommandError::new_from_safe_message(format!("{context}: {error}"))
}

/// Fails unless the releases are stored in Secrets, the only storage whose metadata is read here.
///
/// `driver` is the `HELM_DRIVER` the Helm subprocesses run with. Nothing in the engine sets it, but the
/// process environment is inherited, and reading the Secrets of a cluster whose releases live in ConfigMaps
/// would report no release at all, so their cleanup would be skipped without a trace.
fn ensure_secret_storage(driver: Option<&str>) -> Result<(), CommandError> {
    match driver {
        Some(driver) if !SECRET_STORAGE_DRIVERS.contains(&driver) => Err(CommandError::new_from_safe_message(format!(
            "Helm releases are stored with the `{driver}` driver (HELM_DRIVER), \
             only the `secret` driver can be inventoried from the Secret metadata"
        ))),
        Some(_) | None => Ok(()),
    }
}

fn page_params(continue_token: Option<&str>) -> ListParams {
    let params = ListParams::default().labels(OWNER_SELECTOR).limit(PAGE_SIZE);
    match continue_token {
        Some(token) => params.continue_token(token),
        None => params,
    }
}

/// Latest revision of every release of `namespace`, whatever its status (`helm list -a`).
///
/// Lists the metadata of the Helm Secrets only, never their payload, following every page.
/// A page that cannot be read fails the whole inventory: a partial one would make installed releases look absent.
fn list_helm_releases(client: kube::Client, namespace: &str) -> Result<Vec<HelmRelease>, CommandError> {
    let secrets: Api<Secret> = Api::namespaced(client, namespace);
    let failure =
        |error: kube::Error| command_error(&format!("Failed to list the Helm releases of `{namespace}`"), error);

    let mut latest: BTreeMap<String, HelmRelease> = BTreeMap::new();
    let mut ignored = 0_usize;
    let mut continue_token: Option<String> = None;
    loop {
        let page = block_on(secrets.list_metadata(&page_params(continue_token.as_deref()))).map_err(failure)?;
        for item in page.items {
            let release = item
                .metadata
                .name
                .as_deref()
                .zip(item.metadata.labels.as_ref())
                .and_then(|(name, labels)| HelmRelease::from_secret_metadata(name, labels));
            let Some(release) = release else {
                ignored += 1;
                continue;
            };
            match latest.get(&release.name) {
                Some(known) if known.revision >= release.revision => {}
                Some(_) | None => {
                    latest.insert(release.name.clone(), release);
                }
            }
        }

        // An empty token means the last page.
        let next = page.metadata.continue_.filter(|token| !token.is_empty());
        if next.is_some() && next == continue_token {
            return Err(command_error(
                &format!("Failed to list the Helm releases of `{namespace}`"),
                "the API server returned the same continue token twice",
            ));
        }
        if next.is_none() {
            break;
        }
        continue_token = next;
    }

    if ignored > 0 {
        warn!(
            namespace,
            ignored, "Ignored Secrets labelled `owner=helm` that are not Helm release revisions"
        );
    }
    Ok(latest.into_values().collect())
}

/// Latest revision of every Helm release of `namespace`, read from the Secret metadata of the cluster.
pub(super) fn installed_helm_releases(
    client: kube::Client,
    namespace: &str,
    helm_driver: Option<&str>,
) -> Result<Vec<HelmRelease>, CommandError> {
    ensure_secret_storage(helm_driver)
        .map_err(|error| command_error(&format!("Cannot list the Helm releases of `{namespace}`"), error))?;
    list_helm_releases(client, namespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{Request, Response};
    use kube::client::Body;
    use serde_json::{Value, json};
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};

    const NAMESPACE: &str = "env-ns";

    /// A revision, as the metadata-only list returns it.
    fn revision(name: &str, revision: u64, status: &str) -> Value {
        json!({
            "apiVersion": "meta.k8s.io/v1",
            "kind": "PartialObjectMetadata",
            "metadata": {
                "name": format!("sh.helm.release.v1.{name}.v{revision}"),
                "namespace": NAMESPACE,
                "labels": {"owner": "helm", "name": name, "status": status, "version": revision.to_string()},
            },
        })
    }

    fn page(items: Vec<Value>, continue_token: &str) -> (u16, Value) {
        (
            200,
            json!({
                "apiVersion": "meta.k8s.io/v1",
                "kind": "PartialObjectMetadataList",
                "metadata": {"continue": continue_token, "resourceVersion": "42"},
                "items": items,
            }),
        )
    }

    fn api_error(code: u16, reason: &str) -> (u16, Value) {
        (
            code,
            json!({"kind": "Status", "apiVersion": "v1", "status": "Failure", "message": reason,
                   "reason": reason, "code": code}),
        )
    }

    /// What the API server was asked.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedRequest {
        method: String,
        path: String,
        query: String,
        accept: String,
    }

    /// Serves `answer(continue token)` and records the requests.
    fn mock_client(
        answer: impl Fn(Option<&str>) -> (u16, Value) + Send + Sync + 'static,
    ) -> (kube::Client, Arc<Mutex<Vec<RecordedRequest>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let service = tower::service_fn(move |request: Request<Body>| {
            let query = request.uri().query().unwrap_or_default().to_string();
            let token = url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "continue")
                .map(|(_, value)| value.into_owned());
            recorded.lock().unwrap().push(RecordedRequest {
                method: request.method().to_string(),
                path: request.uri().path().to_string(),
                query,
                accept: request
                    .headers()
                    .get("accept")
                    .and_then(|accept| accept.to_str().ok())
                    .unwrap_or_default()
                    .to_string(),
            });
            let (status, body) = answer(token.as_deref());
            let response = Response::builder()
                .status(status)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();
            async move { Ok::<_, Infallible>(response) }
        });
        (block_on(async { kube::Client::new(service, "default") }), requests)
    }

    fn query_pairs(request: &RecordedRequest) -> BTreeMap<String, String> {
        url::form_urlencoded::parse(request.query.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
    }

    /// Serves `pages` in order: the first one without token, the next ones for the token of the page before.
    fn paged(pages: Vec<Vec<Value>>) -> impl Fn(Option<&str>) -> (u16, Value) + Send + Sync + 'static {
        move |token| {
            let index = token.map_or(0, |token| token.trim_start_matches("page-").parse::<usize>().unwrap());
            let next = if index + 1 < pages.len() {
                format!("page-{}", index + 1)
            } else {
                String::new()
            };
            page(pages[index].clone(), &next)
        }
    }

    fn release(name: &str, revision: u64, status: HelmReleaseStatus) -> HelmRelease {
        HelmRelease {
            name: name.to_string(),
            revision,
            status,
        }
    }

    #[test]
    fn lists_the_metadata_of_the_helm_secrets_of_the_namespace_only() {
        let (client, requests) = mock_client(|_| page(vec![revision("web", 1, "deployed")], ""));

        let releases = list_helm_releases(client, NAMESPACE).unwrap();

        assert_eq!(releases, vec![release("web", 1, HelmReleaseStatus::Deployed)]);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, format!("/api/v1/namespaces/{NAMESPACE}/secrets"));
        let query = query_pairs(request);
        assert_eq!(query.get("labelSelector").map(String::as_str), Some("owner=helm"));
        assert_eq!(query.get("limit"), Some(&PAGE_SIZE.to_string()));
        assert!(!query.contains_key("fieldSelector"));
        assert!(!query.contains_key("watch"));
        // The payload of the Secrets is never asked for: the server answers with the metadata only.
        assert!(request.accept.contains("as=PartialObjectMetadataList"), "{}", request.accept);
        assert!(request.accept.contains("g=meta.k8s.io"), "{}", request.accept);
    }

    #[test]
    fn follows_every_page_with_the_continue_token_of_the_previous_one() {
        let (client, requests) = mock_client(paged(vec![
            vec![revision("a", 1, "deployed"), revision("b", 1, "deployed")],
            vec![revision("c", 1, "failed")],
            vec![revision("d", 1, "deployed")],
        ]));

        let releases = list_helm_releases(client, NAMESPACE).unwrap();

        let names: Vec<_> = releases.iter().map(|release| release.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d"]);
        let requests = requests.lock().unwrap();
        let tokens: Vec<_> = requests
            .iter()
            .map(|request| query_pairs(request).remove("continue"))
            .collect();
        assert_eq!(tokens, [None, Some("page-1".to_string()), Some("page-2".to_string())]);
        // The selector is kept on every page.
        assert!(
            requests
                .iter()
                .all(|request| query_pairs(request)["labelSelector"] == "owner=helm")
        );
    }

    #[test]
    fn lists_more_releases_than_the_helm_default_page_of_256() {
        // 600 releases of two revisions each, over pages of 500 Secrets.
        let secrets: Vec<Value> = (0..600)
            .flat_map(|index| {
                let name = format!("release-{index:04}");
                [revision(&name, 1, "superseded"), revision(&name, 2, "deployed")]
            })
            .collect();
        let pages = secrets
            .chunks(PAGE_SIZE as usize)
            .map(<[Value]>::to_vec)
            .collect::<Vec<_>>();
        assert!(pages.len() > 1);
        let (client, requests) = mock_client(paged(pages));

        let releases = list_helm_releases(client, NAMESPACE).unwrap();

        assert_eq!(releases.len(), 600);
        assert!(
            releases
                .iter()
                .all(|release| release.revision == 2 && release.status == HelmReleaseStatus::Deployed)
        );
        assert_eq!(requests.lock().unwrap().len(), 3);
    }

    #[test]
    fn keeps_the_latest_revision_of_a_release_whatever_the_order_and_the_status() {
        let (client, _) = mock_client(paged(vec![
            vec![revision("web", 3, "failed"), revision("web", 1, "superseded")],
            // The latest revision of a release can be on another page than its older ones.
            vec![revision("web", 2, "deployed"), revision("job", 7, "pending-upgrade")],
        ]));

        let releases = list_helm_releases(client, NAMESPACE).unwrap();

        assert_eq!(
            releases,
            vec![
                release("job", 7, HelmReleaseStatus::PendingUpgrade),
                release("web", 3, HelmReleaseStatus::Failed),
            ]
        );
    }

    #[test]
    fn compares_revisions_as_numbers() {
        let (client, _) =
            mock_client(|_| page(vec![revision("web", 9, "superseded"), revision("web", 10, "deployed")], ""));

        let releases = list_helm_releases(client, NAMESPACE).unwrap();

        assert_eq!(releases, vec![release("web", 10, HelmReleaseStatus::Deployed)]);
    }

    #[test]
    fn keeps_the_releases_of_every_status_like_helm_list_all() {
        let statuses = [
            ("unknown", HelmReleaseStatus::Unknown),
            ("deployed", HelmReleaseStatus::Deployed),
            ("uninstalled", HelmReleaseStatus::Uninstalled),
            ("superseded", HelmReleaseStatus::Superseded),
            ("failed", HelmReleaseStatus::Failed),
            ("uninstalling", HelmReleaseStatus::Uninstalling),
            ("pending-install", HelmReleaseStatus::PendingInstall),
            ("pending-upgrade", HelmReleaseStatus::PendingUpgrade),
            ("pending-rollback", HelmReleaseStatus::PendingRollback),
            ("from-the-future", HelmReleaseStatus::Other("from-the-future".to_string())),
        ];
        let items: Vec<Value> = statuses
            .iter()
            .enumerate()
            .map(|(index, (status, _))| revision(&format!("release-{index}"), 1, status))
            .collect();
        let (client, _) = mock_client(move |_| page(items.clone(), ""));

        let releases = list_helm_releases(client, NAMESPACE).unwrap();

        let expected: Vec<_> = statuses
            .iter()
            .enumerate()
            .map(|(index, (_, status))| release(&format!("release-{index}"), 1, status.clone()))
            .collect();
        assert_eq!(releases, expected);
    }

    #[test]
    fn ignores_secrets_that_are_not_helm_release_revisions() {
        let mut no_name = revision("web", 1, "deployed");
        no_name["metadata"]["labels"].as_object_mut().unwrap().remove("name");
        let mut no_revision = revision("web", 1, "deployed");
        no_revision["metadata"]["labels"]
            .as_object_mut()
            .unwrap()
            .remove("version");
        let mut no_status = revision("web", 1, "deployed");
        no_status["metadata"]["labels"]
            .as_object_mut()
            .unwrap()
            .remove("status");
        let mut bad_revision = revision("web", 1, "deployed");
        bad_revision["metadata"]["labels"]["version"] = json!("latest");
        let mut negative_revision = revision("web", 1, "deployed");
        negative_revision["metadata"]["labels"]["version"] = json!("-1");
        let mut foreign_name = revision("web", 1, "deployed");
        foreign_name["metadata"]["name"] = json!("my-own-secret");
        let mut wrong_revision = revision("web", 1, "deployed");
        wrong_revision["metadata"]["name"] = json!("sh.helm.release.v1.web.v2");
        let no_labels = json!({
            "apiVersion": "meta.k8s.io/v1", "kind": "PartialObjectMetadata",
            "metadata": {"name": "sh.helm.release.v1.web.v1", "namespace": NAMESPACE},
        });
        let (client, _) = mock_client(move |_| {
            page(
                vec![
                    no_name.clone(),
                    no_revision.clone(),
                    no_status.clone(),
                    bad_revision.clone(),
                    negative_revision.clone(),
                    foreign_name.clone(),
                    wrong_revision.clone(),
                    no_labels.clone(),
                    revision("kept", 4, "deployed"),
                ],
                "",
            )
        });

        let releases = list_helm_releases(client, NAMESPACE).unwrap();

        assert_eq!(releases, vec![release("kept", 4, HelmReleaseStatus::Deployed)]);
    }

    #[test]
    fn a_release_name_with_dots_is_read_from_the_labels() {
        let (client, _) = mock_client(|_| page(vec![revision("my.release.v1", 2, "deployed")], ""));

        let releases = list_helm_releases(client, NAMESPACE).unwrap();

        assert_eq!(releases, vec![release("my.release.v1", 2, HelmReleaseStatus::Deployed)]);
    }

    #[test]
    fn an_empty_namespace_has_no_release() {
        let (client, _) = mock_client(|_| page(vec![], ""));

        assert_eq!(list_helm_releases(client, NAMESPACE).unwrap(), vec![]);
    }

    #[test]
    fn a_failure_of_a_later_page_fails_the_inventory_instead_of_returning_the_first_pages() {
        let (client, requests) = mock_client(|token| match token {
            None => page(vec![revision("a", 1, "deployed")], "page-1"),
            Some(_) => api_error(500, "InternalError"),
        });

        let error = list_helm_releases(client, NAMESPACE).unwrap_err();

        assert!(
            error
                .to_string()
                .contains(&format!("Failed to list the Helm releases of `{NAMESPACE}`")),
            "{error}"
        );
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn an_expired_continue_token_fails_the_inventory() {
        let (client, _) = mock_client(|token| match token {
            None => page(vec![revision("a", 1, "deployed")], "page-1"),
            Some(_) => api_error(410, "Expired"),
        });

        let error = list_helm_releases(client, NAMESPACE).unwrap_err();

        assert!(error.to_string().contains("Failed to list the Helm releases"), "{error}");
    }

    #[test]
    fn a_failure_of_the_first_page_fails_the_inventory() {
        let (client, _) = mock_client(|_| api_error(403, "Forbidden"));

        let error = list_helm_releases(client, NAMESPACE).unwrap_err();

        assert!(error.to_string().contains("Failed to list the Helm releases"), "{error}");
    }

    #[test]
    fn a_continue_token_that_does_not_advance_fails_instead_of_looping() {
        let (client, requests) = mock_client(|_| page(vec![revision("a", 1, "deployed")], "stuck"));

        let error = list_helm_releases(client, NAMESPACE).unwrap_err();

        assert!(error.to_string().contains("same continue token"), "{error}");
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn only_the_secret_storage_can_be_inventoried() {
        for driver in [None, Some(""), Some("secret"), Some("secrets")] {
            assert!(ensure_secret_storage(driver).is_ok(), "{driver:?}");
        }
        for driver in ["configmap", "configmaps", "memory", "sql", "Secret", "other"] {
            let error = ensure_secret_storage(Some(driver)).unwrap_err().to_string();
            assert!(
                error.contains(&format!("`{driver}`")) && error.contains("HELM_DRIVER"),
                "{error}"
            );
        }
    }

    #[test]
    fn another_storage_fails_before_any_request_to_the_cluster() {
        let (client, requests) = mock_client(|_| page(vec![], ""));

        let error = installed_helm_releases(client, NAMESPACE, Some("configmap")).unwrap_err();

        let error = error.to_string();
        assert!(
            error.contains(&format!("Cannot list the Helm releases of `{NAMESPACE}`")),
            "{error}"
        );
        assert!(error.contains("`configmap`"), "{error}");
        assert!(requests.lock().unwrap().is_empty());
    }

    #[test]
    fn the_secret_storage_lists_the_cluster() {
        let (client, requests) = mock_client(|_| page(vec![revision("web", 1, "deployed")], ""));

        let releases = installed_helm_releases(client, NAMESPACE, Some("secret")).unwrap();

        assert_eq!(releases, vec![release("web", 1, HelmReleaseStatus::Deployed)]);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }
}
