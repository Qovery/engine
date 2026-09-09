use crate::constants::AWS_APN_ID_TAG_KEY;
use std::fmt::{Display, Formatter};

/// AWS Partner Network identifier, required by AWS to measure Qovery-managed resources for the AWS
/// Marketplace listing. Read once at engine startup and carried through `Context`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AwsApnId(Option<String>);

impl AwsApnId {
    pub const ENV_VAR: &'static str = "QOVERY_AWS_APN_ID";
    const NOT_SET: &'static str = "not-set";

    /// Empty and blank values count as unset: `helper.sh` always passes the variable, with an empty
    /// value when the CI/CD secret is missing.
    pub fn new(value: Option<String>) -> Self {
        AwsApnId(value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()))
    }

    /// No identifier to report, either because the variable is missing or because the cloud
    /// provider is not AWS.
    pub fn unset() -> Self {
        AwsApnId(None)
    }

    pub fn is_set(&self) -> bool {
        self.0.is_some()
    }

    /// Renders as `not-set` when unset, so a misconfiguration is visible on the resource rather
    /// than an empty tag that AWS accepts without error.
    pub fn tag_value(&self) -> &str {
        self.0.as_deref().unwrap_or(Self::NOT_SET)
    }

    pub fn warn_if_unset(&self) {
        if !self.is_set() {
            warn!(
                "{} is unset or empty, AWS resources will be tagged `{}={}`",
                Self::ENV_VAR,
                AWS_APN_ID_TAG_KEY,
                self.tag_value()
            );
        }
    }
}

impl Display for AwsApnId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag_value())
    }
}

impl From<&str> for AwsApnId {
    fn from(value: &str) -> Self {
        AwsApnId::new(Some(value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::AwsApnId;

    #[test]
    fn test_missing_variable_is_unset() {
        let apn_id = AwsApnId::new(None);

        assert!(!apn_id.is_set());
        assert_eq!(apn_id.tag_value(), "not-set");
    }

    #[test]
    fn test_empty_variable_is_unset() {
        let apn_id = AwsApnId::new(Some("".to_string()));

        assert!(!apn_id.is_set());
        assert_eq!(apn_id.tag_value(), "not-set");
    }

    #[test]
    fn test_blank_variable_is_unset() {
        let apn_id = AwsApnId::new(Some("   ".to_string()));

        assert!(!apn_id.is_set());
        assert_eq!(apn_id.tag_value(), "not-set");
    }

    #[test]
    fn test_value_is_set_and_trimmed() {
        let apn_id = AwsApnId::new(Some("  pc:986diru4s7d2vuyde5u1nfukv  ".to_string()));

        assert!(apn_id.is_set());
        assert_eq!(apn_id.tag_value(), "pc:986diru4s7d2vuyde5u1nfukv");
    }

    #[test]
    fn test_unset_matches_default() {
        assert_eq!(AwsApnId::unset(), AwsApnId::default());
        assert_eq!(AwsApnId::unset(), AwsApnId::new(None));
    }

    #[test]
    fn test_display_matches_tag_value() {
        assert_eq!(AwsApnId::unset().to_string(), "not-set");
        assert_eq!(AwsApnId::from("pc:test-apn").to_string(), "pc:test-apn");
    }

    #[test]
    fn test_from_str_normalizes_like_new() {
        assert_eq!(AwsApnId::from(""), AwsApnId::unset());
        assert_eq!(AwsApnId::from("  pc:test-apn "), AwsApnId::from("pc:test-apn"));
    }
}
