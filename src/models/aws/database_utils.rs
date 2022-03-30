use crate::cloud_provider::utilities::{generate_supported_version, get_supported_version_to_use};
use crate::errors::CommandError;
use std::collections::HashMap;

pub(crate) fn get_managed_mysql_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_mysql_versions = HashMap::new();
    // https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_MySQL.html#MySQL.Concepts.VersionMgmt

    // v5.7
    let mut v57 = generate_supported_version(5, 7, 7, Some(16), Some(34), None);
    v57.remove("5.7.32");
    v57.remove("5.7.29");
    v57.remove("5.7.27");
    v57.remove("5.7.20");
    v57.remove("5.7.18");
    supported_mysql_versions.extend(v57);

    // v8
    let mut v8 = generate_supported_version(8, 0, 0, Some(11), Some(26), None);
    v8.remove("8.0.24");
    v8.remove("8.0.22");
    v8.remove("8.0.18");
    v8.remove("8.0.14");
    v8.remove("8.0.12");
    supported_mysql_versions.extend(v8);

    get_supported_version_to_use("RDS MySQL", supported_mysql_versions, requested_version)
}

pub(crate) fn get_managed_mongodb_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_mongodb_versions = HashMap::new();

    // v3.6.0
    let mongo_version = generate_supported_version(3, 6, 6, Some(0), Some(0), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.0.0
    let mongo_version = generate_supported_version(4, 0, 0, Some(0), Some(0), None);
    supported_mongodb_versions.extend(mongo_version);

    get_supported_version_to_use("DocumentDB", supported_mongodb_versions, requested_version)
}

pub(crate) fn get_managed_postgres_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_postgres_versions = HashMap::new();

    // https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_PostgreSQL.html#PostgreSQL.Concepts

    // v10
    let mut v10 = generate_supported_version(10, 1, 18, None, None, None);
    v10.remove("10.2"); // non supported version by AWS
    v10.remove("10.8"); // non supported version by AWS
    supported_postgres_versions.extend(v10);

    // v11
    let mut v11 = generate_supported_version(11, 1, 13, None, None, None);
    v11.remove("11.3"); // non supported version by AWS
    supported_postgres_versions.extend(v11);

    // v12
    let v12 = generate_supported_version(12, 2, 8, None, None, None);
    supported_postgres_versions.extend(v12);

    // v13
    let v13 = generate_supported_version(13, 1, 4, None, None, None);
    supported_postgres_versions.extend(v13);

    get_supported_version_to_use("Postgresql", supported_postgres_versions, requested_version)
}

pub(crate) fn get_managed_redis_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_redis_versions = HashMap::with_capacity(2);
    // https://docs.aws.amazon.com/AmazonElastiCache/latest/red-ug/supported-engine-versions.html

    supported_redis_versions.insert("6".to_string(), "6.x".to_string());
    supported_redis_versions.insert("5".to_string(), "5.0.6".to_string());

    get_supported_version_to_use("Elasticache", supported_redis_versions, requested_version)
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::utilities::{
        get_self_hosted_mongodb_version, get_self_hosted_mysql_version, get_self_hosted_postgres_version,
        get_self_hosted_redis_version,
    };
    use crate::models::aws::database_utils::{
        get_managed_mongodb_version, get_managed_mysql_version, get_managed_postgres_version, get_managed_redis_version,
    };

    #[test]
    fn check_postgres_version() {
        // managed version
        assert_eq!(get_managed_postgres_version("12".to_string()).unwrap(), "12.8");
        assert_eq!(get_managed_postgres_version("12.3".to_string()).unwrap(), "12.3");
        assert_eq!(
            get_managed_postgres_version("12.3.0".to_string())
                .unwrap_err()
                .message()
                .as_str(),
            "Postgresql 12.3.0 version is not supported"
        );
        assert_eq!(
            get_managed_postgres_version("11.3".to_string())
                .unwrap_err()
                .message()
                .as_str(),
            "Postgresql 11.3 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_self_hosted_postgres_version("12".to_string()).unwrap(), "12.8.0");
        assert_eq!(get_self_hosted_postgres_version("12.8".to_string()).unwrap(), "12.8.0");
        assert_eq!(get_self_hosted_postgres_version("12.3.0".to_string()).unwrap(), "12.3.0");
        assert_eq!(
            get_self_hosted_postgres_version("1.0".to_string())
                .unwrap_err()
                .message()
                .as_str(),
            "Postgresql 1.0 version is not supported"
        );
    }

    #[test]
    fn check_redis_version() {
        // managed version
        assert_eq!(get_managed_redis_version("6".to_string()).unwrap(), "6.x");
        assert_eq!(get_managed_redis_version("5".to_string()).unwrap(), "5.0.6");
        assert_eq!(
            get_managed_redis_version("1.0".to_string())
                .unwrap_err()
                .message()
                .as_str(),
            "Elasticache 1.0 version is not supported"
        );

        // self-hosted version
        assert_eq!(get_self_hosted_redis_version("6".to_string()).unwrap(), "6.0.9");
        assert_eq!(get_self_hosted_redis_version("6.0".to_string()).unwrap(), "6.0.9");
        assert_eq!(
            get_self_hosted_redis_version("1.0".to_string())
                .unwrap_err()
                .message()
                .as_str(),
            "Redis 1.0 version is not supported"
        );
    }

    #[test]
    fn check_mysql_version() {
        // managed version
        assert_eq!(get_managed_mysql_version("8".to_string()).unwrap(), "8.0.26");
        assert_eq!(get_managed_mysql_version("8.0".to_string()).unwrap(), "8.0.26");
        assert_eq!(get_managed_mysql_version("8.0.16".to_string()).unwrap(), "8.0.16");
        assert_eq!(
            get_managed_mysql_version("8.0.18".to_string())
                .unwrap_err()
                .message()
                .as_str(),
            "RDS MySQL 8.0.18 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_self_hosted_mysql_version("5".to_string()).unwrap(), "5.7.34");
        assert_eq!(get_self_hosted_mysql_version("5.7".to_string()).unwrap(), "5.7.34");
        assert_eq!(get_self_hosted_mysql_version("5.7.31".to_string()).unwrap(), "5.7.31");
        assert_eq!(
            get_self_hosted_mysql_version("1.0".to_string())
                .unwrap_err()
                .message()
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
                .message()
                .as_str(),
            "DocumentDB 4.4 version is not supported"
        );
        // self-hosted version
        assert_eq!(get_self_hosted_mongodb_version("4".to_string()).unwrap(), "4.4.4");
        assert_eq!(get_self_hosted_mongodb_version("4.2".to_string()).unwrap(), "4.2.12");
        assert_eq!(
            get_self_hosted_mongodb_version("3.4".to_string())
                .unwrap_err()
                .message()
                .as_str(),
            "MongoDB 3.4 version is not supported"
        );
    }
}
