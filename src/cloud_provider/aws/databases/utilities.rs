use crate::cloud_provider::utilities::VersionsNumber;
use crate::errors::CommandError;
use crate::models::DatabaseKind;

pub fn get_parameter_group_from_version(
    version: VersionsNumber,
    database_kind: DatabaseKind,
) -> Result<String, CommandError> {
    if version.minor.is_none() {
        return Err(CommandError::new_from_safe_message(format!(
            "Can't determine the minor version, to select parameter group for {:?} version {}",
            database_kind, version
        )));
    };

    match database_kind {
        DatabaseKind::Mysql => Ok(format!("mysql{}.{}", version.major, version.minor.unwrap())),
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
    use crate::cloud_provider::utilities::VersionsNumber;
    use crate::models::DatabaseKind;
    use std::str::FromStr;

    #[test]
    fn check_rds_mysql_parameter_groups() {
        let mysql_parameter_group = get_parameter_group_from_version(
            VersionsNumber::from_str("5.7.0").expect("error while trying to get version from str"),
            DatabaseKind::Mysql,
        );
        assert_eq!(mysql_parameter_group.unwrap(), "mysql5.7");

        let mysql_parameter_group = get_parameter_group_from_version(
            VersionsNumber::from_str("8.0").expect("error while trying to get version from str"),
            DatabaseKind::Mysql,
        );
        assert_eq!(mysql_parameter_group.unwrap(), "mysql8.0");

        let mysql_parameter_group = get_parameter_group_from_version(
            VersionsNumber::from_str("8").expect("error while trying to get version from str"),
            DatabaseKind::Mysql,
        );
        assert_eq!(
            mysql_parameter_group.unwrap_err().message(),
            "Can't determine the minor version, to select parameter group for Mysql version 8"
        );
    }
}
