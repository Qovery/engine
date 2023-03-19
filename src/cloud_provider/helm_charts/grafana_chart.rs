use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, ChartValuesGenerated, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use kube::Client;

/// Grafana helm chart
/// Doc https://github.com/grafana/grafana
pub struct GrafanaChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    grafana_admin_user: GrafanaAdminUser,
    grafana_datasources: GrafanaDatasources,
    persistence_storage_class: String, // TODO(benjaminch): make it an enum
}

impl GrafanaChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        grafana_admin_user: GrafanaAdminUser,
        grafana_datasources: GrafanaDatasources,
        persistence_storage_class: String,
    ) -> GrafanaChart {
        GrafanaChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                GrafanaChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                GrafanaChart::chart_name(),
            ),
            grafana_admin_user,
            grafana_datasources,
            persistence_storage_class,
        }
    }

    pub fn chart_name() -> String {
        "grafana".to_string()
    }
}

impl ToCommonHelmChart for GrafanaChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: GrafanaChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: HelmChartNamespaces::Prometheus,
                values_files: vec![self.chart_values_path.to_string()],
                yaml_files_content: vec![ChartValuesGenerated {
                    filename: "grafana_generated.yaml".to_string(),
                    yaml_content: self.grafana_datasources.to_datasources_yaml(),
                }],
                values: vec![
                    ChartSetValue {
                        key: "image.repository".to_string(),
                        value: "public.ecr.aws/r3m4q3r9/pub-mirror-grafana".to_string(),
                    },
                    ChartSetValue {
                        key: "persistence.storageClassName".to_string(),
                        value: self.persistence_storage_class.to_string(),
                    },
                    ChartSetValue {
                        key: "adminUser".to_string(),
                        value: self.grafana_admin_user.login.to_string(),
                    },
                    ChartSetValue {
                        key: "adminPassword".to_string(),
                        value: self.grafana_admin_user.password.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(GrafanaChartChecker::new())),
        }
    }
}

#[derive(Clone)]
pub struct GrafanaAdminUser {
    login: String,
    password: String,
}

impl GrafanaAdminUser {
    pub fn new(login: String, password: String) -> Self {
        GrafanaAdminUser { login, password }
    }
}

pub struct CloudWatchConfig {
    region: String,
    aws_iam_cloudwatch_key: String,
    aws_iam_cloudwatch_secret: String,
}

impl CloudWatchConfig {
    pub fn new(region: String, aws_iam_cloudwatch_key: String, aws_iam_cloudwatch_secret: String) -> Self {
        CloudWatchConfig {
            region,
            aws_iam_cloudwatch_key,
            aws_iam_cloudwatch_secret,
        }
    }
}

pub struct GrafanaDatasources {
    pub prometheus_internal_url: String,
    pub loki_chart_name: String,
    pub loki_namespace: String,
    pub cloudwatch_config: Option<CloudWatchConfig>,
}

impl GrafanaDatasources {
    fn to_datasources_yaml(&self) -> String {
        let mut datasources = format!(
            "
datasources:
  datasources.yaml:
    apiVersion: 1
    datasources:
      - name: Prometheus
        type: prometheus
        url: \"{prometheus_internal_url}:9090\"
        access: proxy
        isDefault: true
      - name: PromLoki
        type: prometheus
        url: \"http://{loki_chart_name}.{loki_namespace}.svc:3100/loki\"
        access: proxy
        isDefault: false
      - name: Loki
        type: loki
        url: \"http://{loki_chart_name}.{loki_namespace}.svc:3100\"
      ",
            prometheus_internal_url = self.prometheus_internal_url,
            loki_chart_name = self.loki_chart_name,
            loki_namespace = self.loki_namespace,
        );

        if let Some(cloudwatch_config) = &self.cloudwatch_config {
            datasources.push_str(
                format!(
                    "
      - name: Cloudwatch
        type: cloudwatch
        jsonData:
          authType: keys
          defaultRegion: {region}
        secureJsonData:
          accessKey: '{aws_iam_cloudwatch_key}'
          secretKey: '{aws_iam_cloudwatch_secret}'
      ",
                    region = cloudwatch_config.region,
                    aws_iam_cloudwatch_key = cloudwatch_config.aws_iam_cloudwatch_key,
                    aws_iam_cloudwatch_secret = cloudwatch_config.aws_iam_cloudwatch_secret,
                )
                .as_str(),
            );
        }

        datasources
    }
}

#[derive(Clone)]
pub struct GrafanaChartChecker {}

impl GrafanaChartChecker {
    pub fn new() -> GrafanaChartChecker {
        GrafanaChartChecker {}
    }
}

impl Default for GrafanaChartChecker {
    fn default() -> Self {
        GrafanaChartChecker::new()
    }
}

impl ChartInstallationChecker for GrafanaChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1400): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm_charts::grafana_chart::{GrafanaAdminUser, GrafanaChart, GrafanaDatasources};
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn grafana_chart_directory_exists_test() {
        // setup:
        let chart = GrafanaChart::new(
            None,
            GrafanaAdminUser::new("whatever".to_string(), "whatever".to_string()),
            GrafanaDatasources {
                prometheus_internal_url: "whatever".to_string(),
                loki_chart_name: "whatever".to_string(),
                loki_namespace: "whatever".to_string(),
                cloudwatch_config: None,
            },
            "whatever".to_string(),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            GrafanaChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn grafana_chart_values_file_exists_test() {
        // setup:
        let chart = GrafanaChart::new(
            None,
            GrafanaAdminUser::new("whatever".to_string(), "whatever".to_string()),
            GrafanaDatasources {
                prometheus_internal_url: "whatever".to_string(),
                loki_chart_name: "whatever".to_string(),
                loki_namespace: "whatever".to_string(),
                cloudwatch_config: None,
            },
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
                HelmChartType::Shared
            ),
            GrafanaChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn grafana_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = GrafanaChart::new(
            None,
            GrafanaAdminUser::new("whatever".to_string(), "whatever".to_string()),
            GrafanaDatasources {
                prometheus_internal_url: "whatever".to_string(),
                loki_chart_name: "whatever".to_string(),
                loki_namespace: "whatever".to_string(),
                cloudwatch_config: None,
            },
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
                    HelmChartType::Shared
                ),
                GrafanaChart::chart_name(),
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
