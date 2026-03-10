use crate::cmd::docker;
use crate::errors::EngineError;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::kubernetes::{Kind, Kubernetes, KubernetesVersion, event_details};
use crate::io_models::application::GitCredentials;
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::metrics::MetricsParameters;
use crate::io_models::models::CpuArchitecture;
use crate::io_models::models::CpuArchitecture::{AMD64, ARM64};
use crate::logger::Logger;
use crate::utilities::to_short_id;
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use uuid::Uuid;

pub struct EksAnywhere {
    pub context: Context,
    pub id: String,
    pub kind: Kind,
    pub long_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub name: String,
    pub version: KubernetesVersion,
    pub region: String,
    pub template_directory: PathBuf,
    pub options: EksAnywhereOptions,
    pub logger: Box<dyn Logger>,
    pub advanced_settings: ClusterAdvancedSettings,
    pub kubeconfig: String,
    pub temp_dir: PathBuf,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
}

impl EksAnywhere {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: String,
        cloud_provider: &dyn CloudProvider,
        kind: Kind,
        region: String,
        version: KubernetesVersion,
        options: EksAnywhereOptions,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        kubeconfig: String,
        temp_dir: PathBuf,
    ) -> Result<EksAnywhere, Box<EngineError>> {
        let event_details = event_details(cloud_provider, long_id, name.to_string(), &context);
        let template_directory = PathBuf::from(format!("{}/eksanywhere/bootstrap", context.lib_root_dir()));

        let cluster = EksAnywhere {
            context,
            id: to_short_id(&long_id),
            kind,
            long_id,
            created_at: Default::default(),
            name,
            version,
            region,
            template_directory,
            options,
            logger,
            advanced_settings,
            kubeconfig,
            temp_dir,
            customer_helm_charts_override: None,
        };

        // make sure to write kubeconfig file
        write_kubeconfig_on_disk(&cluster.kubeconfig_local_file_path(), &cluster.kubeconfig, event_details)?;

        Ok(cluster)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EksAnywhereOptions {
    pub qovery_grpc_url: String,
    pub qovery_engine_url: String,
    pub metrics_parameters: Option<MetricsParameters>,
    pub jwt_token: String,
    pub infrastructure_charts_parameters: InfrastructureChartsParameters,
    pub tls_email_report: String,
}

impl EksAnywhereOptions {
    pub fn new(
        qovery_grpc_url: String,
        qovery_engine_url: String,
        metrics_parameters: Option<MetricsParameters>,
        jwt_token: String,
        infrastructure_charts_parameters: InfrastructureChartsParameters,
        tls_email_report: String,
    ) -> Self {
        EksAnywhereOptions {
            qovery_grpc_url,
            qovery_engine_url,
            metrics_parameters,
            jwt_token,
            infrastructure_charts_parameters,
            tls_email_report,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InfrastructureChartsParameters {
    pub metal_lb_parameters: MetalLbChartOverrides,
    pub nginx_parameters: NginxChartOverrides,
    pub cert_manager_parameters: CertManagerParameters,
    #[serde(default)]
    pub eks_anywhere_parameters: Option<EksAnywhereParameters>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EksAnywhereParameters {
    #[serde(default)]
    pub git_repository: Option<EksAnywhereGitRepository>,
    #[serde(default)]
    pub yaml_file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EksAnywhereGitRepository {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    pub branch: String,
    #[serde(alias = "rootPath")]
    pub root_path: String,
    #[serde(default, alias = "gitTokenId")]
    pub git_token_id: Option<Uuid>,
    #[serde(default)]
    pub git_credentials: Option<GitCredentials>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CertManagerParameters {
    pub kubernetes_namespace: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MetalLbChartOverrides {
    pub ip_address_pools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NginxChartOverrides {
    // Because as we have configured metallb as L2 and network traffic to local, only 1 instance of nginx can receive traffic.
    pub replica_count: u8,
    // As cert-manager is already expected to be installed, indicate the default ssl certificate
    pub default_ssl_certificate: String,
    // We must override ingress external IP, as our LB ip pool range is NATed from this public IP
    pub publish_status_address: String,
    // Specify the IP we want for the LB, to allow them to configure nats
    pub annotation_metal_lb_load_balancer_ips: String,
    // Override external dns with the public IP
    pub annotation_external_dns_kubernetes_target: String,
}

impl Kubernetes for EksAnywhere {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        self.kind
    }

    fn short_id(&self) -> &str {
        self.id.as_str()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> KubernetesVersion {
        self.version.clone()
    }

    fn region(&self) -> &str {
        self.region.as_str()
    }

    fn zones(&self) -> Option<Vec<&str>> {
        None
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn is_network_managed_by_user(&self) -> bool {
        true
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        // We take what is configured by the engine, if nothing is configured we default to amd64
        info!("BUILDER_CPU_ARCHITECTURES: {:?}", env::var("BUILDER_CPU_ARCHITECTURES"));
        let archs: Vec<CpuArchitecture> = env::var("BUILDER_CPU_ARCHITECTURES")
            .unwrap_or_default()
            .split(',')
            .filter_map(|x| docker::Architecture::from_str(x).ok())
            .map(|x| match x {
                docker::Architecture::AMD64 => AMD64,
                docker::Architecture::ARM64 => ARM64,
            })
            .collect();
        info!("BUILDER_CPU_ARCHITECTURES: {:?}", archs);

        if archs.is_empty() { vec![AMD64] } else { archs }
    }

    fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn loadbalancer_l4_annotations(&self, _cloud_provider_lb_name: Option<&str>) -> Vec<(String, String)> {
        Vec::with_capacity(0)
    }

    fn as_infra_actions(&self) -> &dyn InfrastructureAction {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn base_eks_anywhere_options() -> serde_json::Value {
        json!({
            "qovery_grpc_url": "https://grpc.qovery.test",
            "qovery_engine_url": "https://engine.qovery.test",
            "metrics_parameters": null,
            "jwt_token": "jwt-token",
            "infrastructure_charts_parameters": {
                "metal_lb_parameters": {
                    "ip_address_pools": ["10.0.0.1-10.0.0.10"]
                },
                "nginx_parameters": {
                    "replica_count": 1,
                    "default_ssl_certificate": "qovery/default-cert",
                    "publish_status_address": "1.1.1.1",
                    "annotation_metal_lb_load_balancer_ips": "10.0.0.2",
                    "annotation_external_dns_kubernetes_target": "1.1.1.1"
                },
                "cert_manager_parameters": {
                    "kubernetes_namespace": "cert-manager"
                }
            },
            "tls_email_report": "ops@qovery.com"
        })
    }

    #[test]
    fn should_parse_eks_anywhere_parameters_from_infrastructure_charts_parameters() {
        let mut payload = base_eks_anywhere_options();
        payload["infrastructure_charts_parameters"]["eks_anywhere_parameters"] = json!({
            "git_repository": {
                "url": "https://bitbucket.com/workspace/cluster.git",
                "provider": "BITBUCKET",
                "branch": "main",
                "root_path": "/",
                "git_token_id": Uuid::new_v4().to_string(),
                "git_credentials": {
                    "login": "x-token-auth",
                    "access_token": "token",
                    "expired_at": "2026-01-01T00:00:00Z"
                }
            },
            "yaml_file_path": "/clusters/cluster-a.yaml"
        });

        let options: EksAnywhereOptions = serde_json::from_value(payload).expect("options should deserialize");
        let eks_anywhere_parameters = options
            .infrastructure_charts_parameters
            .eks_anywhere_parameters
            .expect("eks_anywhere_parameters should be present");

        assert_eq!(
            eks_anywhere_parameters
                .git_repository
                .expect("git_repository should be present")
                .url
                .expect("url should be present"),
            "https://bitbucket.com/workspace/cluster.git"
        );
        assert_eq!(
            eks_anywhere_parameters
                .yaml_file_path
                .expect("yaml_file_path should be present"),
            "/clusters/cluster-a.yaml"
        );
    }

    #[test]
    fn should_keep_backward_compatibility_when_eks_anywhere_parameters_are_absent() {
        let payload = base_eks_anywhere_options();
        let options: EksAnywhereOptions = serde_json::from_value(payload).expect("options should deserialize");

        assert!(
            options
                .infrastructure_charts_parameters
                .eks_anywhere_parameters
                .is_none()
        );
    }
}
