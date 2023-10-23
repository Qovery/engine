use regex::Regex;

pub trait ObfuscationService: Send + Sync {
    fn obfuscate_secrets(&self, text: String) -> String;
}

#[derive(Clone)]
pub struct StdObfuscationService {
    regex: Option<Regex>,
}

impl StdObfuscationService {
    pub fn new(secrets: Vec<String>) -> Self {
        let regex = if secrets.is_empty() {
            None
        } else {
            let secret_regex = secrets
                .iter()
                .map(|secret| format!("\\b{}\\b", regex::escape(secret)))
                .collect::<Vec<String>>()
                .join("|");

            Some(Regex::new(&secret_regex).unwrap())
        };

        StdObfuscationService { regex }
    }
}

impl Default for StdObfuscationService {
    fn default() -> Self {
        Self::new(vec![])
    }
}

impl ObfuscationService for StdObfuscationService {
    fn obfuscate_secrets(&self, text: String) -> String {
        if let Some(regex) = &self.regex {
            regex.replace_all(&text, "xxx").to_string()
        } else {
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::deployment_report::obfuscation_service::{ObfuscationService, StdObfuscationService};

    #[test]
    fn test_obfuscate_logs_without_secret() {
        let log = "a log with my password: 1234-abcd".to_string();
        let obfuscation_service = StdObfuscationService::new(vec![]);

        assert_eq!(obfuscation_service.obfuscate_secrets(log.clone()), log);
    }

    #[test]
    fn test_obfuscate_logs_with_secret() {
        let log = "a log with my password: 1234-abcd".to_string();
        let obfuscation_service =
            StdObfuscationService::new(vec!["with".to_string(), "1234-abcd".to_string(), "assw".to_string()]);

        assert_eq!(
            obfuscation_service.obfuscate_secrets(log.to_owned()),
            "a log xxx my password: xxx".to_string()
        );
    }
}
