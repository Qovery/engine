use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces, HpaMode,
    QoveryGatewayClass,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::runtime::block_on;
use crate::services::kube_client::GatewayClass;
use kube::Api;
use kube::core::params::ListParams;
use kube::core::{Expression, Selector};
use std::collections::HashSet;

pub struct QoveryGatewayClassChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    gateway_classes_to_be_installed: HashSet<QoveryGatewayClass>,
    access_log_format: Option<String>,
    hpa_mode: HpaMode,
}

impl QoveryGatewayClassChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        namespace: HelmChartNamespaces,
        gateway_classes_to_be_checked_after_install: HashSet<QoveryGatewayClass>,
        access_log_format: Option<String>,
        hpa_mode: HpaMode,
    ) -> Self {
        QoveryGatewayClassChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryGatewayClassChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryGatewayClassChart::chart_name(),
            ),
            namespace,
            gateway_classes_to_be_installed: gateway_classes_to_be_checked_after_install,
            access_log_format,
            hpa_mode,
        }
    }

    pub fn chart_name() -> String {
        "qovery-gateway-class".to_string()
    }

    /// Helper function to add HPA configuration values for a specific gateway
    fn add_hpa_values(values: &mut Vec<ChartSetValue>, gateway_prefix: &str, hpa_mode: &HpaMode) {
        match hpa_mode {
            HpaMode::Enabled { config } => {
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.enabled"),
                    value: "true".to_string(),
                });
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.minReplicas"),
                    value: config.min_replicas.to_string(),
                });
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.maxReplicas"),
                    value: config.max_replicas.to_string(),
                });
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.targetCPUUtilizationPercentage"),
                    value: config
                        .cpu_average_utilization_percentage
                        .as_ref()
                        .map(|cpu| cpu.as_u8_percent().to_string())
                        .unwrap_or_else(|| "null".to_string()),
                });
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.targetMemoryUtilizationPercentage"),
                    value: config
                        .memory_average_utilization_percentage
                        .as_ref()
                        .map(|mem| mem.as_u8_percent().to_string())
                        .unwrap_or_else(|| "null".to_string()),
                });
            }
            HpaMode::Disabled => {
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.enabled"),
                    value: "false".to_string(),
                });
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.minReplicas"),
                    value: "1".to_string(),
                });
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.maxReplicas"),
                    value: "1".to_string(),
                });
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.targetCPUUtilizationPercentage"),
                    value: "null".to_string(),
                });
                values.push(ChartSetValue {
                    key: format!("{gateway_prefix}.hpa.targetMemoryUtilizationPercentage"),
                    value: "null".to_string(),
                });
            }
        }
    }
}

impl ToCommonHelmChart for QoveryGatewayClassChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values = vec![
            ChartSetValue {
                key: "gatewayClass.qoveryPublic.enable".to_string(),
                value: self
                    .gateway_classes_to_be_installed
                    .contains(&QoveryGatewayClass::PublicGateway)
                    .to_string(),
            },
            ChartSetValue {
                key: "gatewayClass.qoveryPrivate.enable".to_string(),
                value: self
                    .gateway_classes_to_be_installed
                    .contains(&QoveryGatewayClass::PrivateGateway)
                    .to_string(),
            },
        ];

        // Add access log format if provided
        if let Some(ref format) = self.access_log_format {
            values.push(ChartSetValue {
                key: "gatewayClass.qoveryPublic.accessLog.format".to_string(),
                value: format.clone(),
            });
            values.push(ChartSetValue {
                key: "gatewayClass.qoveryPrivate.accessLog.format".to_string(),
                value: format.clone(),
            });
        } else {
            values.push(ChartSetValue {
                key: "gatewayClass.qoveryPublic.accessLog.format".to_string(),
                value: "".to_string(),
            });
            values.push(ChartSetValue {
                key: "gatewayClass.qoveryPrivate.accessLog.format".to_string(),
                value: "".to_string(),
            });
        }

        // Configure HPA for both public and private gateways
        Self::add_hpa_values(&mut values, "gatewayClass.qoveryPublic", &self.hpa_mode);
        Self::add_hpa_values(&mut values, "gatewayClass.qoveryPrivate", &self.hpa_mode);

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryGatewayClassChart::chart_name(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values,
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryGatewayClassChartInstallationChecker::new(
                self.gateway_classes_to_be_installed.clone(),
            ))),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct QoveryGatewayClassChartInstallationChecker {
    gateway_classes_to_be_checked_after_install: HashSet<QoveryGatewayClass>,
}

impl QoveryGatewayClassChartInstallationChecker {
    pub fn new(gateway_classes_to_be_checked_after_install: HashSet<QoveryGatewayClass>) -> Self {
        QoveryGatewayClassChartInstallationChecker {
            gateway_classes_to_be_checked_after_install,
        }
    }
}

impl ChartInstallationChecker for QoveryGatewayClassChartInstallationChecker {
    fn verify_installation(&self, kube_client: &kube::Client) -> Result<(), CommandError> {
        let gateway_classes: Api<GatewayClass> = Api::all(kube_client.clone());

        if !self.gateway_classes_to_be_checked_after_install.is_empty() {
            let selector: Selector = Expression::In(
                "qovery-type".to_string(),
                self.gateway_classes_to_be_checked_after_install
                    .iter()
                    .map(|pc| pc.to_string().to_lowercase())
                    .collect(),
            )
            .into();

            match block_on(gateway_classes.list(&ListParams::default().labels_from(&selector))) {
                Ok(gateway_classes_result) => {
                    let installed_gateway_classes: HashSet<String, std::collections::hash_map::RandomState> =
                        HashSet::from_iter(
                            gateway_classes_result
                                .items
                                .into_iter()
                                .filter_map(|item| item.metadata.name.map(|name| name.to_lowercase())),
                        );
                    for required_gateway_class in self.gateway_classes_to_be_checked_after_install.iter() {
                        if !installed_gateway_classes.contains(&required_gateway_class.to_string().to_lowercase()) {
                            return Err(CommandError::new_from_safe_message(format!(
                                "Error: q-gateway-class (metadata.name={required_gateway_class}) is not set"
                            )));
                        }
                    }
                }
                Err(e) => {
                    return Err(CommandError::new(
                        format!("Error trying to get q-gateway-class ({selector})",),
                        Some(e.to_string()),
                        None,
                    ));
                }
            }
        }

        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmChartNamespaces, HpaMode};
    use crate::infrastructure::helm_charts::qovery_gateway_class_chart::QoveryGatewayClassChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use std::collections::HashSet;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_gateway_class_chart_directory_exists_test() {
        // setup:
        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            None,
            HpaMode::Enabled {
                config: Default::default(),
            },
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            QoveryGatewayClassChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn qovery_gateway_class_chart_values_file_exists_test() {
        // setup:
        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            None,
            HpaMode::Enabled {
                config: Default::default(),
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
            QoveryGatewayClassChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn qovery_gateway_class_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            None,
            HpaMode::Enabled {
                config: Default::default(),
            },
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
                QoveryGatewayClassChart::chart_name()
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
