//! Exact `engine_version` a catalog RDS module must declare, read from the live instance.
//!
//! RDS resolves a bare major (`"18"`) to its regional default minor, which an instance on a newer minor
//! can neither keep nor upgrade to (`Cannot upgrade postgres from 14.24 to 18.3`), and the terraform output
//! lags the instance after an auto-minor upgrade. So the engine pins the full version before every plan.

use crate::environment::action::deploy_database::{get_rds_engine_versions, get_rds_valid_upgrade_targets};
use crate::environment::action::deploy_terraform_service::{drivable_instance, with_region};
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::infrastructure::models::cloud_provider::service::DatabaseType;
use crate::io_models::terraform::PausableInstance;
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EngineVersions {
    pub status: String,
    pub current: String,
    /// Version RDS will move to at the next maintenance window, if a change is pending.
    pub pending: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unresolvable {
    UnreadableVersion(String),
    /// The AWS answer when the upgrade targets cannot be listed.
    TargetsUnreadable(String),
    NoUpgradeTarget {
        running: String,
        catalog_major: u32,
        offered: Vec<String>,
    },
}

impl Unresolvable {
    pub fn describe(&self) -> String {
        match self {
            Unresolvable::UnreadableVersion(version) => format!("cannot read engine version `{version}`"),
            Unresolvable::TargetsUnreadable(answer) => format!("cannot list its upgrade targets: {answer}"),
            Unresolvable::NoUpgradeTarget {
                running,
                catalog_major,
                offered,
            } => format!(
                "RDS offers no {catalog_major}.x upgrade from {running} (offered: {})",
                if offered.is_empty() {
                    "none".to_string()
                } else {
                    offered.join(", ")
                }
            ),
        }
    }
}

pub(super) fn major_of(version: &str) -> Option<u32> {
    version.split('.').next()?.trim().parse().ok()
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let parts = |version: &str| -> Vec<u64> { version.split('.').map(|part| part.parse().unwrap_or(0)).collect() };
    parts(left).cmp(&parts(right))
}

/// `valid_targets` is only read when the instance runs an older major than the catalog's.
pub(crate) fn resolve_engine_version(
    running: &str,
    pending: Option<&str>,
    catalog_major: u32,
    valid_targets: &dyn Fn() -> Result<Vec<String>, String>,
) -> Result<String, Unresolvable> {
    // The provider keeps its state while a change is pending; pinning anything else would request another one.
    if let Some(pending) = pending.filter(|it| major_of(it) == Some(catalog_major)) {
        return Ok(pending.to_string());
    }
    let Some(running_major) = major_of(running) else {
        return Err(Unresolvable::UnreadableVersion(running.to_string()));
    };
    match running_major.cmp(&catalog_major) {
        // Newer than the catalog (upgraded outside Qovery, not relabelled yet): kept, RDS cannot downgrade.
        Ordering::Equal | Ordering::Greater => Ok(running.to_string()),
        Ordering::Less => {
            let offered = valid_targets().map_err(Unresolvable::TargetsUnreadable)?;
            offered
                .iter()
                .filter(|target| major_of(target) == Some(catalog_major))
                .max_by(|left, right| compare_versions(left, right))
                .cloned()
                .ok_or(Unresolvable::NoUpgradeTarget {
                    running: running.to_string(),
                    catalog_major,
                    offered,
                })
        }
    }
}

/// Pin for a deploy or a preview: `Ok(None)` when there is nothing to pin, the reason RDS gave otherwise.
pub(crate) fn pinned_engine_version_or_error(
    instance: &PausableInstance,
    cloud_credentials: &[(&str, &str)],
    event_details: &EventDetails,
    log: &mut dyn FnMut(String),
) -> Result<Option<String>, Box<EngineError>> {
    match pin_engine_version(instance, cloud_credentials) {
        None => Ok(None),
        Some(Ok(pinned)) => {
            log(format!(
                "Pinning database `{}` engine_version to {} (running {})",
                pinned.db_id, pinned.version, pinned.running
            ));
            if instance
                .catalog_engine_major
                .is_some_and(|catalog| major_of(&pinned.running).is_some_and(|running| running > catalog))
            {
                log(format!(
                    "Database `{}` runs {}, a newer major than its blueprint: move the blueprint to that major",
                    pinned.db_id, pinned.running
                ));
            }
            Ok(Some(pinned.version))
        }
        Some(Err(reason)) => Err(Box::new(EngineError::new_cannot_resolve_managed_database_version(
            event_details.clone(),
            reason,
        ))),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PinnedEngineVersion {
    pub db_id: String,
    pub running: String,
    pub version: String,
}

/// `None` when there is nothing to pin: no catalog major, or an instance that is not an RDS PostgreSQL or MySQL.
/// `cloud_credentials` are the cluster's AWS ones; the region is switched to the instance's.
fn pin_engine_version(
    instance: &PausableInstance,
    cloud_credentials: &[(&str, &str)],
) -> Option<Result<PinnedEngineVersion, String>> {
    let catalog_major = instance.catalog_engine_major?;
    let adopted = drivable_instance(instance)?;
    let engine = match adopted.db_type {
        DatabaseType::PostgreSQL => "postgres",
        DatabaseType::MySQL => "mysql",
        _ => return None,
    };
    let credentials = with_region(cloud_credentials.to_vec(), &adopted.region);
    let aws_answer = |(error, output): (crate::cmd::command::CommandError, String)| {
        if output.is_empty() { error.to_string() } else { output }
    };

    Some(
        (|| {
            let versions = get_rds_engine_versions(&adopted.id, &credentials).map_err(aws_answer)?;
            let targets = || get_rds_valid_upgrade_targets(engine, &versions.current, &credentials).map_err(aws_answer);
            let version =
                resolve_engine_version(&versions.current, versions.pending.as_deref(), catalog_major, &targets)
                    .map_err(|it| it.describe())?;
            Ok(PinnedEngineVersion {
                db_id: adopted.id.clone(),
                running: versions.current.clone(),
                version,
            })
        })()
        .map_err(|reason: String| format!("Cannot pin the engine version of database `{}`: {reason}", adopted.id)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(versions: &[&str]) -> impl Fn() -> Result<Vec<String>, String> {
        let versions: Vec<String> = versions.iter().map(|it| it.to_string()).collect();
        move || Ok(versions.clone())
    }

    fn unreachable_targets() -> Result<Vec<String>, String> {
        panic!("upgrade targets must not be read when no major upgrade is needed")
    }

    #[test]
    fn keeps_the_running_version_on_the_catalog_major() {
        assert_eq!(
            resolve_engine_version("14.24", None, 14, &unreachable_targets),
            Ok("14.24".to_string())
        );
        assert_eq!(
            resolve_engine_version("8.0.39", None, 8, &unreachable_targets),
            Ok("8.0.39".to_string())
        );
    }

    #[test]
    fn keeps_a_pending_change_to_the_catalog_major() {
        assert_eq!(
            resolve_engine_version("14.22", Some("18.6"), 18, &unreachable_targets),
            Ok("18.6".to_string())
        );
    }

    #[test]
    fn ignores_a_pending_change_to_another_major() {
        let offered = targets(&["18.3", "18.6"]);
        assert_eq!(
            resolve_engine_version("14.22", Some("14.23"), 18, &offered),
            Ok("18.6".to_string())
        );
    }

    #[test]
    fn upgrades_to_the_only_target_a_late_minor_can_reach() {
        let offered = targets(&["15.19", "16.15", "17.11", "18.6"]);
        assert_eq!(resolve_engine_version("14.24", None, 18, &offered), Ok("18.6".to_string()));
    }

    #[test]
    fn upgrades_to_the_highest_target_of_the_catalog_major() {
        let offered = targets(&["17.11", "18.3", "18.4", "18.6", "18.10"]);
        assert_eq!(resolve_engine_version("14.22", None, 18, &offered), Ok("18.10".to_string()));
    }

    #[test]
    fn keeps_a_version_upgraded_past_the_catalog_major() {
        assert_eq!(
            resolve_engine_version("18.6", None, 14, &unreachable_targets),
            Ok("18.6".to_string())
        );
    }

    #[test]
    fn names_the_targets_rds_offers_when_none_is_on_the_catalog_major() {
        let offered = targets(&["15.19", "16.15"]);
        let resolved = resolve_engine_version("14.24", None, 18, &offered);
        assert_eq!(
            resolved,
            Err(Unresolvable::NoUpgradeTarget {
                running: "14.24".to_string(),
                catalog_major: 18,
                offered: vec!["15.19".to_string(), "16.15".to_string()],
            })
        );
        assert_eq!(
            resolved.err().map(|it| it.describe()).as_deref(),
            Some("RDS offers no 18.x upgrade from 14.24 (offered: 15.19, 16.15)")
        );
    }

    #[test]
    fn fails_on_an_unreadable_running_version() {
        assert_eq!(
            resolve_engine_version("", None, 18, &unreachable_targets),
            Err(Unresolvable::UnreadableVersion(String::new()))
        );
    }

    #[test]
    fn surfaces_a_failed_target_lookup() {
        let failing = || Err("AccessDenied".to_string());
        assert_eq!(
            resolve_engine_version("14.24", None, 18, &failing),
            Err(Unresolvable::TargetsUnreadable("AccessDenied".to_string()))
        );
    }

    #[test]
    fn has_nothing_to_pin_without_a_catalog_major() {
        let instance = PausableInstance {
            instance_identifier: "orders".to_string(),
            database_kind: crate::io_models::terraform::AdoptedDatabaseKind::Postgresql,
            target_hostname: "orders.ab12cd34ef56.eu-west-3.rds.amazonaws.com".to_string(),
            catalog_engine_major: None,
        };
        assert_eq!(pin_engine_version(&instance, &[]), None);
    }

    #[test]
    fn has_nothing_to_pin_on_a_document_database() {
        let instance = PausableInstance {
            instance_identifier: "orders".to_string(),
            database_kind: crate::io_models::terraform::AdoptedDatabaseKind::Mongodb,
            target_hostname: "orders.ab12cd34ef56.eu-west-3.rds.amazonaws.com".to_string(),
            catalog_engine_major: Some(5),
        };
        assert_eq!(pin_engine_version(&instance, &[]), None);
    }
}
