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
use base64::Engine;
use kube::Api;
use kube::core::params::ListParams;
use kube::core::{Expression, Selector};
use serde_json::Value;
use std::collections::HashSet;

pub struct QoveryGatewayClassChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    additional_chart_path: Option<HelmChartValuesFilePath>,
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
        karpenter_enabled: bool,
    ) -> Self {
        QoveryGatewayClassChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryGatewayClassChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                QoveryGatewayClassChart::chart_name(),
            ),
            additional_chart_path: match karpenter_enabled {
                true => Some(HelmChartValuesFilePath::new(
                    chart_prefix_path,
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    format!("{}-with-karpenter", QoveryGatewayClassChart::chart_name()),
                )),
                false => None,
            },
            namespace,
            gateway_classes_to_be_installed: gateway_classes_to_be_checked_after_install,
            access_log_format,
            hpa_mode,
        }
    }

    pub fn chart_name() -> String {
        "qovery-gateway-class".to_string()
    }

    /// Helper function to handle log format processing and encoding, ensuring it is valid JSON and properly minified before encoding
    fn encode_access_log_format(format: &str) -> Result<String, HelmChartError> {
        // Strip surrounding quotes if present (some configs store it as "\"{ ... }\"")
        // First attempt: try to parse as a JSON string (handles escaped quotes)
        let mut unquoted = serde_json::from_str::<String>(format).unwrap_or_else(|_| format.to_string());

        // Second attempt: if still starts/ends with quotes, strip them manually (handles double-escaping)
        while unquoted.len() > 2 && unquoted.starts_with('"') && unquoted.ends_with('"') {
            unquoted = unquoted[1..unquoted.len() - 1].to_string();
        }

        // Normalize the input: replace literal newlines, tabs, and carriage returns with spaces
        // This handles cases where the JSON comes with actual line breaks from config files
        let normalized: String = unquoted
            .chars()
            .map(|c| match c {
                '\n' | '\r' | '\t' => ' ',
                _ => c,
            })
            .collect();

        // Parse the JSON to validate it and minify it (remove extra whitespace)
        let json_value = serde_json::from_str::<Value>(&normalized).map_err(|e| HelmChartError::RenderingError {
            chart_name: Self::chart_name(),
            msg: format!("Invalid JSON format for envoy access log format: {}", e),
        })?;

        // Re-serialize without pretty-printing to get a one-line JSON string
        let minified = serde_json::to_string(&json_value).map_err(|e| HelmChartError::RenderingError {
            chart_name: Self::chart_name(),
            msg: format!("Failed to serialize envoy access log format: {}", e),
        })?;

        Ok(base64::engine::general_purpose::STANDARD.encode(minified))
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
        let mut values_files = vec![self.chart_values_path.to_string()];
        if let Some(additional_chart_path) = &self.additional_chart_path {
            values_files.push(additional_chart_path.to_string());
        }

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

        let mut values_string = vec![];

        // Process access log format if provided (base64 encode for safe Helm transmission)
        let encoded_format = self
            .access_log_format
            .as_ref()
            .map(|f| f.trim())
            .filter(|f| !f.is_empty())
            .map(Self::encode_access_log_format)
            .transpose()?
            .unwrap_or_default();

        // Set the same format for both public and private gateways
        values_string.push(ChartSetValue {
            key: "gatewayClass.qoveryPublic.accessLog.format".to_string(),
            value: encoded_format.clone(),
        });
        values_string.push(ChartSetValue {
            key: "gatewayClass.qoveryPrivate.accessLog.format".to_string(),
            value: encoded_format,
        });

        // Configure HPA for both public and private gateways
        Self::add_hpa_values(&mut values, "gatewayClass.qoveryPublic", &self.hpa_mode);
        Self::add_hpa_values(&mut values, "gatewayClass.qoveryPrivate", &self.hpa_mode);

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryGatewayClassChart::chart_name(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files,
                values,
                values_string,
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryGatewayClassChartInstallationChecker::new(
                self.gateway_classes_to_be_installed.clone(),
            ))),
            vertical_pod_autoscaler: None,
            pre_execute_action: None,
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
    use crate::infrastructure::models::kubernetes::Kind;
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
            true,
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
            true,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(Kind::Eks),
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
            true,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(Kind::Eks),
                ),
                QoveryGatewayClassChart::chart_name(),
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }

    /// Test that valid JSON access log format is properly base64 encoded
    #[test]
    fn qovery_gateway_class_chart_valid_json_access_log_format_test() {
        // setup: valid compact JSON
        let json_format = r#"{"time":"%START_TIME%","method":"%REQ(:METHOD)%"}"#.to_string();
        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            Some(json_format.clone()),
            HpaMode::Enabled {
                config: Default::default(),
            },
            true,
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify: should have base64 encoded values
        use base64::Engine;

        let public_value = common_chart
            .chart_info
            .values_string
            .iter()
            .find(|v| v.key == "gatewayClass.qoveryPublic.accessLog.format")
            .expect("Public gateway access log format should exist");

        // Decode and verify it's valid JSON with correct values
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&public_value.value)
            .expect("Should be valid base64");
        let decoded_str = String::from_utf8(decoded).expect("Should be valid UTF-8");
        let decoded_json: serde_json::Value = serde_json::from_str(&decoded_str).expect("Should be valid JSON");

        // Verify the JSON contains the expected fields
        assert_eq!(decoded_json["time"], "%START_TIME%");
        assert_eq!(decoded_json["method"], "%REQ(:METHOD)%");

        // Verify private gateway has the same value
        let private_value = common_chart
            .chart_info
            .values_string
            .iter()
            .find(|v| v.key == "gatewayClass.qoveryPrivate.accessLog.format")
            .expect("Private gateway access log format should exist");

        assert_eq!(
            public_value.value, private_value.value,
            "Public and private gateways should have the same access log format"
        );
    }

    /// Test that multi-line JSON is minified before base64 encoding
    #[test]
    fn qovery_gateway_class_chart_multiline_json_access_log_format_test() {
        // setup: multi-line formatted JSON
        let multiline_json = r#"{
  "time": "%START_TIME%",
  "method": "%REQ(:METHOD)%",
  "path": "%REQ(X-ENVOY-ORIGINAL-PATH?:PATH)%"
}"#
        .to_string();
        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            Some(multiline_json),
            HpaMode::Enabled {
                config: Default::default(),
            },
            true,
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify: should be minified (no newlines) and base64 encoded
        use base64::Engine;

        let public_value = common_chart
            .chart_info
            .values_string
            .iter()
            .find(|v| v.key == "gatewayClass.qoveryPublic.accessLog.format")
            .expect("Public gateway access log format should exist");

        // Verify the decoded value doesn't contain newlines
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&public_value.value)
            .expect("Should be valid base64");
        let decoded_str = String::from_utf8(decoded).expect("Should be valid UTF-8");
        assert!(!decoded_str.contains('\n'), "Decoded JSON should not contain newlines");

        // Verify it's valid JSON with the correct fields
        let decoded_json: serde_json::Value = serde_json::from_str(&decoded_str).expect("Should be valid JSON");
        assert_eq!(decoded_json["time"], "%START_TIME%");
        assert_eq!(decoded_json["method"], "%REQ(:METHOD)%");
        assert_eq!(decoded_json["path"], "%REQ(X-ENVOY-ORIGINAL-PATH?:PATH)%");
    }

    /// Test with real-world format from local.json (as it appears after JSON parsing)
    #[test]
    fn qovery_gateway_class_chart_real_world_access_log_format_test() {
        // setup: realistic format from local.json
        // In the JSON file it's stored as: "envoy.log_format": "\"{\n  \"start_time\": ...\n}\""
        // After JSON parsing, this becomes a string with actual newlines, which is what we receive
        let json_format = "{\n  \"start_time\": \"%START_TIME%\",\n  \"method\": \"%REQ(:METHOD)%\",\n  \"x-envoy-origin-path\": \"%REQ(X-ENVOY-ORIGINAL-PATH?:PATH)%\",\n  \"protocol\": \"%PROTOCOL%\",\n  \"response_code\": \"%RESPONSE_CODE%\",\n  \"response_flags\": \"%RESPONSE_FLAGS%\",\n  \"response_code_details\": \"%RESPONSE_CODE_DETAILS%\",\n  \"connection_termination_details\": \"%CONNECTION_TERMINATION_DETAILS%\",\n  \"upstream_transport_failure_reason\": \"%UPSTREAM_TRANSPORT_FAILURE_REASON%\",\n  \"bytes_received\": \"%BYTES_RECEIVED%\",\n  \"bytes_sent\": \"%BYTES_SENT%\",\n  \"duration\": \"%DURATION%\",\n  \"x-envoy-upstream-service-time\": \"%RESP(X-ENVOY-UPSTREAM-SERVICE-TIME)%\",\n  \"x-forwarded-for\": \"%REQ(X-FORWARDED-FOR)%\",\n  \"user-agent\": \"%REQ(USER-AGENT)%\",\n  \"x-request-id\": \"%REQ(X-REQUEST-ID)%\",\n  \":authority\": \"%REQ(:AUTHORITY)%\",\n  \"upstream_host\": \"%UPSTREAM_HOST%\",\n  \"upstream_cluster\": \"%UPSTREAM_CLUSTER%\",\n  \"upstream_local_address\": \"%UPSTREAM_LOCAL_ADDRESS%\",\n  \"downstream_local_address\": \"%DOWNSTREAM_LOCAL_ADDRESS%\",\n  \"downstream_remote_address\": \"%DOWNSTREAM_REMOTE_ADDRESS%\",\n  \"requested_server_name\": \"%REQUESTED_SERVER_NAME%\",\n  \"route_name\": \"%ROUTE_NAME%\"\n}".to_string();

        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            Some(json_format),
            HpaMode::Enabled {
                config: Default::default(),
            },
            true,
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify: should be minified and base64 encoded
        use base64::Engine;

        let public_value = common_chart
            .chart_info
            .values_string
            .iter()
            .find(|v| v.key == "gatewayClass.qoveryPublic.accessLog.format")
            .expect("Public gateway access log format should exist");

        // Decode and verify
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&public_value.value)
            .expect("Should be valid base64");
        let decoded_str = String::from_utf8(decoded).expect("Should be valid UTF-8");

        // Should not contain newlines (minified)
        assert!(!decoded_str.contains('\n'), "Decoded JSON should not contain newlines");

        // Verify it's valid JSON with all expected fields
        let decoded_json: serde_json::Value = serde_json::from_str(&decoded_str).expect("Should be valid JSON");

        // Check a few key fields to ensure they're all present
        assert_eq!(decoded_json["start_time"], "%START_TIME%");
        assert_eq!(decoded_json["method"], "%REQ(:METHOD)%");
        assert_eq!(decoded_json["response_code"], "%RESPONSE_CODE%");
        assert_eq!(decoded_json[":authority"], "%REQ(:AUTHORITY)%");
        assert_eq!(decoded_json["upstream_cluster"], "%UPSTREAM_CLUSTER%");
        assert_eq!(decoded_json["route_name"], "%ROUTE_NAME%");
    }

    /// Test with quoted JSON format (as it might come from some config parsers)
    #[test]
    fn qovery_gateway_class_chart_quoted_json_access_log_format_test() {
        // setup: JSON wrapped in quotes (with escaped quotes inside)
        let json_format =
            r#""{\n  \"start_time\": \"%START_TIME%\",\n  \"method\": \"%REQ(:METHOD)%\"\n}""#.to_string();

        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            Some(json_format),
            HpaMode::Enabled {
                config: Default::default(),
            },
            true,
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify: should properly unwrap the quotes and parse the JSON
        use base64::Engine;

        let public_value = common_chart
            .chart_info
            .values_string
            .iter()
            .find(|v| v.key == "gatewayClass.qoveryPublic.accessLog.format")
            .expect("Public gateway access log format should exist");

        // Decode and verify
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&public_value.value)
            .expect("Should be valid base64");
        let decoded_str = String::from_utf8(decoded).expect("Should be valid UTF-8");
        let decoded_json: serde_json::Value = serde_json::from_str(&decoded_str).expect("Should be valid JSON");

        // Verify the fields are correct
        assert_eq!(decoded_json["start_time"], "%START_TIME%");
        assert_eq!(decoded_json["method"], "%REQ(:METHOD)%");
    }

    /// Test that empty or whitespace-only format is treated as no format
    #[test]
    fn qovery_gateway_class_chart_empty_access_log_format_test() {
        // setup: empty string
        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            Some("   \n  \t  ".to_string()), // whitespace only
            HpaMode::Enabled {
                config: Default::default(),
            },
            true,
        );

        // execute:
        let common_chart = chart.to_common_helm_chart().unwrap();

        // verify: should have empty values (not base64 encoded)
        let public_value = common_chart
            .chart_info
            .values_string
            .iter()
            .find(|v| v.key == "gatewayClass.qoveryPublic.accessLog.format")
            .expect("Public gateway access log format should exist");

        assert_eq!(public_value.value, "", "Empty/whitespace format should result in empty value");
    }

    /// Test that invalid JSON returns an error
    #[test]
    fn qovery_gateway_class_chart_invalid_json_access_log_format_test() {
        // setup: invalid JSON
        let invalid_json = r#"{"time": "%START_TIME%", invalid}"#.to_string();
        let chart = QoveryGatewayClassChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HashSet::new(),
            Some(invalid_json),
            HpaMode::Enabled {
                config: Default::default(),
            },
            true,
        );

        // execute:
        let result = chart.to_common_helm_chart();

        // verify: should return an error
        assert!(result.is_err(), "Invalid JSON should return an error");
        if let Err(e) = result {
            let error_message = format!("{}", e);
            assert!(
                error_message.contains("Invalid JSON format for envoy access log format"),
                "Error message should indicate invalid JSON format, got: {}",
                error_message
            );
        }
    }
}
