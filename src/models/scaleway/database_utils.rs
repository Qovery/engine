use crate::cloud_provider::service::DatabaseType;
use crate::models::database::DatabaseError;
use crate::models::types::VersionsNumber;
use std::sync::Arc;

pub(super) fn is_allowed_managed_postgres_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    // Scaleway supported postgres versions
    // https://api.scaleway.com/rdb/v1/regions/fr-par/database-engines

    // Allow only major from 11 to 15
    if !&["11", "12", "13", "14", "15"].contains(&requested_version.major.as_str()) {
        return Err(DatabaseError::UnsupportedDatabaseVersion {
            database_type: DatabaseType::PostgreSQL,
            database_version: Arc::from(requested_version.to_string()),
        });
    }

    // If we want to filter out some versions, we should filter those out here
    // <-

    Ok(())
}

pub(super) fn is_allowed_managed_mysql_version(requested_version: &VersionsNumber) -> Result<(), DatabaseError> {
    // Scaleway supported MySQL versions
    // https://api.scaleway.com/rdb/v1/regions/fr-par/database-engines

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

#[cfg(test)]
mod tests {
    use crate::cloud_provider::service::DatabaseType;
    use crate::models::database::DatabaseError;
    use crate::models::scaleway::database_utils::{
        is_allowed_managed_mysql_version, is_allowed_managed_postgres_version,
    };
    use crate::models::types::VersionsNumberBuilder;
    use std::sync::Arc;

    #[test]
    fn test_scw_is_allowed_managed_mysql_versions() {
        // v5
        assert!(is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(5).build()).is_ok());
        assert!(is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(5).minor(1).build()).is_ok());
        assert!(
            is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(5).minor(2).patch(3).build()).is_ok()
        );

        // v8
        assert!(is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(8).build()).is_ok());
        assert!(is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(8).minor(1).build()).is_ok());
        assert!(
            is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(8).minor(2).patch(3).build()).is_ok()
        );
    }

    #[test]
    fn test_scw_is_allowed_managed_mysql_unsupported_versions() {
        // unsupported versions
        // <- unsupported versions to be added here
        assert_eq!(
            is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(4).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MySQL,
                database_version: Arc::from("4"),
            }
        );
        assert_eq!(
            is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(6).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MySQL,
                database_version: Arc::from("6"),
            }
        );
        assert_eq!(
            is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(7).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MySQL,
                database_version: Arc::from("7"),
            }
        );
        assert_eq!(
            is_allowed_managed_mysql_version(&VersionsNumberBuilder::new().major(9).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::MySQL,
                database_version: Arc::from("9"),
            }
        );
    }

    #[test]
    fn test_scw_is_allowed_managed_postgres_versions() {
        // v11
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(11).build()).is_ok());
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(11).minor(6).build()).is_ok());
        assert!(
            is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(11).minor(7).patch(2).build())
                .is_ok()
        );

        // v12
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(12).build()).is_ok());
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(12).minor(7).build()).is_ok());
        assert!(
            is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(12).minor(8).patch(3).build())
                .is_ok()
        );

        // v13
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(13).build()).is_ok());
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(13).minor(8).build()).is_ok());
        assert!(
            is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(13).minor(9).patch(4).build())
                .is_ok()
        );

        // v14
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(14).build()).is_ok());
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(14).minor(9).build()).is_ok());
        assert!(is_allowed_managed_postgres_version(
            &VersionsNumberBuilder::new().major(14).minor(10).patch(5).build()
        )
        .is_ok());

        // v15
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(15).build()).is_ok());
        assert!(is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(15).minor(10).build()).is_ok());
        assert!(is_allowed_managed_postgres_version(
            &VersionsNumberBuilder::new().major(15).minor(11).patch(6).build()
        )
        .is_ok());
    }

    #[test]
    fn test_scw_is_allowed_managed_postgres_unsupported_versions() {
        // unsupported versions
        // <- unsupported versions to be added here
        assert_eq!(
            is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(10).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::PostgreSQL,
                database_version: Arc::from("10"),
            }
        );
        assert_eq!(
            is_allowed_managed_postgres_version(&VersionsNumberBuilder::new().major(16).build()).unwrap_err(),
            DatabaseError::UnsupportedDatabaseVersion {
                database_type: DatabaseType::PostgreSQL,
                database_version: Arc::from("16"),
            }
        );
    }
}
