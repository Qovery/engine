use crate::cmd::docker::Docker;
use crate::engine_task::qovery_api::QoveryApi;
use crate::events::{EventDetails, Transmitter};
use crate::io_models::QoveryIdentifier;
use crate::utilities::to_short_id;
use rand::Rng;
use rand::distr::Alphanumeric;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct Context {
    organization_id: Uuid,
    organization_short_id: String,
    cluster_id: Uuid,
    cluster_short_id: String,
    cluster_name: String,
    execution_id: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    test_cluster: bool,
    features: Vec<Features>,
    metadata: Option<Metadata>,
    pub docker: Arc<Docker>,
    pub qovery_api: Arc<dyn QoveryApi>,
    event_details: EventDetails,
}

impl Context {
    pub fn new(
        organization_id: Uuid,
        cluster_id: Uuid,
        execution_id: String,
        workspace_root_dir: String,
        lib_root_dir: String,
        test_cluster: bool,
        features: Vec<Features>,
        metadata: Option<Metadata>,
        docker: Arc<Docker>,
        qovery_api: Arc<dyn QoveryApi>,
        event_details: EventDetails,
    ) -> Self {
        Context {
            organization_id,
            organization_short_id: to_short_id(&organization_id),
            cluster_id,
            cluster_short_id: to_short_id(&cluster_id),
            cluster_name: QoveryIdentifier::new(cluster_id).qovery_resource_name().to_string(),
            execution_id,
            workspace_root_dir,
            lib_root_dir,
            test_cluster,
            features,
            metadata,
            docker,
            qovery_api,
            event_details,
        }
    }

    pub fn organization_short_id(&self) -> &str {
        &self.organization_short_id
    }

    pub fn organization_long_id(&self) -> &Uuid {
        &self.organization_id
    }

    pub fn cluster_long_id(&self) -> &Uuid {
        &self.cluster_id
    }

    pub fn cluster_short_id(&self) -> &str {
        &self.cluster_short_id
    }

    pub fn cluster_name(&self) -> &str {
        &self.cluster_name
    }

    pub fn execution_id(&self) -> &str {
        self.execution_id.as_str()
    }

    pub fn partial_execution_id(&self) -> &str {
        let delimiter = '-';
        let execution_id = self.execution_id();

        if let Some(pos) = execution_id.rfind(delimiter) {
            return &execution_id[..pos];
        }

        execution_id
    }

    pub fn workspace_root_dir(&self) -> &str {
        self.workspace_root_dir.as_str()
    }

    pub fn lib_root_dir(&self) -> &str {
        self.lib_root_dir.as_str()
    }

    pub fn metadata(&self) -> Option<&Metadata> {
        self.metadata.as_ref()
    }

    pub fn is_dry_run_deploy(&self) -> bool {
        match &self.metadata {
            Some(meta) => matches!(meta.dry_run_deploy, Some(true)),
            _ => false,
        }
    }

    pub fn requires_forced_upgrade(&self) -> bool {
        match &self.metadata {
            Some(meta) => matches!(meta.forced_upgrade, Some(true)),
            _ => false,
        }
    }

    pub fn is_test_cluster(&self) -> bool {
        self.test_cluster
    }

    pub fn resource_expiration_in_seconds(&self) -> Option<u32> {
        match &self.metadata {
            Some(meta) => meta.resource_expiration_in_seconds,
            _ => None,
        }
    }

    pub fn is_first_cluster_deployment(&self) -> bool {
        match &self.metadata {
            Some(meta) => meta.is_first_cluster_deployment.unwrap_or(false),
            _ => false,
        }
    }

    pub fn update_is_first_cluster_deployment(&mut self, is_first_cluster_deployment: bool) {
        if let Some(meta) = self.metadata.as_mut() {
            meta.update_is_first_cluster_deployment(is_first_cluster_deployment);
        }
    }

    // Qovery features
    pub fn is_feature_enabled(&self, name: &Features) -> bool {
        for feature in &self.features {
            if feature == name {
                return true;
            }
        }
        false
    }

    pub fn get_event_details(&self, transmitter: Transmitter) -> EventDetails {
        EventDetails::clone_changing_transmitter(self.event_details.clone(), transmitter)
    }
}

/// put everything you want here that is required to change the behaviour of the request.
/// E.g you can indicate that this request is a test, then you can adapt the behaviour as you want.
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Metadata {
    pub dry_run_deploy: Option<bool>,
    pub forced_upgrade: Option<bool>,
    pub resource_expiration_in_seconds: Option<u32>,
    pub is_first_cluster_deployment: Option<bool>,
}

impl Metadata {
    pub fn new(
        dry_run_deploy: Option<bool>,
        resource_expiration_in_seconds: Option<u32>,
        forced_upgrade: Option<bool>,
        is_first_cluster_deployment: Option<bool>,
    ) -> Self {
        Metadata {
            dry_run_deploy,
            resource_expiration_in_seconds,
            forced_upgrade,
            is_first_cluster_deployment,
        }
    }
    pub fn update_is_first_cluster_deployment(&mut self, is_first_cluster_deployment: bool) {
        self.is_first_cluster_deployment = Some(is_first_cluster_deployment)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, Eq, PartialEq)]
pub enum Features {
    LogsHistory,
    MetricsHistory,
    Grafana,
}

// trait used to reimplement clone without same fields
// this trait is used for Context struct
pub trait CloneForTest {
    fn clone_not_same_execution_id(&self) -> Self;
}

// for test we need to clone context but to change the directory workspace used
// to to this we just have to suffix the execution id in tests
impl CloneForTest for Context {
    fn clone_not_same_execution_id(&self) -> Context {
        let mut new = self.clone();
        let suffix = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(|e| e.to_string())
            .collect::<String>();
        new.execution_id = format!("{}-{}", self.execution_id, suffix);
        new
    }
}

#[cfg(test)]
mod tests {
    use crate::io_models::context::Metadata;

    #[test]
    /// Preventing empty / partially empty metadata input from triggering a deserialization error
    fn test_metadata_deserialization_empty_json() {
        // execute:
        let result: Metadata = serde_json::from_str("{}").expect("Error while trying to deserialize Metadata");

        // verify:
        assert_eq!(None, result.is_first_cluster_deployment);
        assert_eq!(None, result.resource_expiration_in_seconds);
        assert_eq!(None, result.forced_upgrade);
        assert_eq!(None, result.dry_run_deploy);
    }

    #[test]
    fn test_metadata_deserialization_is_first_cluster_deployment() {
        // setup:
        struct TestCase<'a> {
            input: &'a str,
            expected: Result<Option<bool>, ()>,
        }

        let test_cases = vec![
            TestCase {
                input: r#"{}"#,
                expected: Ok(None),
            },
            TestCase {
                input: r#"{"is_first_cluster_deployment": true}"#,
                expected: Ok(Some(true)),
            },
            TestCase {
                input: r#"{"is_first_cluster_deployment": false}"#,
                expected: Ok(Some(false)),
            },
            TestCase {
                input: r#"{"is_first_cluster_deployment": 1}"#,
                expected: Err(()),
            },
            TestCase {
                input: r#"{"is_first_cluster_deployment": 0}"#,
                expected: Err(()),
            },
            TestCase {
                input: r#"{"is_first_cluster_deployment": "true"}"#,
                expected: Err(()),
            },
            TestCase {
                input: r#"{"is_first_cluster_deployment": "false"}"#,
                expected: Err(()),
            },
            TestCase {
                input: r#"{"is_first_cluster_deployment": "bad"}"#,
                expected: Err(()),
            },
            TestCase {
                input: r#"{"is_first_cluster_deployment": -3}"#,
                expected: Err(()),
            },
        ];

        for tc in test_cases {
            // execute:
            let result_value: Result<Metadata, serde_json::Error> = serde_json::from_str(tc.input);

            // verify:
            assert_eq!(
                tc.expected,
                match result_value {
                    Ok(metadata) => Ok(metadata.is_first_cluster_deployment),
                    Err(_) => Err(()),
                }
            );
        }
    }
}
