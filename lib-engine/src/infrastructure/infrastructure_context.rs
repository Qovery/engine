use std::borrow::Borrow;
use std::path::PathBuf;
use std::sync::Mutex;
use thiserror::Error;
use uuid::Uuid;

use crate::engine_task::qovery_api::SharedClusterFailureContext;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::models::build_platform::BuildPlatform;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::container_registry::ContainerRegistry;
use crate::infrastructure::models::dns_provider::DnsProvider;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::context::Context;
use crate::metrics_registry::MetricsRegistry;
use crate::services::{kube_client::QubeClient, kubernetes_api_deprecation_service::KubernetesApiDeprecationService};

#[derive(Error, Debug, PartialEq, Eq)]
pub enum EngineConfigError {
    #[error("Build platform is not valid error: {0}")]
    BuildPlatformNotValid(EngineError),
    #[error("Cloud provider is not valid error: {0}")]
    CloudProviderNotValid(EngineError),
    #[error("DNS provider is not valid error: {0}")]
    DnsProviderNotValid(EngineError),
    #[error("Kubernetes is not valid error: {0}")]
    KubernetesNotValid(EngineError),
}

/// Selects which Kubernetes authentication sources a caller explicitly allows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KubeClientAuthMode {
    /// Preserve the infrastructure lifecycle guard: an infrastructure deployment requires a kubeconfig.
    RequireKubeconfigForInfrastructure,
    /// Prefer an existing kubeconfig, but allow the pod ServiceAccount when no file exists.
    AllowInCluster,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KubeClientAuthSource {
    Kubeconfig,
    InCluster,
}

impl KubeClientAuthSource {
    fn is_allowed(self, auth_mode: KubeClientAuthMode, is_infra_deployment: bool) -> bool {
        self == Self::Kubeconfig || !is_infra_deployment || auth_mode == KubeClientAuthMode::AllowInCluster
    }
}

struct CachedKubeClient {
    client: QubeClient,
    auth_source: KubeClientAuthSource,
}

#[derive(Debug, Eq, PartialEq)]
struct KubeconfigRequired;

impl EngineConfigError {
    pub fn engine_error(&self) -> &EngineError {
        match self {
            EngineConfigError::BuildPlatformNotValid(e) => e,
            EngineConfigError::CloudProviderNotValid(e) => e,
            EngineConfigError::DnsProviderNotValid(e) => e,
            EngineConfigError::KubernetesNotValid(e) => e,
        }
    }
}

pub struct InfrastructureContext {
    context: Context,
    build_platform: Box<dyn BuildPlatform>,
    container_registry: ContainerRegistry,
    cloud_provider: Box<dyn CloudProvider>,
    dns_provider: Box<dyn DnsProvider>,
    kubernetes: Box<dyn Kubernetes>,
    metrics_registry: Box<dyn MetricsRegistry>,
    is_infra_deployment: bool,
    kube_client: Mutex<Option<CachedKubeClient>>,
    kubernetes_api_deprecation_service: KubernetesApiDeprecationService,
    pub cluster_failure_context: SharedClusterFailureContext,
}

impl InfrastructureContext {
    pub fn new(
        context: Context,
        build_platform: Box<dyn BuildPlatform>,
        container_registry: ContainerRegistry,
        cloud_provider: Box<dyn CloudProvider>,
        dns_provider: Box<dyn DnsProvider>,
        kubernetes: Box<dyn Kubernetes>,
        metrics_registry: Box<dyn MetricsRegistry>,
        is_infra_deployment: bool,
    ) -> InfrastructureContext {
        let cluster_id = *context.cluster_long_id();
        let deployment_infrastructure_id = extract_deployment_infrastructure_id(context.execution_id());
        InfrastructureContext {
            context,
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
            metrics_registry,
            is_infra_deployment,
            kube_client: Mutex::new(None),
            kubernetes_api_deprecation_service: KubernetesApiDeprecationService::default(),
            cluster_failure_context: SharedClusterFailureContext::new(cluster_id, deployment_infrastructure_id),
        }
    }

    pub fn kubernetes(&self) -> &dyn Kubernetes {
        self.kubernetes.as_ref()
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    pub fn build_platform(&self) -> &dyn BuildPlatform {
        self.build_platform.borrow()
    }

    pub fn container_registry(&self) -> &ContainerRegistry {
        self.container_registry.borrow()
    }

    pub fn cloud_provider(&self) -> &dyn CloudProvider {
        (*self.cloud_provider).borrow()
    }

    pub fn dns_provider(&self) -> &dyn DnsProvider {
        (*self.dns_provider).borrow()
    }

    pub fn metrics_registry(&self) -> &dyn MetricsRegistry {
        self.metrics_registry.borrow()
    }

    pub fn is_valid(&self) -> Result<(), Box<EngineConfigError>> {
        if let Err(e) = self.dns_provider.is_valid() {
            return Err(Box::new(EngineConfigError::DnsProviderNotValid(
                e.to_engine_error(self.dns_provider.event_details()),
            )));
        }

        Ok(())
    }

    // The kubeconfig file may not exist yet on disk, so we create the client lazily
    pub fn mk_kube_client(&self) -> Result<QubeClient, Box<EngineError>> {
        self.mk_kube_client_with_auth_mode(KubeClientAuthMode::RequireKubeconfigForInfrastructure)
    }

    /// Creates a Kubernetes client using the authentication sources explicitly allowed by the caller.
    pub fn mk_kube_client_with_auth_mode(&self, auth_mode: KubeClientAuthMode) -> Result<QubeClient, Box<EngineError>> {
        if let Some(cached_client) = self.kube_client.lock().unwrap().borrow().as_ref()
            && cached_client
                .auth_source
                .is_allowed(auth_mode, self.is_infra_deployment)
        {
            return Ok(cached_client.client.clone());
        }

        let event_details = self
            .kubernetes()
            .get_event_details(Infrastructure(InfrastructureStep::RetrieveClusterResources));

        let kubeconfig_path = select_kubeconfig_path(
            self.kubernetes().kubeconfig_local_file_path(),
            self.is_infra_deployment,
            auth_mode,
        )
        .map_err(|_| {
            // Infrastructure lifecycle operations must not silently switch to the credentials of
            // the pod they may be upgrading. Only explicitly authorized callers bypass this guard.
            Box::new(EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(
                event_details.clone(),
            ))
        })?;

        let kube_credentials: Vec<(String, String)> = self
            .cloud_provider
            .credentials_environment_variables()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let auth_source = if kubeconfig_path.is_some() {
            KubeClientAuthSource::Kubeconfig
        } else {
            KubeClientAuthSource::InCluster
        };
        let client = QubeClient::new(event_details, kubeconfig_path, kube_credentials)?;

        *self.kube_client.lock().unwrap() = Some(CachedKubeClient {
            client: client.clone(),
            auth_source,
        });
        Ok(client)
    }

    pub fn kubernetes_api_deprecation_service(&self) -> &KubernetesApiDeprecationService {
        &self.kubernetes_api_deprecation_service
    }
}

fn select_kubeconfig_path(
    kubeconfig_path: PathBuf,
    is_infra_deployment: bool,
    auth_mode: KubeClientAuthMode,
) -> Result<Option<PathBuf>, KubeconfigRequired> {
    if kubeconfig_path.exists() {
        return Ok(Some(kubeconfig_path));
    }
    if is_infra_deployment && auth_mode == KubeClientAuthMode::RequireKubeconfigForInfrastructure {
        return Err(KubeconfigRequired);
    }
    Ok(None)
}

pub fn extract_deployment_infrastructure_id(execution_id: &str) -> Option<Uuid> {
    execution_id
        .rfind('-')
        .and_then(|pos| Uuid::parse_str(&execution_id[..pos]).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_cluster_auth_must_be_explicit_for_infrastructure_deployments() {
        let missing_path = std::env::temp_dir().join(format!("missing-kubeconfig-{}", Uuid::new_v4()));

        assert_eq!(
            select_kubeconfig_path(
                missing_path.clone(),
                true,
                KubeClientAuthMode::RequireKubeconfigForInfrastructure,
            ),
            Err(KubeconfigRequired)
        );
        assert_eq!(
            select_kubeconfig_path(missing_path, true, KubeClientAuthMode::AllowInCluster),
            Ok(None)
        );
    }

    #[test]
    fn explicit_in_cluster_auth_still_prefers_an_existing_kubeconfig() {
        let kubeconfig = tempfile::NamedTempFile::new().unwrap();
        let kubeconfig_path = kubeconfig.path().to_path_buf();

        assert_eq!(
            select_kubeconfig_path(kubeconfig_path.clone(), true, KubeClientAuthMode::AllowInCluster),
            Ok(Some(kubeconfig_path))
        );
    }

    #[test]
    fn cached_in_cluster_auth_cannot_bypass_the_infrastructure_guard() {
        assert!(
            !KubeClientAuthSource::InCluster.is_allowed(KubeClientAuthMode::RequireKubeconfigForInfrastructure, true)
        );
        assert!(KubeClientAuthSource::InCluster.is_allowed(KubeClientAuthMode::AllowInCluster, true));
        assert!(
            KubeClientAuthSource::InCluster.is_allowed(KubeClientAuthMode::RequireKubeconfigForInfrastructure, false)
        );
        assert!(
            KubeClientAuthSource::Kubeconfig.is_allowed(KubeClientAuthMode::RequireKubeconfigForInfrastructure, true)
        );
    }
}
