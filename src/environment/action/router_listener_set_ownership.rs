//! Cluster side of the ownership of the hostname ListenerSet releases, see `models::router_listener_set_ownership`.
//!
//! The claim of a release is a ConfigMap of the environment namespace named `<release>-owner`.
//! Its creation is the only atomic step: the API server refuses a second create, whoever read what before.

use crate::cmd::command::CommandKiller;
use crate::cmd::helm::HelmError;
use crate::cmd::kubectl::kubectl_get_listenerset_served_version;
use crate::environment::models::router_listener_set_ownership::{
    Claim, ClaimCreation, ClaimError, InstalledRelease, SERVICE_ID_LABEL, claim_release, release_owner_from_manifest,
};
use crate::errors::CommandError;
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::api::{Api, ApiResource, DeleteParams, ListParams, ObjectMeta, PostParams, Preconditions};
use kube::core::{DynamicObject, GroupVersionKind};
use std::collections::BTreeMap;

const SCOPE_LABEL: &str = "qovery.com/listenerset-scope";
/// Scope of the claim ConfigMaps.
const CLAIM_SCOPE: &str = "hostname-claim";
/// Scope written by the previous naming scheme, `listenerset-<router id>-<hash>`.
const LEGACY_RELEASE_SCOPE: &str = "router-hostname";
const HELM_RELEASE_NAME_ANNOTATION: &str = "meta.helm.sh/release-name";
const HELM_RELEASE_NAMESPACE_ANNOTATION: &str = "meta.helm.sh/release-namespace";
const OWNER_KEY: &str = "owner";
const RELEASE_KEY: &str = "release";
const HOSTNAME_KEY: &str = "hostname";

fn claim_name(release: &str) -> String {
    format!("{release}-owner")
}

fn claims_api(target: &DeploymentTarget) -> Api<ConfigMap> {
    Api::namespaced(target.kube.client(), target.environment.namespace())
}

fn command_error(context: &str, error: impl std::fmt::Display) -> CommandError {
    CommandError::new_from_safe_message(format!("{context}: {error}"))
}

fn read_claim_owner(api: &Api<ConfigMap>, release: &str) -> Result<Option<String>, CommandError> {
    let claim = block_on(api.get_opt(&claim_name(release)))
        .map_err(|error| command_error(&format!("Failed to read the claim of release `{release}`"), error))?;
    // A claim without owner is not a claim of nobody: it cannot be told apart from a tampered one.
    Ok(claim.map(|claim| {
        claim
            .data
            .and_then(|mut data| data.remove(OWNER_KEY))
            .unwrap_or_default()
    }))
}

pub(super) fn installed_release(target: &DeploymentTarget, release: &str) -> Result<InstalledRelease, CommandError> {
    let chart = ChartInfo {
        name: release.to_string(),
        namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
        ..Default::default()
    };
    match target.helm.get_release_manifest(&chart, &[], &CommandKiller::never()) {
        Ok(manifest) => Ok(release_owner_from_manifest(&manifest)),
        Err(HelmError::ReleaseDoesNotExist(_)) => Ok(InstalledRelease::NotInstalled),
        Err(error) => Err(command_error(
            &format!("Failed to read the Helm manifest of release `{release}`"),
            error,
        )),
    }
}

fn create_claim(
    target: &DeploymentTarget,
    api: &Api<ConfigMap>,
    release: &str,
    hostname: &str,
    owner: &str,
) -> Result<ClaimCreation, CommandError> {
    let environment = target.environment;
    let labels = BTreeMap::from([
        (SCOPE_LABEL.to_string(), CLAIM_SCOPE.to_string()),
        (SERVICE_ID_LABEL.to_string(), owner.to_string()),
        ("qovery.com/environment-id".to_string(), environment.long_id.to_string()),
        ("qovery.com/project-id".to_string(), environment.project_long_id.to_string()),
    ]);
    let claim = ConfigMap {
        metadata: ObjectMeta {
            name: Some(claim_name(release)),
            namespace: Some(environment.namespace().to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(BTreeMap::from([
            (OWNER_KEY.to_string(), owner.to_string()),
            (RELEASE_KEY.to_string(), release.to_string()),
            (HOSTNAME_KEY.to_string(), hostname.to_string()),
        ])),
        ..Default::default()
    };
    match block_on(api.create(&PostParams::default(), &claim)) {
        Ok(_) => Ok(ClaimCreation::Created),
        Err(kube::Error::Api(status)) if status.code == 409 => Ok(ClaimCreation::AlreadyExists),
        Err(error) => Err(command_error(
            &format!("Failed to create the claim of release `{release}`"),
            error,
        )),
    }
}

/// Takes the claim of `release` for the router, or fails without taking over a release of another router.
/// Must succeed before the release is applied.
pub(super) fn claim_hostname_release(
    target: &DeploymentTarget,
    router_long_id: &str,
    release: &str,
    hostname: &str,
) -> Result<(), ClaimError<CommandError>> {
    let api = claims_api(target);
    claim_release(
        router_long_id,
        || read_claim_owner(&api, release),
        || installed_release(target, release),
        || create_claim(target, &api, release, hostname, router_long_id),
    )
}

/// Every claim of the environment namespace, whoever holds it.
pub(super) fn list_claims(target: &DeploymentTarget) -> Result<Vec<Claim>, CommandError> {
    let selector = format!("{SCOPE_LABEL}={CLAIM_SCOPE}");
    let claims = block_on(claims_api(target).list(&ListParams::default().labels(&selector)))
        .map_err(|error| command_error("Failed to list the claims of the ListenerSet releases", error))?;
    Ok(claims
        .items
        .into_iter()
        .filter_map(|claim| {
            let uid = claim.metadata.uid?;
            let mut data = claim.data?;
            Some(Claim {
                release: data.remove(RELEASE_KEY)?,
                owner: data.remove(OWNER_KEY)?,
                uid,
            })
        })
        .collect())
}

/// Deletes the claim, only if it is still the very object that was listed.
/// Must happen after the release is uninstalled, so that nobody can claim a release that is still applied.
pub(super) fn delete_claim(target: &DeploymentTarget, claim: &Claim) -> Result<(), CommandError> {
    let mut params = DeleteParams::background();
    params.preconditions = Some(Preconditions {
        uid: Some(claim.uid.clone()),
        resource_version: None,
    });
    match block_on(claims_api(target).delete(&claim_name(&claim.release), &params)) {
        Ok(_) => Ok(()),
        // Already gone, or replaced by another claim that is not ours to delete.
        Err(kube::Error::Api(status)) if status.code == 404 || status.code == 409 => Ok(()),
        Err(error) => Err(command_error(
            &format!("Failed to delete the claim of release `{}`", claim.release),
            error,
        )),
    }
}

/// Owner of each release of the previous naming scheme, read from its live ListenerSets
/// (`router-hostname` scope), by release name. Only the releases of the environment namespace are read.
pub(super) fn list_legacy_release_owners(target: &DeploymentTarget) -> Result<BTreeMap<String, String>, CommandError> {
    let kube_client = target.kube.client();
    // Without the CRD no ListenerSet exists, so no release has an owner.
    let Some(version) = kubectl_get_listenerset_served_version(&kube_client) else {
        return Ok(BTreeMap::new());
    };
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", &version, "ListenerSet");
    let api: Api<DynamicObject> = Api::all_with(kube_client, &ApiResource::from_gvk(&gvk));
    let selector = format!("{SCOPE_LABEL}={LEGACY_RELEASE_SCOPE}");
    let listener_sets = block_on(api.list(&ListParams::default().labels(&selector)))
        .map_err(|error| command_error("Failed to list the ListenerSets of the previous hostname releases", error))?;

    let release_namespace = target.environment.namespace();
    Ok(listener_sets
        .items
        .into_iter()
        .filter_map(|listener_set| {
            let annotations = listener_set.metadata.annotations.as_ref()?;
            if annotations.get(HELM_RELEASE_NAMESPACE_ANNOTATION)?.as_str() != release_namespace {
                return None;
            }
            let release = annotations.get(HELM_RELEASE_NAME_ANNOTATION)?.clone();
            let owner = listener_set.metadata.labels.as_ref()?.get(SERVICE_ID_LABEL)?.clone();
            Some((release, owner))
        })
        .collect())
}
