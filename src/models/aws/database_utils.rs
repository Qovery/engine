use crate::errors::CommandError;
use crate::models::database_utils::{generate_supported_version, get_supported_version_to_use};
use std::collections::HashMap;

pub(super) fn get_managed_mysql_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_mysql_versions = HashMap::new();
    // https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_MySQL.html#MySQL.Concepts.VersionMgmt

    // v5.7
    let mut v57 = generate_supported_version(5, 7, 7, Some(33), Some(38), None);
    v57.remove("5.7.35");
    v57.remove("5.7.36");
    supported_mysql_versions.extend(v57);

    // v8
    let mut v8 = generate_supported_version(8, 0, 0, Some(23), Some(28), None);
    v8.remove("8.0.24");
    supported_mysql_versions.extend(v8);

    get_supported_version_to_use("RDS MySQL", supported_mysql_versions, requested_version)
}

pub(super) fn get_managed_mongodb_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_mongodb_versions = HashMap::new();

    // v3.6.0
    let mongo_version = generate_supported_version(3, 6, 6, Some(0), Some(0), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.0.0
    let mongo_version = generate_supported_version(4, 0, 0, Some(0), Some(0), None);
    supported_mongodb_versions.extend(mongo_version);

    get_supported_version_to_use("DocumentDB", supported_mongodb_versions, requested_version)
}

pub(super) fn get_managed_postgres_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_postgres_versions = HashMap::new();

    // https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_PostgreSQL.html#PostgreSQL.Concepts

    // v10
    let v10 = generate_supported_version(10, 17, 21, None, None, None);
    supported_postgres_versions.extend(v10);

    // v11
    let v11 = generate_supported_version(11, 12, 16, None, None, None);
    supported_postgres_versions.extend(v11);

    // v12
    let v12 = generate_supported_version(12, 7, 11, None, None, None);
    supported_postgres_versions.extend(v12);

    // v13
    let v13 = generate_supported_version(13, 3, 7, None, None, None);
    supported_postgres_versions.extend(v13);

    let v14 = generate_supported_version(14, 1, 3, None, None, None);
    supported_postgres_versions.extend(v14);

    get_supported_version_to_use("Postgresql", supported_postgres_versions, requested_version)
}

pub(super) fn get_managed_redis_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_redis_versions = HashMap::with_capacity(2);
    // https://docs.aws.amazon.com/AmazonElastiCache/latest/red-ug/supported-engine-versions.html

    supported_redis_versions.insert("7".to_string(), "7.0".to_string());
    supported_redis_versions.insert("6".to_string(), "6.x".to_string());
    supported_redis_versions.insert("5".to_string(), "5.0.6".to_string());

    get_supported_version_to_use("Elasticache", supported_redis_versions, requested_version)
}

#[cfg(test)]
mod tests {
    use crate::errors::ErrorMessageVerbosity::SafeOnly;
    use crate::models::aws::database_utils::{
        get_managed_mongodb_version, get_managed_mysql_version, get_managed_postgres_version, get_managed_redis_version,
    };
    use crate::models::database_utils::{
        get_self_hosted_mongodb_version, get_self_hosted_mysql_version, get_self_hosted_postgres_version,
        get_self_hosted_redis_version,
    };

    #[test]
    fn check_postgres_version() {
        // managed version
        assert_eq!(get_managed_postgres_version("12".to_string()).unwrap(), "12.11");
        assert_eq!(get_managed_postgres_version("12.7".to_string()).unwrap(), "12.7");
        assert_eq!(get_managed_postgres_version("13".to_string()).unwrap(), "13.7");
        assert_eq!(get_managed_postgres_version("13.5".to_string()).unwrap(), "13.5");
        assert_eq!(get_managed_postgres_version("14".to_string()).unwrap(), "14.3");
        assert_eq!(get_managed_postgres_version("14.2".to_string()).unwrap(), "14.2");
        assert_eq!(
            get_managed_postgres_version("12.3.0".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "Postgresql 12.3.0 version is not supported"
        );
        assert_eq!(
            get_managed_postgres_version("11.3".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "Postgresql 11.3 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_self_hosted_postgres_version("14".to_string()).unwrap(), "14.7.0");
        assert_eq!(get_self_hosted_postgres_version("14.4".to_string()).unwrap(), "14.4.0");
        assert_eq!(get_self_hosted_postgres_version("14.4.0".to_string()).unwrap(), "14.4.0");
        assert_eq!(
            get_self_hosted_postgres_version("1.0".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "Postgresql 1.0 version is not supported"
        );
    }

    #[test]
    fn check_redis_version() {
        // managed version
        assert_eq!(get_managed_redis_version("7".to_string()).unwrap(), "7.0");
        assert_eq!(get_managed_redis_version("6".to_string()).unwrap(), "6.x");
        assert_eq!(get_managed_redis_version("5".to_string()).unwrap(), "5.0.6");
        assert_eq!(
            get_managed_redis_version("1.0".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "Elasticache 1.0 version is not supported"
        );

        // self-hosted version
        assert_eq!(get_self_hosted_redis_version("7".to_string()).unwrap(), "7.0.9");
        assert_eq!(get_self_hosted_redis_version("6".to_string()).unwrap(), "6.2.11");
        assert_eq!(get_self_hosted_redis_version("6.0".to_string()).unwrap(), "6.2.11");
        assert_eq!(
            get_self_hosted_redis_version("1.0".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "Redis 1.0 version is not supported"
        );
    }

    #[test]
    fn check_mysql_version() {
        // managed version
        assert_eq!(get_managed_mysql_version("8".to_string()).unwrap(), "8.0.28");
        assert_eq!(get_managed_mysql_version("8.0".to_string()).unwrap(), "8.0.28");
        assert_eq!(get_managed_mysql_version("8.0.27".to_string()).unwrap(), "8.0.27");
        assert_eq!(
            get_managed_mysql_version("8.0.31".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "RDS MySQL 8.0.31 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_self_hosted_mysql_version("5".to_string()).unwrap(), "5.7.41");
        assert_eq!(get_self_hosted_mysql_version("5.7".to_string()).unwrap(), "5.7.41");
        assert_eq!(get_self_hosted_mysql_version("5.7.31".to_string()).unwrap(), "5.7.31");
        assert_eq!(
            get_self_hosted_mysql_version("1.0".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "MySQL 1.0 version is not supported"
        );
    }

    #[test]
    fn check_mongodb_version() {
        // managed version
        assert_eq!(get_managed_mongodb_version("4".to_string()).unwrap(), "4.0.0");
        assert_eq!(get_managed_mongodb_version("4.0".to_string()).unwrap(), "4.0.0");
        assert_eq!(
            get_managed_mongodb_version("4.4".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "DocumentDB 4.4 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_self_hosted_mongodb_version("4".to_string()).unwrap(), "4.4.15");
        assert_eq!(get_self_hosted_mongodb_version("4.2".to_string()).unwrap(), "4.2.21");
        assert_eq!(
            get_self_hosted_mongodb_version("3.4".to_string())
                .unwrap_err()
                .message(SafeOnly)
                .as_str(),
            "MongoDB 3.4 version is not supported"
        );
    }
}
