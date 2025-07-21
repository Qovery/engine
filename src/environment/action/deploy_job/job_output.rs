use serde_derive::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// Used to validate the job json output format with serde
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct JobOutputVariable {
    pub value: String,
    pub sensitive: bool,
    pub description: String,
}

impl Default for JobOutputVariable {
    fn default() -> Self {
        JobOutputVariable {
            value: String::new(),
            sensitive: true,
            description: String::new(),
        }
    }
}

#[derive(Debug)]
pub enum JobOutputSerializationError {
    SerializationError { serde_err: serde_json::Error },
    OutputVariableValidationError { err: String },
}

pub fn serialize_job_output(
    json: &[u8],
    output_variable_validation_pattern: &str,
) -> Result<HashMap<String, JobOutputVariable>, JobOutputSerializationError> {
    let serde_hash_map: HashMap<&str, Value> = serde_json::from_slice(json)
        .map_err(|err| JobOutputSerializationError::SerializationError { serde_err: err })?;
    let mut job_output_variables: HashMap<String, JobOutputVariable> = HashMap::new();
    // Validate all variable names against the pattern
    let re = regex::Regex::new(output_variable_validation_pattern).map_err(|e| {
        JobOutputSerializationError::OutputVariableValidationError {
            err: format!("Invalid regex pattern: {output_variable_validation_pattern}: {e}"),
        }
    })?;

    for (key, value) in serde_hash_map {
        if !re.is_match(key) {
            return Err(JobOutputSerializationError::OutputVariableValidationError {
                err: format!(
                    "Invalid job output variable name: '{key}'. It must match pattern: {output_variable_validation_pattern}"
                ),
            });
        }
        let job_output_variable_object = value.as_object();
        let job_output_variable_hashmap = match job_output_variable_object {
            None => continue,
            Some(hashmap) => hashmap,
        };

        let serde_value_default = &Value::default();
        let value = job_output_variable_hashmap.get("value").unwrap_or(serde_value_default);

        // Get job output 'value' as string or any other type
        let job_output_value = if value.is_string() {
            value.as_str().unwrap_or_default().to_string()
        } else {
            value.to_string()
        };
        let job_output_description = job_output_variable_hashmap
            .get("description")
            .unwrap_or(serde_value_default)
            .as_str()
            .unwrap_or_default()
            .to_string();

        job_output_variables.insert(
            key.to_string(),
            JobOutputVariable {
                value: job_output_value,
                sensitive: job_output_variable_hashmap
                    .get("sensitive")
                    .unwrap_or(serde_value_default)
                    .as_bool()
                    .unwrap_or(false),
                description: job_output_description,
            },
        );
    }
    Ok(job_output_variables)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn should_serialize_json_to_job_output_variable_with_string_value() {
        // given
        let json_output_with_string_values = r#"
        {"foo": { "value": "bar", "sensitive": true }, "foo_2": {"value": "bar_2"} }
        "#;

        // when
        let hashmap =
            serialize_job_output(json_output_with_string_values.as_bytes(), "^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();

        // then
        assert_eq!(
            hashmap.get("foo").unwrap(),
            &JobOutputVariable {
                value: "bar".to_string(),
                sensitive: true,
                description: "".to_string(),
            }
        );
        assert_eq!(
            hashmap.get("foo_2").unwrap(),
            &JobOutputVariable {
                value: "bar_2".to_string(),
                sensitive: false,
                description: "".to_string(),
            }
        );
    }

    #[test]
    fn should_serialize_json_to_job_output_variable_with_numeric_value() {
        // given
        let json_output_with_numeric_values = r#"
        {"foo": { "value": 123, "sensitive": true }, "foo_2": {"value": 123.456} }
        "#;

        // when
        let hashmap =
            serialize_job_output(json_output_with_numeric_values.as_bytes(), "^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();

        // then
        assert_eq!(
            hashmap.get("foo").unwrap(),
            &JobOutputVariable {
                value: "123".to_string(),
                sensitive: true,
                description: "".to_string(),
            }
        );
        assert_eq!(
            hashmap.get("foo_2").unwrap(),
            &JobOutputVariable {
                value: "123.456".to_string(),
                sensitive: false,
                description: "".to_string(),
            }
        );
        let json_final = serde_json::to_string(&hashmap).unwrap();
        println!("{json_final}");
    }

    #[test]
    fn should_serialize_json_to_job_output_variable_with_description() {
        // given
        let json_output_with_numeric_values = r#"
        {"foo": { "value": 123, "description": "a description" }}
        "#;

        // when
        let hashmap =
            serialize_job_output(json_output_with_numeric_values.as_bytes(), "^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();

        // then
        assert_eq!(
            hashmap.get("foo").unwrap(),
            &JobOutputVariable {
                value: "123".to_string(),
                sensitive: false,
                description: "a description".to_string(),
            }
        );
        let json_final = serde_json::to_string(&hashmap).unwrap();
        println!("{json_final}");
    }

    #[test]
    fn should_fail_json_serialization_to_job_output_variable_when_invalid_name_pattern() {
        // given
        let json_output_with_numeric_values = r#"
        {"---": { "value": 123, "description": "a description" }}
        "#;

        // when
        let error = serialize_job_output(json_output_with_numeric_values.as_bytes(), "^[a-zA-Z_][a-zA-Z0-9_]*$")
            .err()
            .unwrap();

        // then
        match error {
            JobOutputSerializationError::SerializationError { serde_err } => {
                assert_eq!(serde_err.to_string(), "should not happen here");
            }
            JobOutputSerializationError::OutputVariableValidationError { err } => {
                assert_eq!(
                    err,
                    "Invalid job output variable name: '---'. It must match pattern: ^[a-zA-Z_][a-zA-Z0-9_]*$"
                        .to_string()
                )
            }
        }
    }
}
