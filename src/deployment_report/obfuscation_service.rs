use regex::Regex;
use std::borrow::Cow;

pub trait ObfuscationService: Send + Sync {
    fn obfuscate_secrets(&self, text: String) -> String;

    fn clone_dyn(&self) -> Box<dyn ObfuscationService>;

    fn with_secrets(&self, secrets: Vec<String>) -> Box<dyn ObfuscationService>;
}

pub struct StdObfuscationService {
    regex: Option<Regex>,
}

impl StdObfuscationService {
    pub fn new(secrets: Vec<String>) -> Self {
        let regex = Self::create_regex(secrets);

        StdObfuscationService { regex }
    }

    fn create_regex(secrets: Vec<String>) -> Option<Regex> {
        if secrets.is_empty() {
            None
        } else {
            let secret_regex = secrets.join("|");

            Some(Regex::new(&secret_regex).unwrap())
        }
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
            match regex.replace_all(&text, "xxx") {
                Cow::Borrowed(_) => {}
                Cow::Owned(obfuscate) => return obfuscate,
            }
            text
        } else {
            text
        }
    }

    fn clone_dyn(&self) -> Box<dyn ObfuscationService> {
        Box::new(StdObfuscationService {
            regex: self.regex.clone(),
        })
    }

    fn with_secrets(&self, secrets: Vec<String>) -> Box<dyn ObfuscationService> {
        let regex = Self::create_regex(secrets);
        Box::new(StdObfuscationService { regex })
    }
}

#[cfg(test)]
mod tests {
    use crate::deployment_report::obfuscation_service::{ObfuscationService, StdObfuscationService};

    #[test]
    fn test_obfuscate_logs_without_secrets_defined() {
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
            obfuscation_service.obfuscate_secrets(log),
            "a log xxx my pxxxord: xxx".to_string()
        );
    }

    #[test]
    fn test_obfuscate_logs_with_secret_with_special() {
        let log = "a log with my password: /1234-a/bcd".to_string();
        let obfuscation_service = StdObfuscationService::new(vec![
            "12".to_string(),
            "with".to_string(),
            "/1234-a/bcd".to_string(),
            "12".to_string(),
        ]);

        assert_eq!(
            obfuscation_service.obfuscate_secrets(log),
            "a log xxx my password: xxx".to_string()
        );
    }

    #[test]
    fn test_obfuscate_logs_without_secret() {
        let log = "no secret in this log".to_string();
        let obfuscation_service =
            StdObfuscationService::new(vec!["with".to_string(), "1234-abcd".to_string(), "assw".to_string()]);

        assert_eq!(obfuscation_service.obfuscate_secrets(log.clone()), log);
    }
}
