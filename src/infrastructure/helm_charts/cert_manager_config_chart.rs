use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInfoUpgradeRetry, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError,
    HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
use kube::Client;

pub struct CertManagerConfigsChart<'a> {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    lets_encrypt_config: &'a LetsEncryptConfig,
    dns_provider_configuration: &'a DnsProviderConfiguration,
    managed_dns_helm_format: String,
    namespace: HelmChartNamespaces,
}

impl<'a> CertManagerConfigsChart<'a> {
    pub fn new(
        chart_prefix_path: Option<&str>,
        lets_encrypt_config: &'a LetsEncryptConfig,
        dns_provider_configuration: &'a DnsProviderConfiguration,
        managed_dns_helm_format: String,
        namespace: HelmChartNamespaces,
    ) -> Self {
        CertManagerConfigsChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                CertManagerConfigsChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                CertManagerConfigsChart::chart_name(),
            ),
            lets_encrypt_config,
            dns_provider_configuration,
            managed_dns_helm_format,
            namespace,
        }
    }

    pub fn chart_name() -> String {
        "cert-manager-configs".to_string()
    }
}

impl ToCommonHelmChart for CertManagerConfigsChart<'_> {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: CertManagerConfigsChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.namespace.clone(),
                // TODO: fix backup apply, it makes the chart deployment failed randomly
                // backup_resources: Some(vec!["cert".to_string(), "issuer".to_string(), "clusterissuer".to_string()]),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "namespace".to_string(),
                        value: self.namespace.to_string(),
                    },
                    ChartSetValue {
                        key: "externalDnsProvider".to_string(),
                        value: self.dns_provider_configuration.get_cert_manager_config_name(),
                    },
                    ChartSetValue {
                        key: "acme.letsEncrypt.emailReport".to_string(),
                        value: self.lets_encrypt_config.email_report().to_string(),
                    },
                    ChartSetValue {
                        key: "acme.letsEncrypt.acmeUrl".to_string(),
                        value: self.lets_encrypt_config.acme_url().to_string(),
                    },
                    ChartSetValue {
                        key: "managedDns".to_string(),
                        value: self.managed_dns_helm_format.to_string(),
                    },
                    // Providers
                    // Cloudflare
                    ChartSetValue {
                        key: "provider.cloudflare.apiToken".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Cloudflare(cloudflare_config) => {
                                cloudflare_config.cloudflare_api_token.to_string()
                            }
                            DnsProviderConfiguration::QoveryDns(_) => "not-set".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "provider.cloudflare.email".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::Cloudflare(cloudflare_config) => {
                                cloudflare_config.cloudflare_email.to_string()
                            }
                            DnsProviderConfiguration::QoveryDns(_) => "not-set".to_string(),
                        },
                    },
                    // Qovery DNS
                    ChartSetValue {
                        key: "provider.pdns.apiPort".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::QoveryDns(qovery_dns_config) => {
                                // TODO(benjaminch): Hack to be fixed: I don't want to use `values_string` field from `ChartInfo`
                                // as it's also kind of a hack.
                                // Good solution will be to merge `values` and `values_string` fields into one and having `ChartSetValue`
                                // to carry type as variant making a cleaner API to be used, way less confusing and ... testable \o/ !
                                //
                                // Ticket: ENG-1404
                                //
                                // pub enum ChartSetValue {
                                //     String(String),
                                //     Integer(i64),
                                //     Boolean(bool),
                                //     Array(Vec<ChartSetValue>),
                                // }
                                //
                                // #[derive(Clone)]
                                // pub struct ChartSetValue {
                                //     pub key: String,
                                //     pub value: ChartSetValue,
                                // }
                                format!("\"{}\"", qovery_dns_config.api_url_port)
                            }
                            DnsProviderConfiguration::Cloudflare(_) => "no-set".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "provider.pdns.apiUrl".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::QoveryDns(qovery_dns_config) => {
                                qovery_dns_config.api_url_scheme_and_domain.to_string()
                            }
                            DnsProviderConfiguration::Cloudflare(_) => "not-set".to_string(),
                        },
                    },
                    ChartSetValue {
                        key: "provider.pdns.apiKey".to_string(),
                        value: match &self.dns_provider_configuration {
                            DnsProviderConfiguration::QoveryDns(qovery_dns_config) => {
                                qovery_dns_config.api_key.to_string()
                            }
                            DnsProviderConfiguration::Cloudflare(_) => "not-set".to_string(),
                        },
                    },
                ],
                upgrade_retry: Some(ChartInfoUpgradeRetry {
                    nb_retry: 10,
                    delay_in_milli_sec: 30_000,
                }),
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(CertManagerConfigsChartChecker::new())),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct CertManagerConfigsChartChecker {}

impl CertManagerConfigsChartChecker {
    pub fn new() -> CertManagerConfigsChartChecker {
        CertManagerConfigsChartChecker {}
    }
}

impl Default for CertManagerConfigsChartChecker {
    fn default() -> Self {
        CertManagerConfigsChartChecker::new()
    }
}

impl ChartInstallationChecker for CertManagerConfigsChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1402): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::cert_manager_config_chart::{CertManagerConfigsChart, LetsEncryptConfig};
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
    use crate::infrastructure::models::dns_provider::qoverydns::QoveryDnsConfig;
    use std::env;
    use url::Url;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn cert_manager_configs_chart_directory_exists_test() {
        // setup:
        let lets_encrypt_config = LetsEncryptConfig::new("whatever".to_string(), true);
        let dns_provider_config = DnsProviderConfiguration::QoveryDns(QoveryDnsConfig {
            api_key: "whatever".to_string(),
            api_url: Url::parse("https://whatever.com").expect("Unable to parse URL"),
            api_url_port: "whatever".to_string(),
            api_url_scheme_and_domain: "whatever".to_string(),
        });
        let chart = CertManagerConfigsChart::new(
            None,
            &lets_encrypt_config,
            &dns_provider_config,
            "whatever".to_string(),
            HelmChartNamespaces::CertManager,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            CertManagerConfigsChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn cert_manager_configs_chart_values_file_exists_test() {
        // setup:
        let lets_encrypt_config = LetsEncryptConfig::new("whatever".to_string(), true);
        let dns_provider_config = DnsProviderConfiguration::QoveryDns(QoveryDnsConfig {
            api_key: "whatever".to_string(),
            api_url: Url::parse("https://whatever.com").expect("Unable to parse URL"),
            api_url_port: "whatever".to_string(),
            api_url_scheme_and_domain: "whatever".to_string(),
        });
        let chart = CertManagerConfigsChart::new(
            None,
            &lets_encrypt_config,
            &dns_provider_config,
            "whatever".to_string(),
            HelmChartNamespaces::CertManager,
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
            CertManagerConfigsChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn cert_manager_configs_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let lets_encrypt_config = LetsEncryptConfig::new("whatever".to_string(), true);
        let dns_provider_config = DnsProviderConfiguration::QoveryDns(QoveryDnsConfig {
            api_key: "whatever".to_string(),
            api_url: Url::parse("https://whatever.com").expect("Unable to parse URL"),
            api_url_port: "whatever".to_string(),
            api_url_scheme_and_domain: "whatever".to_string(),
        });
        let chart = CertManagerConfigsChart::new(
            None,
            &lets_encrypt_config,
            &dns_provider_config,
            "whatever".to_string(),
            HelmChartNamespaces::CertManager,
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
                CertManagerConfigsChart::chart_name()
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }
}
