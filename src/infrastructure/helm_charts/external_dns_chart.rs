use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, UpdateStrategy, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion,
    VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
use crate::io_models::models::{CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use itertools::Itertools;
use kube::Client;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Display;
use std::hash::{Hash, Hasher};
use std::ops::Add;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum ExternalDNSSource {
    GatewayHttpRoute,
    GatewayGrpcRoute,
    GatewayTcpRoute,
    GatewayUdpRoute,
    GatewayTlsRoute,
    Ingress,
    Service,
}

impl Display for ExternalDNSSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let source_str = match *self {
            ExternalDNSSource::GatewayHttpRoute => "gateway-httproute",
            ExternalDNSSource::GatewayGrpcRoute => "gateway-grpcroute",
            ExternalDNSSource::GatewayTcpRoute => "gateway-tcproute",
            ExternalDNSSource::GatewayUdpRoute => "gateway-udproute",
            ExternalDNSSource::GatewayTlsRoute => "gateway-tlsroute",
            ExternalDNSSource::Ingress => "ingress",
            ExternalDNSSource::Service => "service",
        };
        write!(f, "{source_str}")
    }
}

impl ExternalDNSSource {
    fn is_gateway_api(&self) -> bool {
        matches!(
            self,
            Self::GatewayHttpRoute
                | Self::GatewayGrpcRoute
                | Self::GatewayTcpRoute
                | Self::GatewayUdpRoute
                | Self::GatewayTlsRoute
        )
    }
}

#[derive(Debug, Copy, Clone)]
pub enum ExternalDNSSourcesMode {
    GatewayApi,
    Ingress,
    All,
}

pub struct ExternalDNSChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    dns_provider_configuration: DnsProviderConfiguration,
    managed_dns_domain_root_helm_format: String,
    cluster_id: String,
    update_strategy: UpdateStrategy,
    enable_vpa: bool,
    namespace: HelmChartNamespaces,
    customer_helm_chart_vpa_override: Option<CustomerHelmChartsOverride>,
    sources: HashSet<ExternalDNSSource>,
}

impl ExternalDNSChart {
    fn gateway_api_sources() -> Vec<ExternalDNSSource> {
        // HACK(QOV-2106): external-dns 0.21.0 watches TCPRoute, UDPRoute, and TLSRoute through
        // the `gateway.networking.k8s.io/v1alpha2` API. Qovery installs the standard Gateway API
        // channel, which does not install TCPRoute or UDPRoute and serves TLSRoute only at `v1`.
        // Each unavailable informer fails to sync and makes external-dns exit fatally after 60s.
        // Re-enable TCPRoute and UDPRoute when Qovery installs their experimental CRDs. Re-enable
        // TLSRoute after external-dns supports its served `v1` API. https://github.com/kubernetes-sigs/external-dns/issues/6247
        vec![ExternalDNSSource::GatewayHttpRoute, ExternalDNSSource::GatewayGrpcRoute]
    }

    pub fn new(
        chart_prefix_path: Option<&str>,
        dns_provider_configuration: DnsProviderConfiguration,
        managed_dns_domains_root_helm_format: String,
        cluster_id: String,
        update_strategy: UpdateStrategy,
        enable_vpa: bool,
        namespace: HelmChartNamespaces,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        source_mode: ExternalDNSSourcesMode,
    ) -> ExternalDNSChart {
        let gateway_api_sources = Self::gateway_api_sources();

        ExternalDNSChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                ExternalDNSChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                ExternalDNSChart::chart_name(),
            ),
            dns_provider_configuration,
            managed_dns_domain_root_helm_format: managed_dns_domains_root_helm_format,
            cluster_id,
            update_strategy,
            enable_vpa,
            namespace,
            customer_helm_chart_vpa_override: customer_helm_chart_fn(Self::chart_name().add(".vpa")),
            sources: match source_mode {
                ExternalDNSSourcesMode::GatewayApi => gateway_api_sources
                    .into_iter()
                    .chain([ExternalDNSSource::Service])
                    .collect(),
                ExternalDNSSourcesMode::Ingress => [ExternalDNSSource::Ingress, ExternalDNSSource::Service]
                    .into_iter()
                    .collect(),
                ExternalDNSSourcesMode::All => gateway_api_sources
                    .into_iter()
                    .chain([ExternalDNSSource::Ingress, ExternalDNSSource::Service])
                    .collect(),
            },
        }
    }

    fn chart_name() -> String {
        "external-dns".to_string()
    }

    /// Generate a checksum string from the DNS provider secret values.
    /// This checksum is used as a pod annotation to trigger restarts when secrets change.
    /// Uses DefaultHasher to prevent credential exposure in pod metadata.
    fn get_secret_checksum(&self) -> String {
        let mut hasher = DefaultHasher::new();
        match &self.dns_provider_configuration {
            DnsProviderConfiguration::Cloudflare(config) => {
                "cloudflare".hash(&mut hasher);
                config.cloudflare_email.hash(&mut hasher);
                config.cloudflare_api_token.hash(&mut hasher);
            }
            DnsProviderConfiguration::QoveryDns(config) => {
                "pdns".hash(&mut hasher);
                config.api_key.hash(&mut hasher);
            }
            DnsProviderConfiguration::Route53(config) => {
                "route53".hash(&mut hasher);
                config.aws_access_key_id.hash(&mut hasher);
                config.aws_secret_access_key.hash(&mut hasher);
            }
        }
        format!("0x{:x}", hasher.finish())
    }
}

impl ToCommonHelmChart for ExternalDNSChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        // Base values that are always set regardless of provider
        let mut values = vec![
            ChartSetValue {
                key: "updateStrategy.type".to_string(),
                value: self.update_strategy.to_string(),
            },
            ChartSetValue {
                key: "provider".to_string(),
                value: self.dns_provider_configuration.get_external_dns_provider_name(),
            },
            ChartSetValue {
                key: "txtOwnerId".to_string(),
                value: self.cluster_id.to_string(),
            },
            ChartSetValue {
                key: "txtPrefix".to_string(),
                value: "qvy-".to_string(),
            },
        ];

        let values_string = vec![ChartSetValue {
            key: "domainFilters".to_string(),
            value: self.managed_dns_domain_root_helm_format.to_string(),
        }];

        // Set extraArgs based on provider using individual key-value pairs
        match &self.dns_provider_configuration {
            DnsProviderConfiguration::Cloudflare(config) => {
                // Only add --cloudflare-proxied argument when it's set to true
                if config.cloudflare_proxied {
                    values.push(ChartSetValue {
                        key: "extraArgs.cloudflare-proxied".to_string(),
                        value: "null".to_string(),
                    });
                }
                // Add cloudflare-dns-records-per-page argument
                values.push(ChartSetValue {
                    key: "extraArgs.cloudflare-dns-records-per-page".to_string(),
                    value: "100".to_string(),
                });
            }
            DnsProviderConfiguration::QoveryDns(config) => {
                values.extend(vec![
                    ChartSetValue {
                        key: "extraArgs.pdns-server".to_string(),
                        value: format!("{}:{}", config.api_url_scheme_and_domain, config.api_url_port),
                    },
                    ChartSetValue {
                        key: "extraArgs.pdns-api-key".to_string(),
                        value: "$(PDNS_API_KEY)".to_string(),
                    },
                ]);
            }
            DnsProviderConfiguration::Route53(config) => {
                // Add zone-id-filter if hosted_zone_id is provided for better performance
                if let Some(hosted_zone_id) = &config.hosted_zone_id {
                    values.push(ChartSetValue {
                        key: "extraArgs.zone-id-filter".to_string(),
                        value: hosted_zone_id.clone(),
                    });
                }
            }
        }

        // Set proper sources based on selected mode
        for (index, source) in self.sources.iter().sorted().enumerate() {
            values.push(ChartSetValue {
                key: format!("sources[{index}]",),
                value: source.to_string(),
            });
        }

        // Enable Gateway API ListenerSet support only when at least one Gateway-API source is
        // active. Setting `--gateway-listener-sets` on clusters without the ListenerSet CRD
        // (e.g. Ingress-only deployments) would make the informer fail to sync, mirroring the
        // TLSRoute crash class. Qovery's envoy-gateway-crd chart installs ListenerSet whenever
        // it deploys, so gating on Gateway sources matches CRD availability.
        if self.sources.iter().any(ExternalDNSSource::is_gateway_api) {
            values.push(ChartSetValue {
                key: "enableGatewayListenerSets".to_string(),
                value: "true".to_string(),
            });
        }

        // Set env variables based on provider using individual key-value pairs
        match &self.dns_provider_configuration {
            DnsProviderConfiguration::Cloudflare(_) => {
                values.extend(vec![
                    // CF_API_TOKEN
                    ChartSetValue {
                        key: "env[0].name".to_string(),
                        value: "CF_API_TOKEN".to_string(),
                    },
                    ChartSetValue {
                        key: "env[0].valueFrom.secretKeyRef.name".to_string(),
                        value: "external-dns-secret".to_string(),
                    },
                    ChartSetValue {
                        key: "env[0].valueFrom.secretKeyRef.key".to_string(),
                        value: "cloudflare_api_token".to_string(),
                    },
                    ChartSetValue {
                        key: "env[0].valueFrom.secretKeyRef.optional".to_string(),
                        value: "true".to_string(),
                    },
                    // CF_API_KEY (for legacy support, not used for us but still available)
                    ChartSetValue {
                        key: "env[1].name".to_string(),
                        value: "CF_API_KEY".to_string(),
                    },
                    ChartSetValue {
                        key: "env[1].valueFrom.secretKeyRef.name".to_string(),
                        value: "external-dns-secret".to_string(),
                    },
                    ChartSetValue {
                        key: "env[1].valueFrom.secretKeyRef.key".to_string(),
                        value: "cloudflare_api_key".to_string(),
                    },
                    ChartSetValue {
                        key: "env[1].valueFrom.secretKeyRef.optional".to_string(),
                        value: "true".to_string(),
                    },
                    // CF_API_EMAIL
                    ChartSetValue {
                        key: "env[2].name".to_string(),
                        value: "CF_API_EMAIL".to_string(),
                    },
                    ChartSetValue {
                        key: "env[2].valueFrom.secretKeyRef.name".to_string(),
                        value: "external-dns-secret".to_string(),
                    },
                    ChartSetValue {
                        key: "env[2].valueFrom.secretKeyRef.key".to_string(),
                        value: "cloudflare_email".to_string(),
                    },
                ]);
            }
            DnsProviderConfiguration::QoveryDns(_) => {
                values.extend(vec![
                    // PDNS_API_KEY
                    ChartSetValue {
                        key: "env[0].name".to_string(),
                        value: "PDNS_API_KEY".to_string(),
                    },
                    ChartSetValue {
                        key: "env[0].valueFrom.secretKeyRef.name".to_string(),
                        value: "external-dns-secret".to_string(),
                    },
                    ChartSetValue {
                        key: "env[0].valueFrom.secretKeyRef.key".to_string(),
                        value: "pdns_api_key".to_string(),
                    },
                ]);
            }
            DnsProviderConfiguration::Route53(_) => {
                values.extend(vec![
                    // AWS_ACCESS_KEY_ID
                    ChartSetValue {
                        key: "env[0].name".to_string(),
                        value: "AWS_ACCESS_KEY_ID".to_string(),
                    },
                    ChartSetValue {
                        key: "env[0].valueFrom.secretKeyRef.name".to_string(),
                        value: "external-dns-secret".to_string(),
                    },
                    ChartSetValue {
                        key: "env[0].valueFrom.secretKeyRef.key".to_string(),
                        value: "aws_access_key_id".to_string(),
                    },
                    // AWS_SECRET_ACCESS_KEY
                    ChartSetValue {
                        key: "env[1].name".to_string(),
                        value: "AWS_SECRET_ACCESS_KEY".to_string(),
                    },
                    ChartSetValue {
                        key: "env[1].valueFrom.secretKeyRef.name".to_string(),
                        value: "external-dns-secret".to_string(),
                    },
                    ChartSetValue {
                        key: "env[1].valueFrom.secretKeyRef.key".to_string(),
                        value: "aws_secret_access_key".to_string(),
                    },
                    // AWS_DEFAULT_REGION
                    ChartSetValue {
                        key: "env[2].name".to_string(),
                        value: "AWS_DEFAULT_REGION".to_string(),
                    },
                    ChartSetValue {
                        key: "env[2].valueFrom.secretKeyRef.name".to_string(),
                        value: "external-dns-secret".to_string(),
                    },
                    ChartSetValue {
                        key: "env[2].valueFrom.secretKeyRef.key".to_string(),
                        value: "aws_region".to_string(),
                    },
                ]);
            }
        }

        // Add pod annotation with secret checksum to trigger restart when secrets change
        values.push(ChartSetValue {
            key: "podAnnotations.qovery\\.com/external-dns-secret-checksum".to_string(),
            value: self.get_secret_checksum(),
        });

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: "externaldns".to_string(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values,
                values_string,
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(ExternalDNSChartInstallationChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "externaldns-external-dns".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(50)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(200)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(50)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(200)),
                        ),
                        customer_helm_chart_override: self.customer_helm_chart_vpa_override.clone(),
                    }],
                )),
                false => None,
            },
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
struct ExternalDNSChartInstallationChecker {}

impl ExternalDNSChartInstallationChecker {
    pub fn new() -> Self {
        ExternalDNSChartInstallationChecker {}
    }
}

impl Default for ExternalDNSChartInstallationChecker {
    fn default() -> Self {
        ExternalDNSChartInstallationChecker::new()
    }
}

impl ChartInstallationChecker for ExternalDNSChartInstallationChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1368): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

pub struct ExternalDNSSecretChart {
    #[allow(dead_code)]
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    dns_provider_configuration: DnsProviderConfiguration,
    namespace: HelmChartNamespaces,
}

impl ExternalDNSSecretChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        dns_provider_configuration: DnsProviderConfiguration,
        namespace: HelmChartNamespaces,
    ) -> ExternalDNSSecretChart {
        ExternalDNSSecretChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                ExternalDNSSecretChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                ExternalDNSSecretChart::chart_name(),
            ),
            dns_provider_configuration,
            namespace,
        }
    }

    pub fn chart_name() -> String {
        "external-dns-secret".to_string()
    }
}

impl ToCommonHelmChart for ExternalDNSSecretChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: "external-dns-secret".to_string(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "namespace".to_string(),
                        value: self.namespace.to_string(),
                    },
                    ChartSetValue {
                        key: "provider".to_string(),
                        value: self.dns_provider_configuration.get_cert_manager_config_name(),
                    },
                    // Cloudflare secrets
                    ChartSetValue {
                        key: "cloudflare.apiToken".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Cloudflare(config) => config.cloudflare_api_token.to_string(),
                            _ => "null".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "cloudflare.apiKey".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Cloudflare(_) => {
                                // For now, we'll use null since we're primarily using API tokens
                                // This field exists for legacy support
                                "null".to_string()
                            }
                            _ => "null".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "cloudflare.email".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Cloudflare(config) => config.cloudflare_email.to_string(),
                            _ => "null".to_string(),
                        },
                    },
                    // PDNS secrets
                    ChartSetValue {
                        key: "pdns.apiKey".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::QoveryDns(config) => config.api_key.to_string(),
                            _ => "null".to_string(),
                        },
                    },
                    // Route 53 secrets
                    ChartSetValue {
                        key: "route53.accessKeyId".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Route53(config) => config.aws_access_key_id.to_string(),
                            _ => "null".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "route53.secretAccessKey".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Route53(config) => config.aws_secret_access_key.to_string(),
                            _ => "null".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "route53.region".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Route53(config) => config.aws_region.to_string(),
                            _ => "null".to_string(),
                        },
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(ExternalDNSSecretChartInstallationChecker::new())),
            vertical_pod_autoscaler: None, // Secrets don't need VPA
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
struct ExternalDNSSecretChartInstallationChecker {}

impl ExternalDNSSecretChartInstallationChecker {
    pub fn new() -> Self {
        ExternalDNSSecretChartInstallationChecker {}
    }
}

impl Default for ExternalDNSSecretChartInstallationChecker {
    fn default() -> Self {
        ExternalDNSSecretChartInstallationChecker::new()
    }
}

impl ChartInstallationChecker for ExternalDNSSecretChartInstallationChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO: Implement secret verification if needed
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmChartNamespaces, UpdateStrategy};
    use crate::infrastructure::helm_charts::external_dns_chart::{
        ExternalDNSChart, ExternalDNSSecretChart, ExternalDNSSourcesMode,
    };
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
    use crate::infrastructure::models::dns_provider::cloudflare::CloudflareDnsConfig;
    use crate::io_models::models::CustomerHelmChartsOverride;
    use std::env;
    use std::sync::Arc;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn external_dns_chart_directory_exists_test() {
        // setup:
        let chart = ExternalDNSChart::new(
            None,
            DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
                cloudflare_email: "whatever".to_string(),
                cloudflare_api_token: "whatever".to_string(),
                cloudflare_proxied: true,
            }),
            "whatever".to_string(),
            "whatever".to_string(),
            UpdateStrategy::RollingUpdate,
            false,
            HelmChartNamespaces::KubeSystem,
            Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> { None }),
            ExternalDNSSourcesMode::GatewayApi,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            ExternalDNSChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn external_dns_chart_values_file_exists_test() {
        // setup:
        let chart = ExternalDNSChart::new(
            None,
            DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
                cloudflare_email: "whatever".to_string(),
                cloudflare_api_token: "whatever".to_string(),
                cloudflare_proxied: true,
            }),
            "whatever".to_string(),
            "whatever".to_string(),
            UpdateStrategy::RollingUpdate,
            false,
            HelmChartNamespaces::KubeSystem,
            Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> { None }),
            ExternalDNSSourcesMode::GatewayApi,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared,
            ),
            ExternalDNSChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn external_dns_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = ExternalDNSChart::new(
            None,
            DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
                cloudflare_email: "whatever".to_string(),
                cloudflare_api_token: "whatever".to_string(),
                cloudflare_proxied: true,
            }),
            "whatever".to_string(),
            "whatever".to_string(),
            UpdateStrategy::RollingUpdate,
            false,
            HelmChartNamespaces::KubeSystem,
            Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> { None }),
            ExternalDNSSourcesMode::GatewayApi,
        );
        let mut common_chart = chart.to_common_helm_chart().unwrap();

        // Filter out extraArgs.* values since extraArgs is an empty object {} in the YAML
        // and we dynamically set individual keys like extraArgs.cloudflare-proxied
        common_chart
            .chart_info
            .values
            .retain(|value| !value.key.starts_with("extraArgs."));

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared,
                ),
                ExternalDNSChart::chart_name()
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }

    /// Makes sure ExternalDNSSecretChart directory containing all YAML files exists.
    #[test]
    fn external_dns_secret_chart_directory_exists_test() {
        // setup:
        let chart = ExternalDNSSecretChart::new(
            None,
            DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
                cloudflare_email: "whatever".to_string(),
                cloudflare_api_token: "whatever".to_string(),
                cloudflare_proxied: true,
            }),
            HelmChartNamespaces::KubeSystem,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            ExternalDNSSecretChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure ExternalDNSSecretChart values file exists.
    #[test]
    fn external_dns_secret_chart_values_file_exists_test() {
        // setup:
        let chart = ExternalDNSSecretChart::new(
            None,
            DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
                cloudflare_email: "whatever".to_string(),
                cloudflare_api_token: "whatever".to_string(),
                cloudflare_proxied: true,
            }),
            HelmChartNamespaces::KubeSystem,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared,
            ),
            ExternalDNSSecretChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file for ExternalDNSSecretChart.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn external_dns_secret_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = ExternalDNSSecretChart::new(
            None,
            DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
                cloudflare_email: "whatever".to_string(),
                cloudflare_api_token: "whatever".to_string(),
                cloudflare_proxied: true,
            }),
            HelmChartNamespaces::KubeSystem,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared,
                ),
                ExternalDNSSecretChart::chart_name()
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }

    /// Verify that ExternalDNSSource enum variants maintain their declaration order when sorted.
    /// This is critical because the enum derives Ord/PartialOrd, and the ordering is used
    /// when generating helm chart sources (see line 225 where .sorted() is called).
    /// It prevents from useless helm diffs when the order of sources changes unexpectedly.
    #[test]
    fn external_dns_source_enum_preserves_variant_order() {
        use crate::infrastructure::helm_charts::external_dns_chart::ExternalDNSSource;

        // Create a vec with all variants in reverse order
        let mut sources = vec![
            ExternalDNSSource::Service,
            ExternalDNSSource::Ingress,
            ExternalDNSSource::GatewayTlsRoute,
            ExternalDNSSource::GatewayUdpRoute,
            ExternalDNSSource::GatewayGrpcRoute,
            ExternalDNSSource::GatewayTcpRoute,
            ExternalDNSSource::GatewayHttpRoute,
        ];

        // Sort the variants
        sources.sort();

        // Verify the sorted order matches the declaration order
        assert_eq!(
            sources,
            vec![
                ExternalDNSSource::GatewayHttpRoute,
                ExternalDNSSource::GatewayGrpcRoute,
                ExternalDNSSource::GatewayTcpRoute,
                ExternalDNSSource::GatewayUdpRoute,
                ExternalDNSSource::GatewayTlsRoute,
                ExternalDNSSource::Ingress,
                ExternalDNSSource::Service,
            ]
        );
    }

    fn make_external_dns_chart(source_mode: ExternalDNSSourcesMode) -> ExternalDNSChart {
        ExternalDNSChart::new(
            None,
            DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
                cloudflare_email: "whatever".to_string(),
                cloudflare_api_token: "whatever".to_string(),
                cloudflare_proxied: false,
            }),
            "whatever".to_string(),
            "whatever".to_string(),
            UpdateStrategy::RollingUpdate,
            false,
            HelmChartNamespaces::KubeSystem,
            Arc::new(|_| None),
            source_mode,
        )
    }

    /// QOV-2106 — Routes unavailable in Qovery's standard Gateway API installation must be excluded.
    #[test]
    fn external_dns_chart_excludes_incompatible_gateway_routes_for_every_gateway_api_mode() {
        use crate::infrastructure::helm_charts::external_dns_chart::ExternalDNSSource;

        for mode in [ExternalDNSSourcesMode::GatewayApi, ExternalDNSSourcesMode::All] {
            let mode_label = format!("{mode:?}");
            let chart = make_external_dns_chart(mode);
            for source in [
                ExternalDNSSource::GatewayTcpRoute,
                ExternalDNSSource::GatewayUdpRoute,
                ExternalDNSSource::GatewayTlsRoute,
            ] {
                assert!(
                    !chart.sources.contains(&source),
                    "{source} must be excluded in {mode_label} mode"
                );
            }
            assert!(chart.sources.contains(&ExternalDNSSource::GatewayHttpRoute));
            assert!(chart.sources.contains(&ExternalDNSSource::GatewayGrpcRoute));
            assert!(chart.sources.contains(&ExternalDNSSource::Service));
        }
    }

    /// QOV-1926 — `enableGatewayListenerSets` must be set to `true` whenever any
    /// Gateway-API source is active, and must NOT be set otherwise (so the YAML default
    /// `false` wins). Setting it on a cluster without the ListenerSet CRD makes the
    /// informer fail to sync.
    #[test]
    fn external_dns_chart_emits_enable_gateway_listener_sets_only_when_gateway_sources_active() {
        let key = "enableGatewayListenerSets";

        for mode in [ExternalDNSSourcesMode::GatewayApi, ExternalDNSSourcesMode::All] {
            let mode_label = format!("{mode:?}");
            let chart = make_external_dns_chart(mode).to_common_helm_chart().unwrap();
            let entry = chart.chart_info.values.iter().find(|v| v.key == key);
            assert_eq!(
                entry.map(|v| v.value.as_str()),
                Some("true"),
                "{key}=true must be emitted in {mode_label} mode"
            );
        }

        let ingress_chart = make_external_dns_chart(ExternalDNSSourcesMode::Ingress)
            .to_common_helm_chart()
            .unwrap();
        assert!(
            ingress_chart.chart_info.values.iter().all(|v| v.key != key),
            "{key} must not be emitted in Ingress-only mode (would crash the informer without the ListenerSet CRD)"
        );
    }
}
