use crate::environment::models::database::DatabaseError;
use crate::environment::models::types::VersionsNumber;
use crate::infrastructure::models::cloud_provider::service::DatabaseType;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Container PostgreSQL families.
//
// Two families coexist:
//   * majors <= 17 run on the legacy Bitnami chart/image (`pub-mirror-postgresql`, tag = major,
//     e.g. `17`). https://hub.docker.com/r/bitnami/postgresql/tags
//   * majors >= 18 run on the official postgres image (`pub-mirror-postgres`) via the
//     Qovery-authored `postgresql` chart.
//
// The exact image *tag* for the official family is owned by q-core, which sends it as the database
// `version` (e.g. `18.4-trixie`); the engine uses that string verbatim as the tag. The engine only
// derives the family (and therefore the repository/chart) from the major, so onboarding a future
// major needs no engine change — only q-core's version matrix + tag map.
// ---------------------------------------------------------------------------

/// Container PostgreSQL majors served by the legacy Bitnami chart/image.
pub const BITNAMI_POSTGRES_MAJORS: &[&str] = &["10", "11", "12", "13", "14", "15", "16", "17"];

/// First PostgreSQL major served by the official postgres image (non-Bitnami).
pub const OFFICIAL_POSTGRES_MIN_MAJOR: u32 = 18;

/// Repository for the official postgres image. It shares the `pub-mirror-postgresql` mirror with the
/// legacy Bitnami family; the two are told apart only by the tag scheme (major for <= 17, the full
/// Debian-variant tag sent by q-core as the database `version`, e.g. `18.4-trixie`, for >= 18).
pub const OFFICIAL_POSTGRES_IMAGE_REPOSITORY: &str = "pub-mirror-postgres";

/// Whether a PostgreSQL major is served by the legacy Bitnami chart/image.
pub fn is_bitnami_postgres_major(major: &str) -> bool {
    BITNAMI_POSTGRES_MAJORS.contains(&major)
}

/// Whether a PostgreSQL major is served by the official-image chart (q-core supplies the tag).
pub fn is_official_postgres_major(major: &str) -> bool {
    major
        .parse::<u32>()
        .map(|m| m >= OFFICIAL_POSTGRES_MIN_MAJOR)
        .unwrap_or(false)
}

pub fn is_allowed_containered_postgres_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    let major = requested_version.major.as_str();
    // Allowed iff the major is a known Bitnami major or belongs to the official family (>= 18).
    // q-core is the authoritative gate (its version matrix); the engine only sanity-checks the family.
    if is_bitnami_postgres_major(major) || is_official_postgres_major(major) {
        return Ok(());
    }

    Err(DatabaseError::UnsupportedDatabaseVersion {
        database_type: DatabaseType::PostgreSQL,
        database_version: Arc::from(requested_version.to_string()),
    })
}

// ---------------------------------------------------------------------------
// Container MySQL families.
//
// Two families coexist:
//   * majors 5 and 8 run on the legacy Bitnami chart/image (`pub-mirror-mysql`, tag = major,
//     e.g. `8`). https://hub.docker.com/r/bitnami/mysql/tags
//   * majors >= 9 run on the official mysql image (`pub-mirror-mysql`) via the Qovery-authored
//     `mysql` chart.
//
// As with PostgreSQL/Redis, the exact image *tag* for the official family is owned by q-core, which
// sends it as the database `version` (e.g. `9.7-oracle`); the engine uses that string verbatim as the
// tag. The engine only derives the family (and therefore the chart) from the major, so onboarding a
// future major needs no engine change — only q-core's version matrix + tag map.
// ---------------------------------------------------------------------------

/// Container MySQL majors served by the legacy Bitnami chart/image.
pub const BITNAMI_MYSQL_MAJORS: &[&str] = &["5", "8"];

/// First MySQL major served by the official mysql image (non-Bitnami).
pub const OFFICIAL_MYSQL_MIN_MAJOR: u32 = 9;

/// Whether a MySQL major is served by the legacy Bitnami chart/image.
pub fn is_bitnami_mysql_major(major: &str) -> bool {
    BITNAMI_MYSQL_MAJORS.contains(&major)
}

/// Whether a MySQL major is served by the official-image chart (q-core supplies the tag).
pub fn is_official_mysql_major(major: &str) -> bool {
    major
        .parse::<u32>()
        .map(|m| m >= OFFICIAL_MYSQL_MIN_MAJOR)
        .unwrap_or(false)
}

pub fn is_allowed_containered_mysql_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    let major = requested_version.major.as_str();
    // Allowed iff the major is a known Bitnami major (5, 8) or belongs to the official family (>= 9).
    // q-core is the authoritative gate (its version matrix); the engine only sanity-checks the family.
    if is_bitnami_mysql_major(major) || is_official_mysql_major(major) {
        return Ok(());
    }

    Err(DatabaseError::UnsupportedDatabaseVersion {
        database_type: DatabaseType::MySQL,
        database_version: Arc::from(requested_version.to_string()),
    })
}

// ---------------------------------------------------------------------------
// Container MongoDB families.
//
// Two families coexist:
//   * up to 8.0 runs on the legacy Bitnami chart/image (`pub-mirror-mongodb`, tag = stored version,
//     e.g. `8.0`). https://hub.docker.com/r/bitnami/mongodb/tags
//   * 8.3 and later run on the official mongo image (`pub-mirror-mongo`) via the Qovery-authored
//     `mongodb` chart.
//
// Unlike PostgreSQL and Redis the family boundary is on (major, minor), not major: 8.0 stays Bitnami
// while 8.3 moves to the official image, so the family cannot be derived from the major alone. The
// exact image *tag* for the official family is owned by q-core, which sends it as the database
// `version` (e.g. `8.3-noble`); the engine uses that string verbatim as the tag and only derives the
// family (and therefore the repository/chart).
// ---------------------------------------------------------------------------

/// Bitnami majors accepted as-is for container MongoDB. Major 8 is shared with the official family,
/// which is told apart by minor (see [`is_official_mongodb_version`]).
const BITNAMI_MONGODB_MAJORS: &[&str] = &["4", "5", "6", "7", "8"];

/// First MongoDB (major, minor) served by the official mongo image (non-Bitnami). 8.0 stays Bitnami.
pub const OFFICIAL_MONGODB_MIN_MAJOR_MINOR: (u32, u32) = (8, 3);

/// Repository for the official mongo image (non-Bitnami). Distinct from the Bitnami-mirrored
/// `pub-mirror-mongodb`: the official Docker image is `mongo`, mirrored as `pub-mirror-mongo`.
pub const OFFICIAL_MONGODB_IMAGE_REPOSITORY: &str = "pub-mirror-mongo";

/// Leading integer of a version component, tolerating an image-variant suffix (e.g. `3-noble` -> 3).
fn leading_u32(component: Option<&String>) -> Option<u32> {
    component
        .and_then(|c| c.split('-').next())
        .and_then(|c| c.parse::<u32>().ok())
}

/// Whether a MongoDB version is served by the official mongo image (non-Bitnami). True for >= 8.3
/// (and any later major); 8.0 and everything below stays on the Bitnami chart/image.
pub fn is_official_mongodb_version(requested_version: &VersionsNumber) -> bool {
    let major = match requested_version.major.parse::<u32>() {
        Ok(m) => m,
        Err(_) => return false,
    };
    let minor = leading_u32(requested_version.minor.as_ref()).unwrap_or(0);
    (major, minor) >= OFFICIAL_MONGODB_MIN_MAJOR_MINOR
}

/// Whether a MongoDB version is served by the legacy Bitnami chart/image (allowed, non-official).
pub fn is_bitnami_mongodb_version(requested_version: &VersionsNumber) -> bool {
    BITNAMI_MONGODB_MAJORS.contains(&requested_version.major.as_str())
        && !is_official_mongodb_version(requested_version)
}

pub fn is_allowed_containered_mongodb_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    // https://hub.docker.com/r/bitnami/mongodb/tags?page=1&ordering=last_updated
    // Allowed iff the major is a known Bitnami major (4-8) or the version belongs to the official
    // family (>= 8.3). q-core is the authoritative gate (its version matrix); the engine only
    // sanity-checks the family.
    if BITNAMI_MONGODB_MAJORS.contains(&requested_version.major.as_str())
        || is_official_mongodb_version(requested_version)
    {
        return Ok(());
    }

    Err(DatabaseError::UnsupportedDatabaseVersion {
        database_type: DatabaseType::MongoDB,
        database_version: Arc::from(requested_version.to_string()),
    })
}

// ---------------------------------------------------------------------------
// Container Redis families.
//
// Two families coexist:
//   * majors <= 7 run on the legacy Bitnami chart/image (`pub-mirror-redis`, tag = major,
//     e.g. `7`). https://hub.docker.com/r/bitnami/redis/tags
//   * majors >= 8 run on the official redis image (`pub-mirror-redis`) via the
//     Qovery-authored `redis` chart.
//
// As with PostgreSQL, the exact image *tag* for the official family is owned by q-core, which sends
// it as the database `version` (e.g. `8.8-trixie`); the engine uses that string verbatim as the tag.
// The engine only derives the family (and therefore the repository/chart) from the major, so
// onboarding a future major needs no engine change — only q-core's version matrix + tag map.
// ---------------------------------------------------------------------------

/// Container Redis majors served by the legacy Bitnami chart/image.
pub const BITNAMI_REDIS_MAJORS: &[&str] = &["5", "6", "7"];

/// First Redis major served by the official redis image (non-Bitnami).
pub const OFFICIAL_REDIS_MIN_MAJOR: u32 = 8;

/// Whether a Redis major is served by the legacy Bitnami chart/image.
pub fn is_bitnami_redis_major(major: &str) -> bool {
    BITNAMI_REDIS_MAJORS.contains(&major)
}

/// Whether a Redis major is served by the official-image chart (q-core supplies the tag).
pub fn is_official_redis_major(major: &str) -> bool {
    major
        .parse::<u32>()
        .map(|m| m >= OFFICIAL_REDIS_MIN_MAJOR)
        .unwrap_or(false)
}

pub fn is_allowed_containered_redis_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    let major = requested_version.major.as_str();
    // Allowed iff the major is a known Bitnami major or belongs to the official family (>= 8).
    // q-core is the authoritative gate (its version matrix); the engine only sanity-checks the family.
    if is_bitnami_redis_major(major) || is_official_redis_major(major) {
        return Ok(());
    }

    Err(DatabaseError::UnsupportedDatabaseVersion {
        database_type: DatabaseType::Redis,
        database_version: Arc::from(requested_version.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use crate::environment::models::database::DatabaseError;
    use crate::environment::models::database_utils::{
        is_allowed_containered_mongodb_version, is_allowed_containered_mysql_version,
        is_allowed_containered_postgres_version, is_allowed_containered_redis_version, is_bitnami_mongodb_version,
        is_bitnami_postgres_major, is_bitnami_redis_major, is_official_mongodb_version, is_official_postgres_major,
        is_official_redis_major,
    };
    use crate::environment::models::types::VersionsNumberBuilder;
    use crate::infrastructure::models::cloud_provider::service::DatabaseType;
    use std::sync::Arc;

    #[test]
    fn test_is_allowed_containered_mysql_versions() {
        // v5
        assert!(is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(5).build()).is_ok());
        assert!(is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(5).minor(1).build()).is_ok());
        assert!(
            is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(5).minor(2).patch(3).build())
                .is_ok()
        );

        // v8
        assert!(is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(8).build()).is_ok());
        assert!(is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(8).minor(1).build()).is_ok());
        assert!(
            is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(8).minor(2).patch(3).build())
                .is_ok()
        );

        // v9 (non-Bitnami, official mysql image)
        assert!(is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(9).build()).is_ok());
        assert!(is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(9).minor(7).build()).is_ok());
        assert!(
            is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(9).minor(7).patch(0).build())
                .is_ok()
        );
    }

    #[test]
    fn test_is_allowed_containered_mysql_unsupported_versions() {
        // unsupported versions
        // <- unsupported versions to be added here
        assert_eq!(
            is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(4).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MySQL,
                database_version: Arc::from("4"),
            }
        );
        assert_eq!(
            is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(6).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MySQL,
                database_version: Arc::from("6"),
            }
        );
        assert_eq!(
            is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(7).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MySQL,
                database_version: Arc::from("7"),
            }
        );
    }

    #[test]
    fn test_is_allowed_containered_redis_versions() {
        // v5
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(5).build()).is_ok());
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(5).minor(2).build()).is_ok());
        assert!(
            is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(5).minor(3).patch(5).build())
                .is_ok()
        );

        // v6
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(6).build()).is_ok());
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(6).minor(3).build()).is_ok());
        assert!(
            is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(6).minor(4).patch(6).build())
                .is_ok()
        );

        // v7
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(7).build()).is_ok());
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(7).minor(4).build()).is_ok());
        assert!(
            is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(7).minor(5).patch(7).build())
                .is_ok()
        );

        // v8 (non-Bitnami, official redis image)
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(8).build()).is_ok());
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(8).minor(8).build()).is_ok());
        assert!(
            is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(8).minor(8).patch(0).build())
                .is_ok()
        );
    }

    #[test]
    fn test_is_allowed_containered_redis_unsupported_versions() {
        // unsupported versions
        // <- unsupported versions to be added here
        assert_eq!(
            is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(4).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::Redis,
                database_version: Arc::from("4"),
            }
        );
    }

    #[test]
    fn test_redis_version_families() {
        // Bitnami family (5-7): served by the Bitnami chart/image.
        assert!(is_bitnami_redis_major("7"));
        assert!(!is_official_redis_major("7"));

        // Official family (>= 8): q-core supplies the exact image tag; the engine just recognises
        // the family. No per-major engine list, so future majors (9, 10, …) are already accepted.
        assert!(!is_bitnami_redis_major("8"));
        assert!(is_official_redis_major("8"));
        assert!(is_official_redis_major("9"));
        assert!(is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(9).build()).is_ok());

        // Below the supported range stays rejected.
        assert!(!is_bitnami_redis_major("4"));
        assert!(!is_official_redis_major("4"));
    }

    #[test]
    fn test_is_allowed_containered_mongodb_versions() {
        // v4
        assert!(is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(4).build()).is_ok());
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(4).minor(1).build()).is_ok()
        );
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(4).minor(2).patch(3).build())
                .is_ok()
        );

        // v5
        assert!(is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(5).build()).is_ok());
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(5).minor(2).build()).is_ok()
        );
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(5).minor(3).patch(4).build())
                .is_ok()
        );

        // v6
        assert!(is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(6).build()).is_ok());
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(6).minor(3).build()).is_ok()
        );
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(6).minor(4).patch(5).build())
                .is_ok()
        );

        // v7
        assert!(is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(7).build()).is_ok());
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(7).minor(4).build()).is_ok()
        );
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(7).minor(5).patch(6).build())
                .is_ok()
        );

        // v8
        assert!(is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(8).build()).is_ok());
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(8).minor(4).build()).is_ok()
        );
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(8).minor(5).patch(6).build())
                .is_ok()
        );
    }

    #[test]
    fn test_is_allowed_containered_mongodb_unsupported_versions() {
        // Below the supported range stays rejected (regardless of minor).
        assert_eq!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(3).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MongoDB,
                database_version: Arc::from("3"),
            }
        );
        assert_eq!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(2).minor(6).build())
                .unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MongoDB,
                database_version: Arc::from("2.6"),
            }
        );
    }

    #[test]
    fn test_mongodb_version_families() {
        // Bitnami family (4-8.0): served by the Bitnami chart/image. The boundary is on (major, minor),
        // so 8.0/8.1/8.2 stay Bitnami while 8.3 crosses over.
        assert!(is_bitnami_mongodb_version(&VersionsNumberBuilder::new().major(7).build()));
        assert!(is_bitnami_mongodb_version(
            &VersionsNumberBuilder::new().major(8).minor(0).build()
        ));
        assert!(!is_official_mongodb_version(
            &VersionsNumberBuilder::new().major(8).minor(0).build()
        ));
        assert!(!is_official_mongodb_version(&VersionsNumberBuilder::new().major(8).build()));

        // Official family (>= 8.3): q-core supplies the exact image tag (e.g. `8.3-noble`); the engine
        // recognises the family from (major, minor) and tolerates the variant suffix on the minor.
        assert!(is_official_mongodb_version(
            &VersionsNumberBuilder::new().major(8).minor(3).build()
        ));
        assert!(!is_bitnami_mongodb_version(
            &VersionsNumberBuilder::new().major(8).minor(3).build()
        ));
        assert!(is_official_mongodb_version(&VersionsNumberBuilder::new().major(9).build()));
        assert!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(8).minor(3).build()).is_ok()
        );
        assert!(is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(9).build()).is_ok());

        // The minor parser tolerates the image-variant suffix that q-core sends as the version.
        assert!(is_official_mongodb_version(
            &VersionsNumberBuilder::new()
                .major(8)
                .minor_str(Arc::from("3-noble"))
                .build()
        ));

        // Below the supported range belongs to neither family.
        assert!(!is_bitnami_mongodb_version(&VersionsNumberBuilder::new().major(3).build()));
        assert!(!is_official_mongodb_version(&VersionsNumberBuilder::new().major(3).build()));
    }

    #[test]
    fn test_is_allowed_containered_postgres_versions() {
        // v11
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(11).build()).is_ok());
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(11).minor(6).build()).is_ok()
        );
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(11).minor(7).patch(2).build())
                .is_ok()
        );

        // v12
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(12).build()).is_ok());
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(12).minor(7).build()).is_ok()
        );
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(12).minor(8).patch(3).build())
                .is_ok()
        );

        // v13
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(13).build()).is_ok());
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(13).minor(8).build()).is_ok()
        );
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(13).minor(9).patch(4).build())
                .is_ok()
        );

        // v14
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(14).build()).is_ok());
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(14).minor(9).build()).is_ok()
        );
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(14).minor(10).patch(5).build())
                .is_ok()
        );

        // v15
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(15).build()).is_ok());
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(15).minor(10).build()).is_ok()
        );
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(15).minor(11).patch(6).build())
                .is_ok()
        );

        // v16
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(16).build()).is_ok());
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(16).minor(11).build()).is_ok()
        );
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(16).minor(12).patch(7).build())
                .is_ok()
        );

        // v17
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(17).build()).is_ok());
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(17).minor(11).build()).is_ok()
        );
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(17).minor(12).patch(7).build())
                .is_ok()
        );

        // v18 (non-Bitnami, official postgres image)
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(18).build()).is_ok());
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(18).minor(4).build()).is_ok()
        );
        assert!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(18).minor(4).patch(0).build())
                .is_ok()
        );
    }

    #[test]
    fn test_is_allowed_containered_postgres_unsupported_versions() {
        // unsupported versions
        // <- unsupported versions to be added here
        assert_eq!(
            is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(9).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::PostgreSQL,
                database_version: Arc::from("9"),
            }
        );
    }

    #[test]
    fn test_postgres_version_families() {
        // Bitnami family (10-17): served by the Bitnami chart/image.
        assert!(is_bitnami_postgres_major("17"));
        assert!(!is_official_postgres_major("17"));

        // Official family (>= 18): q-core supplies the exact image tag; the engine just recognises
        // the family. No per-major engine list, so future majors (19, 20, …) are already accepted.
        assert!(!is_bitnami_postgres_major("18"));
        assert!(is_official_postgres_major("18"));
        assert!(is_official_postgres_major("19"));
        assert!(is_allowed_containered_postgres_version(&VersionsNumberBuilder::new().major(19).build()).is_ok());

        // Below the supported range stays rejected.
        assert!(!is_bitnami_postgres_major("9"));
        assert!(!is_official_postgres_major("9"));
    }
}
