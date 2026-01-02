use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct KedaParameters {
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keda_parameters_default() {
        let params = KedaParameters::default();
        assert!(!params.enabled);
    }

    #[test]
    fn test_keda_parameters_serialization() {
        let params = KedaParameters { enabled: true };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"enabled\":true"));

        let deserialized: KedaParameters = serde_json::from_str(&json).unwrap();
        assert_eq!(params, deserialized);
    }
}
