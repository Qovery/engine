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

pub fn is_allowed_containered_mysql_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    // https://hub.docker.com/r/bitnami/mysql/tags?page=1&ordering=last_updated

    // Allow only major 5 and 8
    if !&["5", "8"].contains(&requested_version.major.as_str()) {
        return Err(DatabaseError::UnsupportedDatabaseVersion {
            database_type: DatabaseType::MySQL,
            database_version: Arc::from(requested_version.to_string()),
        });
    }

    // If we want to filter out some versions, we should filter those out here
    // <-

    Ok(())
}

pub fn is_allowed_containered_mongodb_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    // https://hub.docker.com/r/bitnami/mongodb/tags?page=1&ordering=last_updated

    // Allow only major 4, 5, 6, 7 and 8
    if !&["4", "5", "6", "7", "8"].contains(&requested_version.major.as_str()) {
        return Err(DatabaseError::UnsupportedDatabaseVersion {
            database_type: DatabaseType::MongoDB,
            database_version: Arc::from(requested_version.to_string()),
        });
    }

    // If we want to filter out some versions, we should filter those out here
    // <-

    Ok(())
}

pub fn is_allowed_containered_redis_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    // https://hub.docker.com/r/bitnami/redis/tags?page=1&ordering=last_updated

    // Allow only major 5, 6 and 7
    if !&["5", "6", "7"].contains(&requested_version.major.as_str()) {
        return Err(DatabaseError::UnsupportedDatabaseVersion {
            database_type: DatabaseType::Redis,
            database_version: Arc::from(requested_version.to_string()),
        });
    }

    // If we want to filter out some versions, we should filter those out here
    // <-

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::environment::models::database::DatabaseError;
    use crate::environment::models::database_utils::{
        is_allowed_containered_mongodb_version, is_allowed_containered_mysql_version,
        is_allowed_containered_postgres_version, is_allowed_containered_redis_version, is_bitnami_postgres_major,
        is_official_postgres_major,
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
        assert_eq!(
            is_allowed_containered_mysql_version(&VersionsNumberBuilder::new().major(9).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MySQL,
                database_version: Arc::from("9"),
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
        assert_eq!(
            is_allowed_containered_redis_version(&VersionsNumberBuilder::new().major(8).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::Redis,
                database_version: Arc::from("8"),
            }
        );
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
        // unsupported versions
        // <- unsupported versions to be added here
        assert_eq!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(3).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MongoDB,
                database_version: Arc::from("3"),
            }
        );
        assert_eq!(
            is_allowed_containered_mongodb_version(&VersionsNumberBuilder::new().major(9).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MongoDB,
                database_version: Arc::from("9"),
            }
        );
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
