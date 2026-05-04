#[derive(thiserror::Error, Debug, PartialEq)]
pub enum BlueprintError {
    #[error(
        "Blueprint tag does not respect the following format '<provider>/<service_name>/<service_version>/<manifest_version>'"
    )]
    InvalidTagFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_describes_expected_format() {
        let err = BlueprintError::InvalidTagFormat;
        let msg = format!("{}", err);
        assert!(msg.contains("<provider>"));
        assert!(msg.contains("<service_name>"));
        assert!(msg.contains("<service_version>"));
        assert!(msg.contains("<manifest_version>"));
    }
}
