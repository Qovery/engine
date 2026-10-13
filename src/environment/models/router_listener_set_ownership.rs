//! Ownership of the hostname ListenerSet releases of the `PER_HOSTNAME_LISTENER_SET` routing mode.
//!
//! A release is named after its hostname only, so the name says nothing about the router that applies it.
//! Core refuses a hostname on two routers, the engine enforces it where breaking it would do damage:
//!
//! - Every release has a claim, a ConfigMap created atomically (the API server refuses a second create).
//!   Of two routers that deploy the same hostname at the same time, exactly one gets the claim.
//!   The claim is created before the release is applied and deleted after it is uninstalled,
//!   so a release is never applied nor uninstalled by a router that does not hold its claim.
//! - A release that exists without a claim is only adopted when its persisted Helm manifest names the router
//!   as owner. The live ListenerSets are not trusted for this, they can be deleted or have drifted.
//!   A release whose owner cannot be established is rejected.
//!
//! The releases of the previous naming scheme, `listenerset-<router id>-<hash>`, are only discovered to be removed.

use crate::environment::models::router_listener_set::{HASH_HEX_CHARS, is_hostname_release_name};
use serde::Deserialize;
use serde_yaml::{Deserializer, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Label that records the router that applied a release, on every object of the chart.
pub(crate) const SERVICE_ID_LABEL: &str = "qovery.com/service-id";

/// What the persisted Helm release says about who owns a release name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InstalledRelease {
    NotInstalled,
    Owner(String),
    /// Installed, but its manifest does not name a single router.
    Unknown,
}

/// Outcome of the atomic creation of a claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClaimCreation {
    Created,
    /// Someone else holds the claim, or the router itself did already.
    AlreadyExists,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ClaimError<E> {
    /// The release belongs to another router. It is never taken over.
    Foreign {
        owner: String,
    },
    /// The release exists but nothing establishes its owner, or the claim changed while it was being taken.
    Unverifiable {
        reason: &'static str,
    },
    Store(E),
}

/// Takes the claim of a release for `router_long_id`, or fails without having changed anything.
///
/// `read_claim` returns the owner recorded in the claim, `installed_release` reads the persisted Helm release
/// and `create_claim` creates the claim only if it does not exist yet.
/// Only the atomic creation decides between two routers: whatever they read before it, the loser of the creation
/// reads the winner back and is rejected.
pub(crate) fn claim_release<E>(
    router_long_id: &str,
    read_claim: impl Fn() -> Result<Option<String>, E>,
    installed_release: impl Fn() -> Result<InstalledRelease, E>,
    create_claim: impl Fn() -> Result<ClaimCreation, E>,
) -> Result<(), ClaimError<E>> {
    let check_owner = |owner: String| {
        if owner == router_long_id {
            Ok(())
        } else {
            Err(ClaimError::Foreign { owner })
        }
    };

    let claimed_by_router = match read_claim().map_err(ClaimError::Store)? {
        Some(owner) => {
            check_owner(owner)?;
            true
        }
        None => false,
    };
    // The claim is not enough: a release applied by a writer that ignores claims is told apart by its manifest.
    // Without a claim, an installed release is only adopted by the router its manifest names.
    match installed_release().map_err(ClaimError::Store)? {
        InstalledRelease::NotInstalled => {}
        InstalledRelease::Owner(owner) => check_owner(owner)?,
        InstalledRelease::Unknown => {
            return Err(ClaimError::Unverifiable {
                reason: "the release exists but its owner cannot be established from its Helm manifest",
            });
        }
    }
    if claimed_by_router {
        return Ok(());
    }
    match create_claim().map_err(ClaimError::Store)? {
        ClaimCreation::Created => Ok(()),
        ClaimCreation::AlreadyExists => match read_claim().map_err(ClaimError::Store)? {
            Some(owner) => check_owner(owner),
            None => Err(ClaimError::Unverifiable {
                reason: "the claim was removed while it was being taken, deploy again",
            }),
        },
    }
}

/// Router named by the `qovery.com/service-id` label of the objects of a rendered Helm manifest.
///
/// Every object of the release carries the same label. A manifest without it, or with two different values,
/// has no owner: it must not be adopted on a guess.
pub(crate) fn release_owner_from_manifest(manifest: &str) -> InstalledRelease {
    let owners: BTreeSet<String> = Deserializer::from_str(manifest)
        .filter_map(|document| Value::deserialize(document).ok())
        .filter_map(|object| {
            object
                .get("metadata")?
                .get("labels")?
                .get(SERVICE_ID_LABEL)?
                .as_str()
                .map(str::to_string)
        })
        .collect();
    let mut owners = owners.into_iter();
    match (owners.next(), owners.next()) {
        (Some(owner), None) => InstalledRelease::Owner(owner),
        _ => InstalledRelease::Unknown,
    }
}

/// A persisted claim on a release name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Claim {
    pub release: String,
    pub owner: String,
    /// Identity of the claim object, so that only this very claim is deleted.
    pub uid: String,
}

/// Claims of `router_long_id` on releases that are not wanted anymore. Claims of other routers are never listed.
pub(crate) fn stale_claims<'a>(router_long_id: &str, claims: &'a [Claim], wanted: &BTreeSet<&str>) -> Vec<&'a Claim> {
    claims
        .iter()
        .filter(|claim| claim.owner == router_long_id && !wanted.contains(claim.release.as_str()))
        .collect()
}

/// What `remove_if_owned` found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Removal {
    Uninstalled,
    NotInstalled,
    /// The installed release names another router, it was left alone.
    ForeignRelease(String),
    /// The installed release names nobody, it was left alone.
    UnknownRelease,
}

/// Uninstalls a release only when its persisted Helm manifest names `router_long_id`.
///
/// The decision is taken on the manifest read right now, never on a list read earlier: a release that
/// drifted to another router, or that nobody can vouch for, is not uninstalled.
pub(crate) fn remove_if_owned<E>(
    router_long_id: &str,
    installed_release: impl Fn() -> Result<InstalledRelease, E>,
    uninstall: impl Fn() -> Result<(), E>,
) -> Result<Removal, E> {
    match installed_release()? {
        InstalledRelease::NotInstalled => Ok(Removal::NotInstalled),
        InstalledRelease::Owner(owner) if owner == router_long_id => uninstall().map(|()| Removal::Uninstalled),
        InstalledRelease::Owner(owner) => Ok(Removal::ForeignRelease(owner)),
        InstalledRelease::Unknown => Ok(Removal::UnknownRelease),
    }
}

/// Removes the release of a claim the router does not want anymore, then the claim itself.
///
/// The claim goes last: until the release is uninstalled no other router can claim it, and a failure keeps the claim
/// so that the next deployment retries. A claim whose release names another router is dropped without touching
/// that release, the router does not serve the hostname anymore. A release that nobody vouches for keeps its claim.
pub(crate) fn release_stale_claim<E>(
    router_long_id: &str,
    claim: &Claim,
    installed_release: impl Fn() -> Result<InstalledRelease, E>,
    uninstall: impl Fn() -> Result<(), E>,
    delete_claim: impl Fn() -> Result<(), E>,
) -> Result<(), ClaimError<E>> {
    if claim.owner != router_long_id {
        return Err(ClaimError::Foreign {
            owner: claim.owner.clone(),
        });
    }
    match remove_if_owned(router_long_id, installed_release, uninstall).map_err(ClaimError::Store)? {
        Removal::Uninstalled | Removal::NotInstalled | Removal::ForeignRelease(_) => {
            delete_claim().map_err(ClaimError::Store)
        }
        Removal::UnknownRelease => Err(ClaimError::Unverifiable {
            reason: "the release exists but its owner cannot be established from its Helm manifest",
        }),
    }
}

/// Installed releases shaped like a hostname release that have no claim and are not wanted.
///
/// They were applied before claims existed. Their owner is checked on the manifest by `remove_if_owned`.
pub(crate) fn unclaimed_hostname_releases<'a>(
    installed_releases: impl IntoIterator<Item = &'a str>,
    claims: &[Claim],
    wanted: &BTreeSet<&str>,
) -> Vec<&'a str> {
    installed_releases
        .into_iter()
        .filter(|name| {
            is_hostname_release_name(name)
                && !wanted.contains(name)
                && !claims.iter().any(|claim| claim.release == *name)
        })
        .collect()
}

/// Prefix of the releases named by the previous scheme, `listenerset-<router id>-<hash>`.
fn legacy_release_prefix(router_id: &str) -> String {
    format!("listenerset-{router_id}-")
}

/// Installed releases of the previous naming and scope scheme that belong to the router.
///
/// A release belongs to the router when its name is `listenerset-<router id>-<16 hex>`, or when its live
/// ListenerSets carry the router as owner under the previous `router-hostname` scope (`legacy_scope_owners`,
/// by release name). Releases of other routers, and releases named after a hostname only, never match.
/// None of them is wanted: the new scheme has its own releases.
pub(crate) fn stale_legacy_releases<'a>(
    router_id: &str,
    router_long_id: &str,
    legacy_scope_owners: &BTreeMap<String, String>,
    installed_releases: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let prefix = legacy_release_prefix(router_id);
    installed_releases
        .into_iter()
        .filter(|name| {
            let named_after_router = name
                .strip_prefix(&prefix)
                .is_some_and(|hash| hash.len() == HASH_HEX_CHARS && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
            let labelled_with_router = legacy_scope_owners
                .get(*name)
                .is_some_and(|owner| owner == router_long_id);
            named_after_router || labelled_with_router
        })
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        Claim, ClaimCreation, ClaimError, InstalledRelease, Removal, claim_release, release_owner_from_manifest,
        release_stale_claim, remove_if_owned, stale_claims, stale_legacy_releases, unclaimed_hostname_releases,
    };
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};

    type TestError = &'static str;
    type Outcome = Result<(), ClaimError<TestError>>;

    /// In-memory claim and release stores, with the atomic create of the API server.
    #[derive(Default)]
    struct Cluster {
        claims: RefCell<BTreeMap<String, String>>,
        installed: RefCell<BTreeMap<String, InstalledRelease>>,
    }

    impl Cluster {
        fn claim(&self, release: &str, router: &str) -> Outcome {
            self.claim_with_hook(release, router, || {})
        }

        /// `before_create` runs after the router has read the cluster and before it creates the claim,
        /// which is where a concurrent router can slip in.
        fn claim_with_hook(&self, release: &str, router: &str, before_create: impl Fn()) -> Outcome {
            claim_release(
                router,
                || Ok(self.claims.borrow().get(release).cloned()),
                || {
                    Ok(self
                        .installed
                        .borrow()
                        .get(release)
                        .cloned()
                        .unwrap_or(InstalledRelease::NotInstalled))
                },
                || {
                    before_create();
                    let mut claims = self.claims.borrow_mut();
                    if claims.contains_key(release) {
                        return Ok(ClaimCreation::AlreadyExists);
                    }
                    claims.insert(release.to_string(), router.to_string());
                    Ok(ClaimCreation::Created)
                },
            )
        }

        fn claim_owner(&self, release: &str) -> Option<String> {
            self.claims.borrow().get(release).cloned()
        }
    }

    const RELEASE: &str = "listenerset-0123456789abcdef";

    fn foreign(owner: &str) -> Outcome {
        Err(ClaimError::Foreign {
            owner: owner.to_string(),
        })
    }

    #[test]
    fn a_new_release_is_claimed_and_claiming_again_is_idempotent() {
        let cluster = Cluster::default();

        assert_eq!(cluster.claim(RELEASE, "router-a"), Ok(()));
        assert_eq!(cluster.claim(RELEASE, "router-a"), Ok(()));
        assert_eq!(cluster.claim_owner(RELEASE).as_deref(), Some("router-a"));
    }

    #[test]
    fn a_claim_held_by_another_router_is_rejected_and_left_untouched() {
        let cluster = Cluster::default();
        cluster.claim(RELEASE, "router-a").unwrap();

        assert_eq!(cluster.claim(RELEASE, "router-b"), foreign("router-a"));
        assert_eq!(cluster.claim_owner(RELEASE).as_deref(), Some("router-a"));
    }

    #[test]
    fn two_routers_that_both_saw_a_free_release_cannot_both_win() {
        let cluster = Cluster::default();

        // Router B read the cluster while the release was free, router A claims before B creates its claim.
        let result = cluster.claim_with_hook(RELEASE, "router-b", || {
            cluster.claim(RELEASE, "router-a").unwrap();
        });

        assert_eq!(result, foreign("router-a"));
        assert_eq!(cluster.claim_owner(RELEASE).as_deref(), Some("router-a"));
    }

    #[test]
    fn a_router_that_loses_the_creation_to_itself_is_not_rejected() {
        let cluster = Cluster::default();

        let result = cluster.claim_with_hook(RELEASE, "router-a", || {
            cluster.claim(RELEASE, "router-a").unwrap();
        });

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn a_claim_that_disappears_during_the_creation_is_not_assumed_to_be_free() {
        let result: Outcome = claim_release(
            "router-a",
            || Ok(None),
            || Ok(InstalledRelease::NotInstalled),
            || Ok(ClaimCreation::AlreadyExists),
        );

        assert!(matches!(result, Err(ClaimError::Unverifiable { .. })), "{result:?}");
    }

    #[test]
    fn an_installed_release_owned_by_another_router_without_claim_or_listener_sets_is_rejected() {
        let cluster = Cluster::default();
        cluster
            .installed
            .borrow_mut()
            .insert(RELEASE.to_string(), InstalledRelease::Owner("router-a".to_string()));

        assert_eq!(cluster.claim(RELEASE, "router-b"), foreign("router-a"));
        assert_eq!(cluster.claim_owner(RELEASE), None, "nothing is claimed on a rejection");
    }

    #[test]
    fn an_installed_release_without_established_owner_is_rejected() {
        let cluster = Cluster::default();
        cluster
            .installed
            .borrow_mut()
            .insert(RELEASE.to_string(), InstalledRelease::Unknown);

        let result = cluster.claim(RELEASE, "router-a");

        assert!(matches!(result, Err(ClaimError::Unverifiable { .. })), "{result:?}");
        assert_eq!(cluster.claim_owner(RELEASE), None);
    }

    #[test]
    fn an_installed_release_owned_by_the_router_without_claim_is_adopted() {
        let cluster = Cluster::default();
        cluster
            .installed
            .borrow_mut()
            .insert(RELEASE.to_string(), InstalledRelease::Owner("router-a".to_string()));

        assert_eq!(cluster.claim(RELEASE, "router-a"), Ok(()));
        assert_eq!(cluster.claim_owner(RELEASE).as_deref(), Some("router-a"));
    }

    #[test]
    fn a_claim_of_the_router_does_not_vouch_for_a_release_that_names_another_router() {
        let cluster = Cluster::default();
        cluster.claim(RELEASE, "router-a").unwrap();
        // Applied by a writer that ignores claims.
        cluster
            .installed
            .borrow_mut()
            .insert(RELEASE.to_string(), InstalledRelease::Owner("router-b".to_string()));

        assert_eq!(cluster.claim(RELEASE, "router-a"), foreign("router-b"));
    }

    #[test]
    fn a_claim_of_the_router_does_not_vouch_for_a_release_without_owner() {
        let cluster = Cluster::default();
        cluster.claim(RELEASE, "router-a").unwrap();
        cluster
            .installed
            .borrow_mut()
            .insert(RELEASE.to_string(), InstalledRelease::Unknown);

        let result = cluster.claim(RELEASE, "router-a");

        assert!(matches!(result, Err(ClaimError::Unverifiable { .. })), "{result:?}");
    }

    #[test]
    fn a_claim_of_the_router_on_a_release_it_applied_is_kept() {
        let cluster = Cluster::default();
        cluster.claim(RELEASE, "router-a").unwrap();
        cluster
            .installed
            .borrow_mut()
            .insert(RELEASE.to_string(), InstalledRelease::Owner("router-a".to_string()));

        assert_eq!(cluster.claim(RELEASE, "router-a"), Ok(()));
    }

    #[test]
    fn a_store_error_is_not_a_free_release() {
        let result: Outcome = claim_release(
            "router-a",
            || Err("api down"),
            || Ok(InstalledRelease::NotInstalled),
            || Ok(ClaimCreation::Created),
        );

        assert!(matches!(result, Err(ClaimError::Store(_))), "{result:?}");
    }

    const MANIFEST: &str = r#"
---
# Source: q-hostname-listenerset/templates/certificate.yaml
apiVersion: cert-manager.io/v1
kind: Certificate
metadata:
  name: "listenerset-0123456789abcdef-tls"
  labels:
    qovery.com/service-id: "router-a"
    qovery.com/listenerset-scope: "hostname"
---
apiVersion: gateway.networking.k8s.io/v1
kind: ListenerSet
metadata:
  name: "listenerset-0123456789abcdef"
  labels:
    qovery.com/service-id: "router-a"
"#;

    #[test]
    fn the_owner_is_read_from_the_persisted_manifest_even_without_live_objects() {
        assert_eq!(
            release_owner_from_manifest(MANIFEST),
            InstalledRelease::Owner("router-a".to_string())
        );
    }

    #[test]
    fn a_manifest_without_a_single_owner_has_no_owner() {
        let without_label = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: x\n";
        let two_owners = MANIFEST.replacen("router-a", "router-b", 1);

        assert_eq!(release_owner_from_manifest(""), InstalledRelease::Unknown);
        assert_eq!(release_owner_from_manifest(without_label), InstalledRelease::Unknown);
        assert_eq!(release_owner_from_manifest(&two_owners), InstalledRelease::Unknown);
    }

    fn claim(release: &str, owner: &str) -> Claim {
        Claim {
            release: release.to_string(),
            owner: owner.to_string(),
            uid: format!("uid-{release}"),
        }
    }

    #[test]
    fn only_the_unwanted_claims_of_the_router_are_stale() {
        let claims = [
            claim("listenerset-aaaaaaaaaaaaaaaa", "router-a"),
            claim("listenerset-bbbbbbbbbbbbbbbb", "router-a"),
            claim("listenerset-cccccccccccccccc", "router-b"),
        ];

        let stale = |router: &str, wanted: &[&str]| -> Vec<String> {
            stale_claims(router, &claims, &wanted.iter().copied().collect::<BTreeSet<_>>())
                .into_iter()
                .map(|claim| claim.release.clone())
                .collect()
        };

        assert_eq!(
            stale("router-a", &[]),
            vec!["listenerset-aaaaaaaaaaaaaaaa", "listenerset-bbbbbbbbbbbbbbbb"]
        );
        assert_eq!(
            stale("router-a", &["listenerset-aaaaaaaaaaaaaaaa"]),
            vec!["listenerset-bbbbbbbbbbbbbbbb"]
        );
        // Deleting router B leaves the claims of router A alone, and the other way round.
        assert_eq!(stale("router-b", &[]), vec!["listenerset-cccccccccccccccc"]);
    }

    #[test]
    fn legacy_releases_of_the_router_are_found_by_name_and_by_the_previous_scope_label() {
        let named = "listenerset-zrouter01-0123456789abcdef";
        let labelled_only = "renamed-release";
        let other_router_by_name = "listenerset-zrouter02-0123456789abcdef";
        let similar_router_by_name = "listenerset-zrouter011-0123456789abcdef";
        let other_router_by_label = "other-release";
        let current_scheme = "listenerset-0123456789abcdef";
        let not_a_hash = "listenerset-zrouter01-not-a-hash";
        let scope_owners = BTreeMap::from([
            (labelled_only.to_string(), "router-a".to_string()),
            (other_router_by_label.to_string(), "router-b".to_string()),
        ]);
        let installed = [
            named,
            labelled_only,
            other_router_by_name,
            similar_router_by_name,
            other_router_by_label,
            current_scheme,
            not_a_hash,
            "router-zrouter01",
        ];

        assert_eq!(
            stale_legacy_releases("zrouter01", "router-a", &scope_owners, installed),
            vec![named.to_string(), labelled_only.to_string()]
        );
    }

    #[test]
    fn a_legacy_release_that_is_not_installed_is_not_listed() {
        let scope_owners = BTreeMap::from([("ghost".to_string(), "router-a".to_string())]);

        assert!(stale_legacy_releases("zrouter01", "router-a", &scope_owners, ["other"]).is_empty());
    }

    /// Records what the cleanup does, in order.
    struct Recorder {
        calls: RefCell<Vec<&'static str>>,
    }

    impl Recorder {
        fn new() -> Self {
            Self {
                calls: RefCell::new(vec![]),
            }
        }

        fn release_stale_claim(
            &self,
            router: &str,
            claim: &Claim,
            installed: InstalledRelease,
            uninstall_fails: bool,
        ) -> Outcome {
            release_stale_claim(
                router,
                claim,
                || {
                    self.calls.borrow_mut().push("read_manifest");
                    Ok(installed.clone())
                },
                || {
                    self.calls.borrow_mut().push("uninstall");
                    if uninstall_fails { Err("helm failed") } else { Ok(()) }
                },
                || {
                    self.calls.borrow_mut().push("delete_claim");
                    Ok(())
                },
            )
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.borrow().clone()
        }
    }

    #[test]
    fn the_release_is_uninstalled_before_its_claim_is_deleted() {
        let recorder = Recorder::new();
        let stale = claim(RELEASE, "router-a");

        let result =
            recorder.release_stale_claim("router-a", &stale, InstalledRelease::Owner("router-a".into()), false);

        assert_eq!(result, Ok(()));
        assert_eq!(recorder.calls(), vec!["read_manifest", "uninstall", "delete_claim"]);
    }

    #[test]
    fn a_failed_uninstall_keeps_the_claim() {
        let recorder = Recorder::new();
        let stale = claim(RELEASE, "router-a");

        let result = recorder.release_stale_claim("router-a", &stale, InstalledRelease::Owner("router-a".into()), true);

        assert_eq!(result, Err(ClaimError::Store("helm failed")));
        assert_eq!(recorder.calls(), vec!["read_manifest", "uninstall"]);
    }

    #[test]
    fn a_claim_without_release_is_deleted_on_its_own() {
        let recorder = Recorder::new();
        let stale = claim(RELEASE, "router-a");

        let result = recorder.release_stale_claim("router-a", &stale, InstalledRelease::NotInstalled, false);

        assert_eq!(result, Ok(()));
        assert_eq!(recorder.calls(), vec!["read_manifest", "delete_claim"]);
    }

    #[test]
    fn a_release_of_another_router_is_never_uninstalled_and_the_stale_claim_is_dropped() {
        let recorder = Recorder::new();
        let stale = claim(RELEASE, "router-a");

        let result =
            recorder.release_stale_claim("router-a", &stale, InstalledRelease::Owner("router-b".into()), false);

        assert_eq!(result, Ok(()));
        assert_eq!(recorder.calls(), vec!["read_manifest", "delete_claim"]);
    }

    #[test]
    fn a_release_without_owner_is_never_uninstalled_and_keeps_its_claim() {
        let recorder = Recorder::new();
        let stale = claim(RELEASE, "router-a");

        let result = recorder.release_stale_claim("router-a", &stale, InstalledRelease::Unknown, false);

        assert!(matches!(result, Err(ClaimError::Unverifiable { .. })), "{result:?}");
        assert_eq!(recorder.calls(), vec!["read_manifest"]);
    }

    #[test]
    fn the_claim_of_another_router_is_never_touched() {
        let recorder = Recorder::new();
        let other = claim(RELEASE, "router-b");

        let result =
            recorder.release_stale_claim("router-a", &other, InstalledRelease::Owner("router-b".into()), false);

        assert_eq!(result, foreign("router-b"));
        assert!(recorder.calls().is_empty(), "{:?}", recorder.calls());
    }

    #[test]
    fn a_release_without_claim_is_only_uninstalled_when_its_manifest_names_the_router() {
        let removal = |installed: InstalledRelease| {
            let recorder = Recorder::new();
            let removal: Result<Removal, TestError> = remove_if_owned(
                "router-a",
                || Ok(installed.clone()),
                || {
                    recorder.calls.borrow_mut().push("uninstall");
                    Ok(())
                },
            );
            (removal, recorder.calls())
        };

        assert_eq!(
            removal(InstalledRelease::Owner("router-a".into())),
            (Ok(Removal::Uninstalled), vec!["uninstall"])
        );
        assert_eq!(
            removal(InstalledRelease::Owner("router-b".into())),
            (Ok(Removal::ForeignRelease("router-b".into())), vec![])
        );
        assert_eq!(removal(InstalledRelease::Unknown), (Ok(Removal::UnknownRelease), vec![]));
        assert_eq!(removal(InstalledRelease::NotInstalled), (Ok(Removal::NotInstalled), vec![]));
    }

    #[test]
    fn unclaimed_hostname_releases_exclude_claimed_wanted_and_foreign_shaped_names() {
        let claimed = "listenerset-aaaaaaaaaaaaaaaa";
        let wanted_release = "listenerset-bbbbbbbbbbbbbbbb";
        let orphan = "listenerset-cccccccccccccccc";
        let claims = [claim(claimed, "router-b")];
        let installed = [
            claimed,
            wanted_release,
            orphan,
            "listenerset-zrouter01-dddddddddddddddd",
            "other-chart",
        ];

        assert_eq!(
            unclaimed_hostname_releases(installed, &claims, &BTreeSet::from([wanted_release])),
            vec![orphan]
        );
    }
}
