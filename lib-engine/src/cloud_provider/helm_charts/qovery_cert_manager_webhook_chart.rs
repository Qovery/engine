use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::dns_provider::qoverydns::QoveryDnsConfig;
use crate::errors::CommandError;
use kube::Client;

pub struct QoveryCertManagerWebhookChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    qovery_dns_config: QoveryDnsConfig,
}

impl QoveryCertManagerWebhookChart {
    pub fn new(chart_prefix_path: Option<&str>, qovery_dns_config: QoveryDnsConfig) -> Self {
        QoveryCertManagerWebhookChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryCertManagerWebhookChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryCertManagerWebhookChart::chart_name(),
            ),
            qovery_dns_config,
        }
    }

    pub fn chart_name() -> String {
        "qovery-cert-manager-webhook".to_string()
    }
}

impl ToCommonHelmChart for QoveryCertManagerWebhookChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: QoveryCertManagerWebhookChart::chart_name(),
                namespace: HelmChartNamespaces::CertManager,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "secret.apiKey".to_string(),
                        value: self.qovery_dns_config.api_key.to_string(),
                    },
                    ChartSetValue {
                        key: "secret.apiUrl".to_string(),
                        value: self.qovery_dns_config.api_url.to_string(), // URL standard port will be omitted from string as standard (80 HTTP & 443 HTTPS)
                    },
                    ChartSetValue {
                        key: "certManager.namespace".to_string(),
                        value: HelmChartNamespaces::CertManager.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryCertManagerWebhookChartChecker::new())),
        }
    }
}

pub struct QoveryCertManagerWebhookChartChecker {}

impl QoveryCertManagerWebhookChartChecker {
    pub fn new() -> QoveryCertManagerWebhookChartChecker {
        QoveryCertManagerWebhookChartChecker {}
    }
}

impl Default for QoveryCertManagerWebhookChartChecker {
    fn default() -> Self {
        QoveryCertManagerWebhookChartChecker::new()
    }
}

impl ChartInstallationChecker for QoveryCertManagerWebhookChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1392): Implement chart install verification
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm_charts::qovery_cert_manager_webhook_chart::QoveryCertManagerWebhookChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use crate::dns_provider::qoverydns::QoveryDnsConfig;
    use std::env;
    use url::Url;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_cert_manager_webhook_chart_directory_exists_test() {
        // setup:
        let chart = QoveryCertManagerWebhookChart::new(
            None,
            QoveryDnsConfig {
                api_url: Url::parse("https://whatever.com").expect("Error parsing URL"),
                api_key: "whatever".to_string(),
                api_url_scheme_and_domain: "whatever".to_string(),
                api_url_port: "whatever".to_string(),
            },
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            QoveryCertManagerWebhookChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }

    /// Makes sure chart values file exists.
    #[test]
    fn qovery_cert_manager_webhook_chart_values_file_exists_test() {
        // setup:
        let chart = QoveryCertManagerWebhookChart::new(
            None,
            QoveryDnsConfig {
                api_url: Url::parse("https://whatever.com").expect("Error parsing URL"),
                api_key: "whatever".to_string(),
                api_url_scheme_and_domain: "whatever".to_string(),
                api_url_port: "whatever".to_string(),
            },
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
            QoveryCertManagerWebhookChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn qovery_cert_manager_webhook_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = QoveryCertManagerWebhookChart::new(
            None,
            QoveryDnsConfig {
                api_url: Url::parse("https://whatever.com").expect("Error parsing URL"),
                api_key: "whatever".to_string(),
                api_url_scheme_and_domain: "whatever".to_string(),
                api_url_port: "whatever".to_string(),
            },
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
                QoveryCertManagerWebhookChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
