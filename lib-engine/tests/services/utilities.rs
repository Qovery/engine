// Common utilities for service tests (Helm, Terraform, etc.)

use crate::helpers::aws::aws_infra_config;
use crate::helpers::azure::{azure_infra_config, clean_environments};
use crate::helpers::gcp::gcp_infra_config;
use crate::helpers::kubernetes::TargetCluster;
use crate::helpers::scaleway::scw_infra_config;
use crate::helpers::utilities::{FuncTestsSecrets, context_for_resource, logger, metrics_registry};
use crate::helpers::{self};
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use qovery_engine::io_models::annotations_group::{Annotation, AnnotationsGroup, AnnotationsGroupScope};
use qovery_engine::io_models::context::{CloneForTest, Context};
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::labels_group::{Label, LabelsGroup};
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;
use std::str::FromStr;
use tracing::log::warn;
use uuid::Uuid;

/// Cloud provider abstraction for tests
#[derive(Debug, Clone, Copy)]
pub enum CloudProvider {
    Aws,
    Scaleway,
    Gcp,
    Azure,
}

impl CloudProvider {
    /// Create context for the given cloud provider
    pub fn create_context(&self, secrets: &FuncTestsSecrets) -> Context {
        match self {
            CloudProvider::Aws => context_for_resource(
                secrets
                    .AWS_TEST_ORGANIZATION_LONG_ID
                    .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
                secrets
                    .AWS_TEST_CLUSTER_LONG_ID
                    .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
            ),
            CloudProvider::Scaleway => context_for_resource(
                secrets
                    .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                    .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
                secrets
                    .SCALEWAY_TEST_CLUSTER_LONG_ID
                    .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
            ),
            CloudProvider::Gcp => context_for_resource(
                secrets
                    .GCP_TEST_ORGANIZATION_LONG_ID
                    .expect("GCP_TEST_ORGANIZATION_LONG_ID is not set"),
                secrets
                    .GCP_TEST_CLUSTER_LONG_ID
                    .expect("GCP_TEST_CLUSTER_LONG_ID is not set"),
            ),
            CloudProvider::Azure => context_for_resource(
                secrets
                    .AZURE_TEST_ORGANIZATION_LONG_ID
                    .expect("AZURE_TEST_ORGANIZATION_LONG_ID is not set"),
                secrets
                    .AZURE_TEST_CLUSTER_LONG_ID
                    .expect("AZURE_TEST_CLUSTER_LONG_ID is not set"),
            ),
        }
    }

    /// Create target cluster for the given cloud provider
    pub fn create_target_cluster(&self, secrets: &FuncTestsSecrets) -> TargetCluster {
        let kubeconfig = match self {
            CloudProvider::Aws => secrets
                .AWS_TEST_KUBECONFIG_b64
                .as_ref()
                .expect("AWS_TEST_KUBECONFIG_b64 is not set"),
            CloudProvider::Scaleway => secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .as_ref()
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set"),
            CloudProvider::Gcp => secrets
                .GCP_TEST_KUBECONFIG_b64
                .as_ref()
                .expect("GCP_TEST_KUBECONFIG_b64 is not set"),
            CloudProvider::Azure => secrets
                .AZURE_TEST_KUBECONFIG_b64
                .as_ref()
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set"),
        };
        TargetCluster::MutualizedTestCluster {
            kubeconfig: kubeconfig.clone(),
        }
    }

    /// Create infrastructure context for the given cloud provider
    pub fn create_infra_context(
        &self,
        target_cluster: &TargetCluster,
        context: &Context,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
    ) -> InfrastructureContext {
        match self {
            CloudProvider::Aws => aws_infra_config(target_cluster, context, logger, metrics_registry),
            CloudProvider::Scaleway => scw_infra_config(target_cluster, context, logger, metrics_registry),
            CloudProvider::Gcp => gcp_infra_config(target_cluster, context, logger, metrics_registry),
            CloudProvider::Azure => azure_infra_config(target_cluster, context, logger, metrics_registry),
        }
    }

    /// Get the cloud provider kind
    pub fn kind(&self) -> Kind {
        match self {
            CloudProvider::Aws => Kind::Aws,
            CloudProvider::Scaleway => Kind::Scw,
            CloudProvider::Gcp => Kind::Gcp,
            CloudProvider::Azure => Kind::Azure,
        }
    }

    /// Get Azure region if cloud provider is Azure
    pub fn get_azure_region(&self, secrets: &FuncTestsSecrets) -> Option<AzureLocation> {
        match self {
            CloudProvider::Azure => Some(
                AzureLocation::from_str(
                    secrets
                        .AZURE_DEFAULT_REGION
                        .as_ref()
                        .expect("AZURE_DEFAULT_REGION is not set")
                        .as_str(),
                )
                .expect("Unknown Azure region"),
            ),
            _ => None,
        }
    }
}

/// Test infrastructure setup
pub struct TestInfra {
    pub context: Context,
    pub target_cluster: TargetCluster,
    pub infra_ctx: InfrastructureContext,
    pub infra_ctx_for_delete: InfrastructureContext,
    pub cloud_provider: CloudProvider,
    pub secrets: FuncTestsSecrets,
    pub logger: Box<dyn Logger>,
    pub metrics_registry: Box<dyn MetricsRegistry>,
}

impl TestInfra {
    /// Create a new test infrastructure for the given cloud provider
    pub fn new(cloud_provider: CloudProvider) -> Self {
        let logger_instance = logger();
        let metrics_registry_instance = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = cloud_provider.create_context(&secrets);
        let target_cluster = cloud_provider.create_target_cluster(&secrets);
        let infra_ctx = cloud_provider.create_infra_context(
            &target_cluster,
            &context,
            logger_instance.clone(),
            metrics_registry_instance.clone(),
        );
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = cloud_provider.create_infra_context(
            &target_cluster,
            &context_for_delete,
            logger_instance.clone(),
            metrics_registry_instance.clone(),
        );

        Self {
            context,
            target_cluster,
            infra_ctx,
            infra_ctx_for_delete,
            cloud_provider,
            secrets,
            logger: logger_instance,
            metrics_registry: metrics_registry_instance,
        }
    }

    /// Create a minimal working environment for testing
    pub fn create_environment(&self) -> EnvironmentRequest {
        let mut env = helpers::environment::working_minimal_environment(&self.context);
        env.applications = vec![];
        env
    }

    /// Create a resume context for pause/resume operations
    pub fn create_resume_context(&self) -> InfrastructureContext {
        let ctx_resume = self.context.clone_not_same_execution_id();
        self.cloud_provider.create_infra_context(
            &self.target_cluster,
            &ctx_resume,
            self.logger.clone(),
            self.metrics_registry.clone(),
        )
    }

    /// Cleanup environments (Azure specific)
    pub fn cleanup(&self, environment: EnvironmentRequest) {
        if let Some(region) = self.cloud_provider.get_azure_region(&self.secrets)
            && let Err(e) = clean_environments(&self.context, vec![environment], region)
        {
            warn!("cannot clean environments, error: {e:?}");
        }
    }
}

/// Creates default test annotations
pub fn create_default_test_annotations() -> (Uuid, AnnotationsGroup) {
    let id = Uuid::new_v4();
    let group = AnnotationsGroup {
        annotations: vec![
            Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            },
            Annotation {
                key: "annot_key2".to_string(),
                value: "true".to_string(),
            },
        ],
        scopes: vec![
            AnnotationsGroupScope::Jobs,
            AnnotationsGroupScope::Pods,
            AnnotationsGroupScope::Secrets,
        ],
    };
    (id, group)
}

/// Creates default test labels
pub fn create_default_test_labels() -> (Uuid, LabelsGroup) {
    let id = Uuid::new_v4();
    let group = LabelsGroup {
        labels: vec![Label {
            key: "label_key".to_string(),
            value: "label_value".to_string(),
            propagate_to_cloud_provider: false,
        }],
    };
    (id, group)
}
