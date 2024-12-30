use itertools::Itertools;
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
            return None;
        }

        let secret_regex = secrets
            .iter()
            .filter(|&secret| !secret.trim().is_empty())
            .map(|secret| regex::escape(secret))
            .collect_vec()
            .join("|");

        if let Ok(regex) = Regex::new(&secret_regex) {
            Some(regex)
        } else {
            error!("Can't create regex from {}", secret_regex);
            None
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
            if let Cow::Owned(obfuscated) = regex.replace_all(&text, "xxx") {
                return obfuscated;
            }
        }
        text
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
    use crate::environment::report::obfuscation_service::{ObfuscationService, StdObfuscationService};

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
            "".to_string(),
            " ".to_string(),
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

    #[test]
    fn test_service_creation_from_gcp_credential() {
        let secret = "{
    \"type\": \"service_account\",
    \"client_email\": \"qovery@qovery-gcp-tests.iam.gserviceaccount.com\",
    \"client_id\": \"33453TRGRG\",
    \"private_key_id\": \"12234E4T4T\",
    \"private_key\": \"-----BEGIN PRIVATE KEY-----\naaaaa\na+y+a\na+bbbbbb\n-----END PRIVATE KEY-----\n\",
    \"auth_uri\": \"https://accounts.google.com/o/oauth2/auth\",
    \"token_uri\": \"https://oauth2.googleapis.com/token\",
    \"auth_provider_x509_cert_url\": \"https://www.googleapis.com/oauth2/v1/certs\",
    \"client_x509_cert_url\": \"https://www.googleapis.com.com\",
    \"project_id\": \"qovery-gcp-tests\",
    \"universe_domain\": \"googleapis.com\"
}";
        let obfuscation_service = StdObfuscationService::new(vec![secret.to_string()]);

        assert!(obfuscation_service.regex.is_some());
        assert_eq!(obfuscation_service.obfuscate_secrets(secret.to_string()), "xxx");
    }
}
