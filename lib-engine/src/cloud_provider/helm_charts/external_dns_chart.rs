use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::dns_provider::DnsProviderConfiguration;
use crate::errors::CommandError;
use kube::Client;

pub struct ExternalDNSChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
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
            chart_values_path: HelmChartValuesFilePath::new(
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
        CommonChart {
            chart_info: ChartInfo {
                name: "externaldns".to_string(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "provider".to_string(),
                        value: self.dns_provider_configuration.get_cert_manager_config_name(),
                    },
                    ChartSetValue {
                        key: "domainFilters".to_string(),
                        value: self
                            .managed_dns_domains_root_helm_format
                            .to_string()
                            .replace('.', r"\."), // escape . from domains
                    },
                    ChartSetValue {
                        key: "txtOwnerId".to_string(),
                        value: self.cluster_id.to_string(),
                    },
                    ChartSetValue {
                        key: "txtPrefix".to_string(),
                        value: format!("qvy-{}-", self.cluster_id),
                    },
                    // Providers configuration
                    // Cloudflare
                    ChartSetValue {
                        key: "cloudflare.apiToken".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Cloudflare(config) => config.cloudflare_api_token.to_string(),
                            _ => "".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "cloudflare.email".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Cloudflare(config) => config.cloudflare_email.to_string(),
                            _ => "".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "cloudflare.proxied".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Cloudflare(_config) => self.proxied.to_string(),
                            _ => "".to_string(),
                        },
                    },
                    // PDNS
                    ChartSetValue {
                        key: "pdns.apiUrl".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::QoveryDns(config) => config.api_url_scheme_and_domain.to_string(),
                            _ => "".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "pdns.apiPort".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::QoveryDns(config) => config.api_url_port.to_string(),
                            _ => "".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "pdns.apiKey".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::QoveryDns(config) => config.api_key.to_string(),
                            _ => "".to_string(),
                        },
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(ExternalDNSChartInstallationChecker::new())),
        }
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

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm_charts::external_dns_chart::ExternalDNSChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use crate::dns_provider::cloudflare::CloudflareDnsConfig;
    use crate::dns_provider::DnsProviderConfiguration;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn external_dns_chart_directory_exists_test() {
        // setup:
        let chart = ExternalDNSChart::new(
            None,
            DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
                cloudflare_email: "whatever".to_string(),
                cloudflare_api_token: "whatever".to_string(),
            }),
            "whatever".to_string(),
            true,
            "whatever".to_string(),
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
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
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
            }),
            "whatever".to_string(),
            true,
            "whatever".to_string(),
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
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
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
            }),
            "whatever".to_string(),
            true,
            "whatever".to_string(),
        );
        let common_chart = chart.to_common_helm_chart();

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
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
