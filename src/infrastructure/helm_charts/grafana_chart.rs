use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, ChartValuesGenerated, CommonChart, HelmChartError,
    HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
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
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut chart = CommonChart {
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
            vertical_pod_autoscaler: None,
        };

        if let Some(conf) = &self.grafana_datasources.cloudwatch_config {
            chart.chart_info.values.push(ChartSetValue {
                // we use string templating (r"...") to escape dot in annotation's key
                key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                value: conf.aws_iam_cloudwatch_role_arn.to_string(),
            });
            chart.chart_info.values.push(ChartSetValue {
                key: "env.AWS_ROLE_ARN".to_string(),
                value: conf.aws_iam_cloudwatch_role_arn.to_string(),
            });
            chart.chart_info.values.push(ChartSetValue {
                key: "env.AWS_REGION".to_string(),
                value: conf.region.to_string(),
            });
        }

        Ok(chart)
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
    aws_iam_cloudwatch_role_arn: String,
}

impl CloudWatchConfig {
    pub fn new(region: String, aws_iam_cloudwatch_role_arn: String) -> Self {
        CloudWatchConfig {
            region,
            aws_iam_cloudwatch_role_arn,
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
          authType: default
          defaultRegion: {region}
      ",
                    region = cloudwatch_config.region,
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
    use crate::infrastructure::helm_charts::grafana_chart::{GrafanaAdminUser, GrafanaChart, GrafanaDatasources};
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
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
        let common_chart = chart.to_common_helm_chart().unwrap();

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
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }
}
