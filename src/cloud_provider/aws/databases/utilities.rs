use crate::cloud_provider::utilities::VersionsNumber;
use crate::error::StringError;
use crate::models::DatabaseKind;
use std::str::FromStr;

pub fn get_parameter_group_from_version(version: &str, database_kind: DatabaseKind) -> Result<String, StringError> {
    let version_number = match VersionsNumber::from_str(version) {
        Ok(v) => {
            if v.minor.is_none() {
                return Err(format!(
                    "Can't determine the minor version, to select parameter group for {:?} version {}",
                    database_kind, version
                ));
            };
            v
        }
        Err(e) => return Err(e),
    };

    match database_kind {
        DatabaseKind::Mysql => Ok(format!(
            "mysql{}.{}",
            version_number.major,
            version_number.minor.unwrap()
        )),
        _ => Ok("".to_string()),
    }
}

// name of the last snapshot before the database get deleted
pub fn aws_final_snapshot_name(database_name: &str) -> String {
    format!("qovery-{}-final-snap", database_name)
}

#[cfg(test)]
mod tests_aws_databases_parameters {
    use crate::cloud_provider::aws::databases::utilities::get_parameter_group_from_version;
    use crate::models::DatabaseKind;

    #[test]
    fn check_rds_mysql_parameter_groups() {
        let mysql_parameter_group = get_parameter_group_from_version("5.7.0", DatabaseKind::Mysql);
        assert_eq!(mysql_parameter_group.unwrap(), "mysql5.7");

        let mysql_parameter_group = get_parameter_group_from_version("8.0", DatabaseKind::Mysql);
        assert_eq!(mysql_parameter_group.unwrap(), "mysql8.0");

        let mysql_parameter_group = get_parameter_group_from_version("8", DatabaseKind::Mysql);
        assert_eq!(
            mysql_parameter_group.unwrap_err(),
            "Can't determine the minor version, to select parameter group for Mysql version 8"
        );
    }
}
