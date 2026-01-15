use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all(serialize = "SCREAMING_SNAKE_CASE", deserialize = "SCREAMING_SNAKE_CASE"))]
pub enum KedaResourceProfile {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all(serialize = "SCREAMING_SNAKE_CASE", deserialize = "SCREAMING_SNAKE_CASE"))]
pub enum KedaAvailability {
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct KedaParameters {
    pub enabled: bool,
    #[serde(default)]
    pub resource_profile: KedaResourceProfile,
    #[serde(default)]
    pub availability: KedaAvailability,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keda_parameters_default() {
        let params = KedaParameters::default();
        assert!(!params.enabled);
        assert_eq!(params.resource_profile, KedaResourceProfile::Normal);
        assert_eq!(params.availability, KedaAvailability::Normal);
    }

    #[test]
    fn test_keda_parameters_serialization() {
        let params = KedaParameters {
            enabled: true,
            resource_profile: KedaResourceProfile::High,
            availability: KedaAvailability::High,
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"enabled\":true"));
        assert!(json.contains("\"resource_profile\":\"HIGH\""));
        assert!(json.contains("\"availability\":\"HIGH\""));

        let deserialized: KedaParameters = serde_json::from_str(&json).unwrap();
        assert_eq!(params, deserialized);
    }

    #[test]
    fn test_keda_parameters_missing_profile_fields_deserialize_to_defaults() {
        let json = r#"{"enabled":true}"#;
        let params: KedaParameters = serde_json::from_str(json).unwrap();
        assert!(params.enabled);
        assert_eq!(params.resource_profile, KedaResourceProfile::Normal);
        assert_eq!(params.availability, KedaAvailability::Normal);
    }
}
