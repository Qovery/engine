use crate::errors::CommandError;
use crate::models::types::VersionsNumber;
use std::collections::HashMap;
use std::str::FromStr;

pub fn get_self_hosted_postgres_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_postgres_versions = HashMap::new();

    // https://hub.docker.com/r/bitnami/postgresql/tags?page=1&ordering=last_updated

    // v10
    let v10 = generate_supported_version(10, 1, 16, Some(0), Some(0), None);
    supported_postgres_versions.extend(v10);

    // v11
    let v11 = generate_supported_version(11, 1, 11, Some(0), Some(0), None);
    supported_postgres_versions.extend(v11);

    // v12
    let v12 = generate_supported_version(12, 2, 8, Some(0), Some(0), None);
    supported_postgres_versions.extend(v12);

    // v13
    let v13 = generate_supported_version(13, 1, 4, Some(0), Some(0), None);
    supported_postgres_versions.extend(v13);

    get_supported_version_to_use("Postgresql", supported_postgres_versions, requested_version)
}

pub fn get_self_hosted_mysql_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_mysql_versions = HashMap::new();
    // https://hub.docker.com/r/bitnami/mysql/tags?page=1&ordering=last_updated

    // v5.7
    let v57 = generate_supported_version(5, 7, 7, Some(16), Some(34), None);
    supported_mysql_versions.extend(v57);

    // v8
    let v8 = generate_supported_version(8, 0, 0, Some(11), Some(24), None);
    supported_mysql_versions.extend(v8);

    get_supported_version_to_use("MySQL", supported_mysql_versions, requested_version)
}

pub fn get_self_hosted_mongodb_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_mongodb_versions = HashMap::new();

    // https://hub.docker.com/r/bitnami/mongodb/tags?page=1&ordering=last_updated

    // v3.6
    let mongo_version = generate_supported_version(3, 6, 6, Some(0), Some(22), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.0
    let mongo_version = generate_supported_version(4, 0, 0, Some(0), Some(23), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.2
    let mongo_version = generate_supported_version(4, 2, 2, Some(0), Some(12), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.4
    let mongo_version = generate_supported_version(4, 4, 4, Some(0), Some(4), None);
    supported_mongodb_versions.extend(mongo_version);

    get_supported_version_to_use("MongoDB", supported_mongodb_versions, requested_version)
}

pub fn get_self_hosted_redis_version(requested_version: String) -> Result<String, CommandError> {
    let mut supported_redis_versions = HashMap::with_capacity(4);
    // https://hub.docker.com/r/bitnami/redis/tags?page=1&ordering=last_updated

    supported_redis_versions.insert("6".to_string(), "6.0.9".to_string());
    supported_redis_versions.insert("6.0".to_string(), "6.0.9".to_string());
    supported_redis_versions.insert("5".to_string(), "5.0.10".to_string());
    supported_redis_versions.insert("5.0".to_string(), "5.0.10".to_string());

    get_supported_version_to_use("Redis", supported_redis_versions, requested_version)
}

pub fn get_supported_version_to_use(
    database_name: &str,
    all_supported_versions: HashMap<String, String>,
    version_to_check: String,
) -> Result<String, CommandError> {
    let version = VersionsNumber::from_str(version_to_check.as_str())?;

    // if a patch version is required
    if version.patch.is_some() {
        return match all_supported_versions.get(&format!(
            "{}.{}.{}",
            version.major,
            version.minor.unwrap(),
            version.patch.unwrap()
        )) {
            Some(version) => Ok(version.to_string()),
            None => {
                return Err(CommandError::new_from_safe_message(format!(
                    "{} {} version is not supported",
                    database_name, version_to_check
                )));
            }
        };
    }

    // if a minor version is required
    if version.minor.is_some() {
        return match all_supported_versions.get(&format!("{}.{}", version.major, version.minor.unwrap())) {
            Some(version) => Ok(version.to_string()),
            None => {
                return Err(CommandError::new_from_safe_message(format!(
                    "{} {} version is not supported",
                    database_name, version_to_check
                )));
            }
        };
    };

    // if only a major version is required
    match all_supported_versions.get(&version.major) {
        Some(version) => Ok(version.to_string()),
        None => {
            return Err(CommandError::new_from_safe_message(format!(
                "{} {} version is not supported",
                database_name, version_to_check
            )));
        }
    }
}

// Ease the support of multiple versions by range
pub fn generate_supported_version(
    major: i32,
    minor_min: i32,
    minor_max: i32,
    update_min: Option<i32>,
    update_max: Option<i32>,
    suffix_version: Option<String>,
) -> HashMap<String, String> {
    let mut supported_versions = HashMap::new();
    let latest_major_version;

    // blank suffix if not requested
    let suffix = match suffix_version {
        Some(suffix) => suffix,
        None => "".to_string(),
    };

    let _ = match update_min {
        // manage minor with updates
        Some(_) => {
            latest_major_version = format!("{}.{}.{}{}", major, minor_max, update_max.unwrap(), suffix);

            if minor_min == minor_max {
                // add short minor format targeting latest version
                supported_versions.insert(format!("{}.{}", major, minor_max), latest_major_version.clone());
                if update_min.unwrap() == update_max.unwrap() {
                    let version = format!("{}.{}.{}", major, minor_min, update_min.unwrap());
                    supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                } else {
                    for update in update_min.unwrap()..update_max.unwrap() + 1 {
                        let version = format!("{}.{}.{}", major, minor_min, update);
                        supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                    }
                }
            } else {
                for minor in minor_min..minor_max + 1 {
                    // add short minor format targeting latest version
                    supported_versions.insert(
                        format!("{}.{}", major, minor),
                        format!("{}.{}.{}", major, minor, update_max.unwrap()),
                    );
                    if update_min.unwrap() == update_max.unwrap() {
                        let version = format!("{}.{}.{}", major, minor, update_min.unwrap());
                        supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                    } else {
                        for update in update_min.unwrap()..update_max.unwrap() + 1 {
                            let version = format!("{}.{}.{}", major, minor, update);
                            supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                        }
                    }
                }
            }
        }
        // manage minor without updates
        None => {
            latest_major_version = format!("{}.{}{}", major, minor_max, suffix);
            for minor in minor_min..minor_max + 1 {
                let version = format!("{}.{}", major, minor);
                supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
            }
        }
    };

    // default major + major.minor supported version
    supported_versions.insert(major.to_string(), latest_major_version);

    supported_versions
}
