use crate::environment::models::types::Percentage;
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serializer, de};
use std::fmt;

impl<'de> Deserialize<'de> for Percentage {
    fn deserialize<D>(deserializer: D) -> Result<Percentage, D::Error>
    where
        D: Deserializer<'de>,
    {
        match deserializer.deserialize_f64(PercentageVisitor) {
            Ok(value) => Percentage::try_from(value).map_err(de::Error::custom),
            Err(e) => Err(e),
        }
    }
}

struct PercentageVisitor;

impl Visitor<'_> for PercentageVisitor {
    type Value = f64;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a percentage value between 0.0 and 1.0")
    }

    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if !(0.0..=1.0).contains(&value) {
            Err(E::custom("Percentage value is out of range"))
        } else {
            Ok(value as f64)
        }
    }
    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if !(0.0..=1.0).contains(&value) {
            Err(E::custom("Percentage value is out of range"))
        } else {
            Ok(value)
        }
    }
}

// HTTP Status Code utilities

/// Validates that a status code is in the valid HTTP range (100-599)
fn is_valid_http_status_code(code: u16) -> bool {
    (100..=599).contains(&code)
}

/// Custom deserializer for HTTP status codes from comma-separated string
/// Supports formats like "404,503" or "400,401,403,404,500"
pub mod http_status_codes {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u16>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            None => Ok(None),
            Some(s) if s.trim().is_empty() => Ok(None),
            Some(s) => {
                let codes: Result<Vec<u16>, _> = s
                    .split(',')
                    .map(|code| {
                        let trimmed = code.trim();
                        trimmed
                            .parse::<u16>()
                            .map_err(|_| {
                                de::Error::custom(format!(
                                    "Invalid HTTP status code '{trimmed}', expected a number between 100 and 599",
                                ))
                            })
                            .and_then(|code| {
                                if is_valid_http_status_code(code) {
                                    Ok(code)
                                } else {
                                    Err(de::Error::custom(format!(
                                        "HTTP status code {code} is out of valid range (100-599)",
                                    )))
                                }
                            })
                    })
                    .collect();
                codes.map(Some)
            }
        }
    }

    pub fn serialize<S>(codes: &Option<Vec<u16>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match codes {
            None => serializer.serialize_none(),
            Some(codes) if codes.is_empty() => serializer.serialize_none(),
            Some(codes) => {
                let s = codes.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",");
                serializer.serialize_str(&s)
            }
        }
    }
}

#[cfg(test)]
mod http_status_codes_tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestStruct {
        #[serde(with = "http_status_codes", default)]
        codes: Option<Vec<u16>>,
    }

    // Tests for valid deserialization cases

    #[test]
    fn test_http_status_codes_deserialize_none() {
        let json = r#"{"codes": null}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, None);
    }

    #[test]
    fn test_http_status_codes_deserialize_empty_string() {
        let json = r#"{"codes": ""}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, None);
    }

    #[test]
    fn test_http_status_codes_deserialize_whitespace_only() {
        let json = r#"{"codes": "   "}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, None);
    }

    #[test]
    fn test_http_status_codes_deserialize_single_code() {
        let json = r#"{"codes": "404"}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, Some(vec![404]));
    }

    #[test]
    fn test_http_status_codes_deserialize_multiple_codes() {
        let json = r#"{"codes": "404,503,502"}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, Some(vec![404, 503, 502]));
    }

    #[test]
    fn test_http_status_codes_deserialize_codes_with_spaces() {
        let json = r#"{"codes": "404, 503, 502"}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, Some(vec![404, 503, 502]));
    }

    #[test]
    fn test_http_status_codes_deserialize_codes_with_extra_spaces() {
        let json = r#"{"codes": " 404 ,  503  , 502 "}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, Some(vec![404, 503, 502]));
    }

    #[test]
    fn test_http_status_codes_deserialize_all_valid_ranges() {
        // Test boundary values: 100 (min), 599 (max), and some common codes
        let json = r#"{"codes": "100,200,300,400,404,500,599"}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, Some(vec![100, 200, 300, 400, 404, 500, 599]));
    }

    #[test]
    fn test_http_status_codes_deserialize_common_error_codes() {
        let json = r#"{"codes": "400,401,403,404,500,502,503,504"}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, Some(vec![400, 401, 403, 404, 500, 502, 503, 504]));
    }

    // Tests for invalid deserialization cases

    #[test]
    fn test_http_status_codes_deserialize_code_too_low() {
        let json = r#"{"codes": "99"}"#;
        let result = serde_json::from_str::<TestStruct>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("out of valid range"));
        assert!(err.contains("99"));
    }

    #[test]
    fn test_http_status_codes_deserialize_code_too_high() {
        let json = r#"{"codes": "600"}"#;
        let result = serde_json::from_str::<TestStruct>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("out of valid range"));
        assert!(err.contains("600"));
    }

    #[test]
    fn test_http_status_codes_deserialize_invalid_one_code_in_list() {
        let json = r#"{"codes": "404,999,503"}"#;
        let result = serde_json::from_str::<TestStruct>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("out of valid range"));
        assert!(err.contains("999"));
    }

    #[test]
    fn test_http_status_codes_deserialize_non_numeric() {
        let json = r#"{"codes": "abc"}"#;
        let result = serde_json::from_str::<TestStruct>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid HTTP status code"));
        assert!(err.contains("abc"));
    }

    #[test]
    fn test_http_status_codes_deserialize_mixed_valid_and_non_numeric() {
        let json = r#"{"codes": "404,abc,503"}"#;
        let result = serde_json::from_str::<TestStruct>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid HTTP status code"));
        assert!(err.contains("abc"));
    }

    #[test]
    fn test_http_status_codes_deserialize_negative_number() {
        let json = r#"{"codes": "-404"}"#;
        let result = serde_json::from_str::<TestStruct>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid HTTP status code"));
    }

    #[test]
    fn test_http_status_codes_deserialize_decimal_number() {
        let json = r#"{"codes": "404.5"}"#;
        let result = serde_json::from_str::<TestStruct>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid HTTP status code"));
    }

    #[test]
    fn test_http_status_codes_deserialize_zero() {
        let json = r#"{"codes": "0"}"#;
        let result = serde_json::from_str::<TestStruct>(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("out of valid range"));
    }

    // Tests for serialization

    #[test]
    fn test_http_status_codes_serialize_none() {
        let test = TestStruct { codes: None };
        let json = serde_json::to_string(&test).expect("Failed to serialize");
        assert_eq!(json, r#"{"codes":null}"#);
    }

    #[test]
    fn test_http_status_codes_serialize_empty_vec() {
        let test = TestStruct { codes: Some(vec![]) };
        let json = serde_json::to_string(&test).expect("Failed to serialize");
        assert_eq!(json, r#"{"codes":null}"#);
    }

    #[test]
    fn test_http_status_codes_serialize_single_code() {
        let test = TestStruct { codes: Some(vec![404]) };
        let json = serde_json::to_string(&test).expect("Failed to serialize");
        assert_eq!(json, r#"{"codes":"404"}"#);
    }

    #[test]
    fn test_http_status_codes_serialize_multiple_codes() {
        let test = TestStruct {
            codes: Some(vec![404, 503, 502]),
        };
        let json = serde_json::to_string(&test).expect("Failed to serialize");
        assert_eq!(json, r#"{"codes":"404,503,502"}"#);
    }

    #[test]
    fn test_http_status_codes_serialize_all_common_codes() {
        let test = TestStruct {
            codes: Some(vec![400, 401, 403, 404, 500, 502, 503, 504]),
        };
        let json = serde_json::to_string(&test).expect("Failed to serialize");
        assert_eq!(json, r#"{"codes":"400,401,403,404,500,502,503,504"}"#);
    }

    // Round-trip tests (serialize then deserialize)

    #[test]
    fn test_http_status_codes_round_trip_none() {
        let original = TestStruct { codes: None };
        let json = serde_json::to_string(&original).expect("Failed to serialize");
        let deserialized: TestStruct = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_http_status_codes_round_trip_single_code() {
        let original = TestStruct { codes: Some(vec![404]) };
        let json = serde_json::to_string(&original).expect("Failed to serialize");
        let deserialized: TestStruct = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_http_status_codes_round_trip_multiple_codes() {
        let original = TestStruct {
            codes: Some(vec![404, 503, 502, 400, 401]),
        };
        let json = serde_json::to_string(&original).expect("Failed to serialize");
        let deserialized: TestStruct = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_http_status_codes_round_trip_boundary_values() {
        let original = TestStruct {
            codes: Some(vec![100, 599]),
        };
        let json = serde_json::to_string(&original).expect("Failed to serialize");
        let deserialized: TestStruct = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(original, deserialized);
    }

    // Edge case tests

    #[test]
    fn test_http_status_codes_deserialize_missing_field() {
        let json = r#"{}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, None);
    }

    #[test]
    fn test_http_status_codes_deserialize_duplicate_codes() {
        let json = r#"{"codes": "404,404,404"}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, Some(vec![404, 404, 404]));
    }

    #[test]
    fn test_http_status_codes_deserialize_unordered_codes() {
        let json = r#"{"codes": "503,404,502,400"}"#;
        let result: TestStruct = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(result.codes, Some(vec![503, 404, 502, 400]));
    }
}
