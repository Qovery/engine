use crate::errors::CommandError;
use crate::models::database_utils::get_supported_version_to_use;
use std::collections::HashMap;

pub(super) fn pick_managed_postgres_version(requested_version: String) -> Result<String, CommandError> {
    // Scaleway supported postgres versions
    // https://api.scaleway.com/rdb/v1/regions/fr-par/database-engines
    let mut supported_postgres_versions = HashMap::new();

    // {"name":"PostgreSQL","version":"13","end_of_life":"2025-11-13T00:00:00Z"}
    // {"name":"PostgreSQL","version":"12","end_of_life":"2024-11-14T00:00:00Z"}
    // {"name":"PostgreSQL","version":"11","end_of_life":"2023-11-09T00:00:00Z"}
    // {"name":"PostgreSQL","version":"10","end_of_life":"2022-11-10T00:00:00Z"}
    supported_postgres_versions.insert("10".to_string(), "10".to_string());
    supported_postgres_versions.insert("10.0".to_string(), "10.0".to_string());
    supported_postgres_versions.insert("11".to_string(), "11".to_string());
    supported_postgres_versions.insert("11.0".to_string(), "11.0".to_string());
    supported_postgres_versions.insert("12".to_string(), "12".to_string());
    supported_postgres_versions.insert("12.0".to_string(), "12.0".to_string());
    supported_postgres_versions.insert("13".to_string(), "13".to_string());
    supported_postgres_versions.insert("13.0".to_string(), "13.0".to_string());

    get_supported_version_to_use("RDB postgres", supported_postgres_versions, requested_version)
}

pub(super) fn pick_managed_mysql_version(requested_version: String) -> Result<String, CommandError> {
    // Scaleway supported MySQL versions
    // https://api.scaleway.com/rdb/v1/regions/fr-par/database-engines
    let mut supported_mysql_versions = HashMap::new();

    // {"name": "MySQL", "version":"8","end_of_life":"2026-04-01T00:00:00Z"}
    supported_mysql_versions.insert("8".to_string(), "8".to_string());
    supported_mysql_versions.insert("8.0".to_string(), "8.0".to_string());

    get_supported_version_to_use("RDB MySQL", supported_mysql_versions, requested_version)
}
