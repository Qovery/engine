use crate::io_models::application::GitCredentials;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
pub struct BlueprintRequest {
    pub execution_id: String,
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub project_long_id: Uuid,
    pub organization_long_id: Uuid,
    #[serde(default = "default_max_parallel_build")]
    pub max_parallel_build: u32,
    #[serde(default = "default_max_parallel_deploy")]
    pub max_parallel_deploy: u32,
    pub variables: Vec<BlueprintVariable>,
    pub git_url: String,
    // git tag to checkout (e.g. "aws/s3/1/1.0.0")
    // Also used to get blueprint files path e.g. myrepo/aws/s3/1 <- contains blueprint files
    pub tag: String,

    // Git credentials for private catalog repos. Sent by q-core.
    // When None, the engine attempts an unauthenticated clone (public repos only).
    #[serde(default)]
    pub git_credentials: Option<GitCredentials>,

    #[serde(default)]
    pub spec_overrides: Option<BlueprintSpecOverrides>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub struct BlueprintVariable {
    pub name: String,
    pub value: String,
    pub is_secret: bool,
}

// Generic map of spec field overrides sent by q-core when the user overrides
// fields marked `overridable: true` in the QBM.
//
// The engine applies these with highest precedence:
//   spec_overrides > qbm.yml spec > hardcoded platform default
//
// Examples:
//   { "credentials": "env" }
//   { "timeout": 7200 }
//   { "resources": { "cpu": "1000m", "ram": "2Gi" } }
pub type BlueprintSpecOverrides = HashMap<String, serde_json::Value>;

fn default_max_parallel_build() -> u32 {
    1u32
}

fn default_max_parallel_deploy() -> u32 {
    1u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_request() {
        let json = r#"{
            "execution_id": "exec-1",
            "long_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "name": "my-blueprint",
            "kube_name": "my-blueprint",
            "project_long_id": "11111111-2222-3333-4444-555555555555",
            "organization_long_id": "22222222-3333-4444-5555-666666666666",
            "variables": [
                { "name": "region", "value": "eu-west-3", "is_secret": false },
                { "name": "password", "value": "s3cret", "is_secret": true }
            ],
            "git_url": "https://github.com/org/catalog.git",
            "tag": "aws/postgres/16/1.0.0"
        }"#;
        let req: BlueprintRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tag, "aws/postgres/16/1.0.0");
        assert_eq!(req.git_url, "https://github.com/org/catalog.git");
        assert_eq!(req.variables.len(), 2);
        assert!(!req.variables[0].is_secret);
        assert!(req.variables[1].is_secret);
        assert_eq!(req.variables[1].value, "s3cret");
        // Defaults
        assert!(req.git_credentials.is_none());
        assert!(req.spec_overrides.is_none());
        assert_eq!(req.max_parallel_build, 1);
        assert_eq!(req.max_parallel_deploy, 1);
    }

    #[test]
    fn deserialize_with_spec_overrides() {
        let json = r#"{
            "execution_id": "exec-2",
            "long_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "name": "test",
            "kube_name": "test",
            "project_long_id": "11111111-2222-3333-4444-555555555555",
            "organization_long_id": "22222222-3333-4444-5555-666666666666",
            "variables": [],
            "git_url": "https://github.com/org/catalog.git",
            "tag": "helm/redis/7/1.0.0",
            "spec_overrides": {
                "credentials": "env",
                "timeout": 7200,
                "resources": { "cpu": "2000m" }
            }
        }"#;
        let req: BlueprintRequest = serde_json::from_str(json).unwrap();
        let overrides = req.spec_overrides.unwrap();
        assert_eq!(overrides.len(), 3);
        assert_eq!(overrides["credentials"], "env");
        assert_eq!(overrides["timeout"], 7200);
        assert_eq!(overrides["resources"]["cpu"], "2000m");
    }

    #[test]
    fn deserialize_ignores_unknown_fields() {
        let json = r#"{
            "execution_id": "exec-3",
            "long_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "name": "test",
            "kube_name": "test",
            "project_long_id": "11111111-2222-3333-4444-555555555555",
            "organization_long_id": "22222222-3333-4444-5555-666666666666",
            "variables": [],
            "git_url": "https://github.com/org/catalog.git",
            "tag": "aws/s3/1/1.0.0",
            "provider": "aws",
            "service_name": "s3",
            "service_version": "1",
            "some_future_field": "value"
        }"#;
        let req: BlueprintRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tag, "aws/s3/1/1.0.0");
    }

    #[test]
    fn blueprint_variable_equality() {
        let v1 = BlueprintVariable {
            name: "region".into(),
            value: "eu-west-3".into(),
            is_secret: false,
        };
        let v2 = v1.clone();
        assert_eq!(v1, v2);

        let v3 = BlueprintVariable {
            name: "region".into(),
            value: "us-east-1".into(),
            is_secret: false,
        };
        assert_ne!(v1, v3);
    }

    #[test]
    fn blueprint_variable_hash_consistency() {
        use std::collections::HashSet;

        let v1 = BlueprintVariable {
            name: "db_password".into(),
            value: "secret123".into(),
            is_secret: true,
        };
        let v2 = v1.clone();

        let mut set = HashSet::new();
        set.insert(v1);
        // Inserting an equal clone should not increase the set size
        set.insert(v2);
        assert_eq!(set.len(), 1);
    }
}
