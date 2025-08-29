use crate::cmd::docker;
use crate::errors::EngineError;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::kubernetes::{Kind, Kubernetes, KubernetesVersion, event_details};
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
    pub metal_lb_chart_overrides: MetalLbChartOverrides,
    pub nginx_chart_overrides: NginxChartOverrides,
    pub cert_manager_parameters: CertManagerParameters,
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
