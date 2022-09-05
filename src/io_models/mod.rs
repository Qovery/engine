use crate::cloud_provider::service;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use uuid::Uuid;

pub mod application;
pub mod container;
pub mod context;
pub mod database;
pub mod domain;
pub mod environment;
pub mod progress_listener;
pub mod router;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QoveryIdentifier {
    raw_long_id: String,
    short: String,
}

impl QoveryIdentifier {
    pub fn new(raw_long_id: String, raw_short_id: String) -> Self {
        QoveryIdentifier {
            raw_long_id,
            short: raw_short_id,
        }
    }

    pub fn new_from_long_id(raw_long_id: String) -> Self {
        QoveryIdentifier::new(raw_long_id.to_string(), QoveryIdentifier::extract_short(raw_long_id.as_str()))
    }

    pub fn new_random() -> Self {
        Self::new_from_long_id(Uuid::new_v4().to_string())
    }

    fn extract_short(raw: &str) -> String {
        let max_execution_id_chars: usize = 8;
        match raw.char_indices().nth(max_execution_id_chars - 1) {
            None => raw.to_string(),
            Some((_, _)) => raw[..max_execution_id_chars].to_string(),
        }
    }

    pub fn short(&self) -> &str {
        &self.short
    }
}

impl Default for QoveryIdentifier {
    fn default() -> Self {
        QoveryIdentifier::new_from_long_id(Uuid::default().to_string())
    }
}

impl From<String> for QoveryIdentifier {
    fn from(s: String) -> Self {
        QoveryIdentifier::new_from_long_id(s)
    }
}

impl Display for QoveryIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.raw_long_id.as_str())
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Action {
    Create,
    Pause,
    Delete,
    Nothing,
}

impl Action {
    pub fn to_service_action(&self) -> service::Action {
        match self {
            Action::Create => service::Action::Create,
            Action::Pause => service::Action::Pause,
            Action::Delete => service::Action::Delete,
            Action::Nothing => service::Action::Nothing,
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::io_models::QoveryIdentifier;

    #[test]
    fn test_qovery_identifier_new_from_long_id() {
        struct TestCase<'a> {
            input: String,
            expected_long_id_output: String,
            expected_short_output: String,
            description: &'a str,
        }

        // setup:
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input: "".to_string(),
                expected_long_id_output: "".to_string(),
                expected_short_output: "".to_string(),
                description: "empty raw long ID input",
            },
            TestCase {
                input: "2a365285-992f-4285-ab96-c55ac81ecde9".to_string(),
                expected_long_id_output: "2a365285-992f-4285-ab96-c55ac81ecde9".to_string(),
                expected_short_output: "2a365285".to_string(),
                description: "proper Uuid input",
            },
            TestCase {
                input: "2a365285".to_string(),
                expected_long_id_output: "2a365285".to_string(),
                expected_short_output: "2a365285".to_string(),
                description: "non standard Uuid input, length 8",
            },
            TestCase {
                input: "2a365285hebnrfvuebr".to_string(),
                expected_long_id_output: "2a365285hebnrfvuebr".to_string(),
                expected_short_output: "2a365285".to_string(),
                description: "non standard Uuid input, length longer than expected short (length 8)",
            },
            TestCase {
                input: "2a365".to_string(),
                expected_long_id_output: "2a365".to_string(),
                expected_short_output: "2a365".to_string(),
                description: "non standard Uuid input, length shorter than expected short (length 8)",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = QoveryIdentifier::new_from_long_id(tc.input.clone());

            // verify:
            assert_eq!(
                tc.expected_long_id_output, result.raw_long_id,
                "case {} : '{}'",
                tc.description, tc.input
            );
            assert_eq!(
                tc.expected_short_output, result.short,
                "case {} : '{}'",
                tc.description, tc.input
            );
        }
    }
}
