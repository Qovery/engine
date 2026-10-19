use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// DTO layer - abstract deserialization from JSON input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsManagerAccessDto {
    pub id: String,
    #[serde(deserialize_with = "string_map_without_nulls")]
    pub endpoint: HashMap<String, String>,
    #[serde(deserialize_with = "string_map_without_nulls")]
    pub authentication: HashMap<String, String>,
}

/// q-core writes unset optional values as `null` (e.g. an assume role without external id):
/// read them as absent keys instead of failing the whole cluster request.
fn string_map_without_nulls<'de, D: Deserializer<'de>>(deserializer: D) -> Result<HashMap<String, String>, D::Error> {
    let map = HashMap::<String, Option<String>>::deserialize(deserializer)?;
    Ok(map.into_iter().filter_map(|(key, value)| Some((key, value?))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// q-core sends optional keys with a `null` value when unset, e.g. an assume role without external id.
    #[test]
    fn test_deserialize_access_with_null_optional_value() {
        let json = r#"{
            "id": "0199a3c4",
            "endpoint": { "type": "AWS_PARAMETER_STORE", "region": "eu-west-1" },
            "authentication": {
                "mode": "ROLE_ARN",
                "arn_role": "arn:aws:iam::111111111111:role/base",
                "assume_role_arn": "arn:aws:iam::222222222222:role/reader",
                "assume_role_external_id": null
            }
        }"#;

        let dto: SecretsManagerAccessDto = serde_json::from_str(json).unwrap();

        assert_eq!(
            dto.authentication,
            HashMap::from([
                ("mode".to_string(), "ROLE_ARN".to_string()),
                ("arn_role".to_string(), "arn:aws:iam::111111111111:role/base".to_string()),
                (
                    "assume_role_arn".to_string(),
                    "arn:aws:iam::222222222222:role/reader".to_string()
                ),
            ])
        );
    }
}
