// Sensitive data filter for terraform resources
// Redacts sensitive values from resource attributes

use std::collections::HashSet;

/// List of sensitive attribute name patterns that should be redacted
const SENSITIVE_PATTERNS: &[&str] = &[
    // Authentication
    "password",
    "passwd",
    "pwd",
    "secret",
    "secrets",
    "token",
    "tokens",
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "access_secret",
    "private_key",
    "privatekey",
    "private-key",
    "public_key",
    "publickey",
    "ssh_key",
    "sshkey",
    "ca_cert",
    "certificate",
    "cert",
    // AWS-specific
    "aws_access_key",
    "aws_secret",
    "secret_access_key",
    "secretaccesskey",
    "session_token",
    // Azure-specific
    "client_secret",
    "connection_string",
    "storage_account_key",
    "account_key",
    // GCP-specific
    "private_key_id",
    "project_id",
    "service_account_key",
    // Database
    "db_password",
    "database_password",
    "master_password",
    "master_user_password",
    "username",
    "user_password",
    "connection_string",
    // OAuth/OIDC
    "client_id",
    "client_secret",
    "oauth_token",
    "oauth_secret",
    "jwt",
    "bearer_token",
    // Keys and credentials
    "key",
    "credential",
    "credentials",
    "auth",
    "authorization",
    "bearer",
    "basic_auth",
    // Other common patterns
    "proxy_password",
    "db_user",
    "webhook_secret",
];

/// Filters sensitive data from terraform resource attributes
pub struct SensitiveFilter {
    patterns: HashSet<String>,
}

impl SensitiveFilter {
    pub fn new() -> Self {
        let patterns = SENSITIVE_PATTERNS.iter().map(|s| s.to_lowercase()).collect();

        SensitiveFilter { patterns }
    }

    /// Check if an attribute name matches a sensitive pattern
    pub fn is_sensitive(&self, attribute_name: &str) -> bool {
        let lower_name = attribute_name.to_lowercase();

        // Direct pattern match or substring match (e.g., "root_password" contains "password")
        self.patterns.iter().any(|pattern| lower_name.contains(pattern))
    }

    /// Recursively filter attributes from a serde_json Value
    pub fn filter_value(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut filtered = serde_json::Map::new();
                for (key, val) in map {
                    if !self.is_sensitive(key) {
                        filtered.insert(key.clone(), self.filter_value(val));
                    }
                }
                serde_json::Value::Object(filtered)
            }
            serde_json::Value::Array(arr) => {
                let filtered: Vec<_> = arr.iter().map(|v| self.filter_value(v)).collect();
                serde_json::Value::Array(filtered)
            }
            other => other.clone(),
        }
    }
}

impl Default for SensitiveFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_creation() {
        let filter = SensitiveFilter::new();
        assert!(!filter.patterns.is_empty());
    }

    #[test]
    fn test_detects_password() {
        let filter = SensitiveFilter::new();
        assert!(filter.is_sensitive("password"));
        assert!(filter.is_sensitive("db_password"));
        assert!(filter.is_sensitive("master_password"));
        assert!(filter.is_sensitive("root_password"));
    }

    #[test]
    fn test_detects_secret() {
        let filter = SensitiveFilter::new();
        assert!(filter.is_sensitive("secret"));
        assert!(filter.is_sensitive("api_secret"));
        assert!(filter.is_sensitive("aws_secret"));
    }

    #[test]
    fn test_detects_token() {
        let filter = SensitiveFilter::new();
        assert!(filter.is_sensitive("token"));
        assert!(filter.is_sensitive("access_token"));
        assert!(filter.is_sensitive("oauth_token"));
    }

    #[test]
    fn test_detects_api_key() {
        let filter = SensitiveFilter::new();
        assert!(filter.is_sensitive("api_key"));
        assert!(filter.is_sensitive("apikey"));
        assert!(filter.is_sensitive("api-key"));
    }

    #[test]
    fn test_detects_private_key() {
        let filter = SensitiveFilter::new();
        assert!(filter.is_sensitive("private_key"));
        assert!(filter.is_sensitive("ssh_key"));
    }

    #[test]
    fn test_case_insensitive() {
        let filter = SensitiveFilter::new();
        assert!(filter.is_sensitive("PASSWORD"));
        assert!(filter.is_sensitive("Password"));
        assert!(filter.is_sensitive("PaSsWoRd"));
    }

    #[test]
    fn test_does_not_match_unrelated() {
        let filter = SensitiveFilter::new();
        assert!(!filter.is_sensitive("id"));
        assert!(!filter.is_sensitive("instance_type"));
        assert!(!filter.is_sensitive("name"));
        assert!(!filter.is_sensitive("tags"));
    }

    #[test]
    fn test_filter_nested_object() {
        let filter = SensitiveFilter::new();
        let input = serde_json::json!({
            "id": "i-12345",
            "password": "secret123",
            "config": {
                "nested_password": "nested_secret",
                "other_field": "value"
            }
        });

        let filtered = filter.filter_value(&input);

        assert!(filtered.get("id").is_some());
        assert!(filtered.get("password").is_none());

        if let Some(config) = filtered.get("config").and_then(|v| v.as_object()) {
            assert!(config.get("nested_password").is_none());
            assert!(config.get("other_field").is_some());
        }
    }

    #[test]
    fn test_filter_array() {
        let filter = SensitiveFilter::new();
        let input = serde_json::json!([
            { "id": "1", "password": "secret" },
            { "id": "2", "password": "secret2" }
        ]);

        let filtered = filter.filter_value(&input);

        if let Some(arr) = filtered.as_array() {
            assert_eq!(arr.len(), 2);
            for item in arr {
                assert!(item.get("id").is_some());
                assert!(item.get("password").is_none());
            }
        }
    }
}
