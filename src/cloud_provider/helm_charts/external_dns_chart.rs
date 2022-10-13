use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::dns_provider::DnsProviderConfiguration;
use crate::errors::CommandError;
use kube::Client;

pub struct ExternalDNSChart {
    chart_path: HelmChartPath,
    dns_provider_configuration: DnsProviderConfiguration,
    managed_dns_domains_root_helm_format: String,
    proxied: bool,
    cluster_id: String,
}

impl ExternalDNSChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        dns_provider_configuration: DnsProviderConfiguration,
        managed_dns_domains_root_helm_format: String,
        proxied: bool,
        cluster_id: String,
    ) -> ExternalDNSChart {
        ExternalDNSChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                ExternalDNSChart::chart_name(),
            ),
            dns_provider_configuration,
            managed_dns_domains_root_helm_format,
            proxied,
            cluster_id,
        }
    }

    fn chart_name() -> String {
        "external-dns".to_string()
    }
}

impl ToCommonHelmChart for ExternalDNSChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        let mut chart = CommonChart {
            chart_info: ChartInfo {
                name: "externaldns".to_string(),
                path: self.chart_path.to_string(),
                values: vec![
                    ChartSetValue {
                        key: "provider".to_string(),
                        value: self.dns_provider_configuration.get_cert_manager_config_name(),
                    },
                    ChartSetValue {
                        key: "annotationFilter".to_string(),
                        value: "external-dns.alpha.kubernetes.io/exclude notin (true)".to_string(), // Make external DNS ignore this ingress https://github.com/kubernetes-sigs/external-dns/issues/1910#issuecomment-976371247
                    },
                    ChartSetValue {
                        key: "domainFilters".to_string(),
                        value: self.managed_dns_domains_root_helm_format.to_string(),
                    },
                    ChartSetValue {
                        key: "triggerLoopOnEvent".to_string(),
                        value: true.to_string(),
                    },
                    ChartSetValue {
                        key: "policy".to_string(),
                        value: "sync".to_string(),
                    },
                    ChartSetValue {
                        key: "txtOwnerId".to_string(),
                        value: self.cluster_id.to_string(),
                    },
                    ChartSetValue {
                        key: "txtPrefix".to_string(),
                        value: format!("qvy-{}-", self.cluster_id),
                    },
                    ChartSetValue {
                        key: "replicas".to_string(),
                        value: 1.to_string(),
                    },
                    // resources limits
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: "50m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: "50m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: "50Mi".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: "50Mi".to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(ExternalDNSChartInstallationChecker::new())),
        };

        match &self.dns_provider_configuration {
            DnsProviderConfiguration::Cloudflare(config) => {
                chart.chart_info.values.extend(vec![
                    ChartSetValue {
                        key: "cloudflare.apiToken".to_string(),
                        value: config.cloudflare_api_token.to_string(),
                    },
                    ChartSetValue {
                        key: "cloudflare.email".to_string(),
                        value: config.cloudflare_email.to_string(),
                    },
                    ChartSetValue {
                        key: "cloudflare.proxied".to_string(),
                        value: self.proxied.to_string(),
                    },
                ]);
            }
            DnsProviderConfiguration::QoveryDns(config) => {
                chart.chart_info.values.extend(vec![
                    ChartSetValue {
                        key: "pdns.apiUrl".to_string(),
                        value: config.api_url_scheme_and_domain.to_string(),
                    },
                    ChartSetValue {
                        key: "pdns.apiPort".to_string(),
                        value: config.api_url_port.to_string(),
                    },
                    ChartSetValue {
                        key: "pdns.apiKey".to_string(),
                        value: config.api_key.to_string(),
                    },
                ]);
            }
        }

        chart
    }
}

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
}
