use crate::cloud_provider::helm::CommonChart;
use std::env;
use std::fmt::{Display, Formatter};

pub mod cluster_autoscaler_chart;
pub mod core_dns_config_chart;
pub mod external_dns_chart;
pub mod kube_prometheus_stack_chart;
pub mod loki_chart;
pub mod prometheus_adapter_chart;
pub mod promtail_chart;
pub mod qovery_storage_class_chart;

pub struct HelmChartPath {
    path: HelmPath,
}

impl HelmChartPath {
    pub fn new(
        path_prefix: Option<&str>,
        directory_location: HelmChartDirectoryLocation,
        chart_name: String,
    ) -> HelmChartPath {
        HelmChartPath {
            path: HelmPath::new(HelmPathType::Chart, path_prefix, directory_location, chart_name),
        }
    }
}

impl Display for HelmChartPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.path.to_string().as_str())
    }
}

pub struct HelmChartValuesFilePath {
    path: HelmPath,
}

impl HelmChartValuesFilePath {
    pub fn new(
        path_prefix: Option<&str>,
        directory_location: HelmChartDirectoryLocation,
        chart_name: String,
    ) -> HelmChartValuesFilePath {
        HelmChartValuesFilePath {
            path: HelmPath::new(HelmPathType::ValuesFile, path_prefix, directory_location, chart_name),
        }
    }
}

impl Display for HelmChartValuesFilePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.path.to_string().as_str())
    }
}

enum HelmPathType {
    ValuesFile,
    Chart,
}

/// Represents chart directory where chart is defined.
struct HelmPath {
    path: String,
}

impl HelmPath {
    fn new(
        helm_path_type: HelmPathType,
        path_prefix: Option<&str>,
        directory_location: HelmChartDirectoryLocation,
        chart_name: String,
    ) -> HelmPath {
        let mut path = format!(
            "{prefix}{directory}/{helm_path_type}/{name}{extension}",
            prefix = path_prefix.unwrap_or("."),
            directory = match directory_location {
                HelmChartDirectoryLocation::CommonFolder => "/common",
                HelmChartDirectoryLocation::CloudProviderFolder => "/",
            },
            helm_path_type = match helm_path_type {
                HelmPathType::ValuesFile => "chart_values",
                HelmPathType::Chart => "charts",
            },
            name = chart_name,
            extension = match helm_path_type {
                HelmPathType::ValuesFile => ".yaml",
                HelmPathType::Chart => "",
            }
        );

        // TODO(benjaminch: Find a more elegant way to remove consecutives /.
        while path.contains("//") {
            path = path.replace("//", "/");
        }

        HelmPath { path }
    }
}

impl Display for HelmPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.path.as_str())
    }
}

/// Represents where chart is supposed to be taken from (specific for provider or shared).
pub enum HelmChartDirectoryLocation {
    CommonFolder,
    CloudProviderFolder,
}

pub trait ToCommonHelmChart {
    fn to_common_helm_chart(&self) -> CommonChart;
}

pub fn get_helm_values_set_in_code_but_absent_in_values_file(
    chart_common_chart: CommonChart,
    values_file_lib_path: String,
) -> Option<Vec<String>> {
    let current_directory = env::current_dir().expect("Impossible to get current directory");
    let chart_values_path = format!(
        "{}/{}",
        current_directory
            .to_str()
            .expect("Impossible to convert current directory to string"),
        values_file_lib_path,
    );

    let f = std::fs::File::open(&chart_values_path)
        .unwrap_or_else(|_| panic!("Impossible to open chart values file: `{}`", chart_values_path));
    let data: serde_yaml::Value =
        serde_yaml::from_reader(f).unwrap_or_else(|_| panic!("Impossible to parse YAML file: `{}`", chart_values_path));

    let mut missing_fields = vec![];

    for value in chart_common_chart.chart_info.values {
        // Check that value declared in rust code exists in the YAML values file
        if let serde_yaml::Value::Mapping(ref m) = data {
            // Black magic allowing to keep only fields before array indexes
            let fields_raw: String = value
                .key
                .to_string()
                .chars()
                .take_while(|&ch| ch != '[')
                .collect::<String>();
            let fields = fields_raw.split('.').collect::<Vec<&str>>();
            let fields_len = fields.len();

            let mut current_value = m;

            for (i, f) in fields.iter().enumerate() {
                if !current_value.contains_key(f) {
                    missing_fields.push(value.key.to_string());
                }

                if i < fields_len - 1 {
                    current_value = current_value[f]
                        .as_mapping()
                        .expect("Error while trying to get nested field");
                }
            }
        }
    }

    match missing_fields.is_empty() {
        true => None,
        false => Some(missing_fields),
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath};

    #[test]
    fn test_helm_chart_path_to_string() {
        // setup:
        struct TestCase {
            input: HelmChartPath,
            expected_path: String,
        }

        let test_cases = vec![
            TestCase {
                input: HelmChartPath::new(None, HelmChartDirectoryLocation::CommonFolder, "yolo".to_string()),
                expected_path: "./common/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/1234567/"),
                    HelmChartDirectoryLocation::CommonFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/1234567/common/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/1234567"),
                    HelmChartDirectoryLocation::CommonFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/1234567/common/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(None, HelmChartDirectoryLocation::CloudProviderFolder, "yolo".to_string()),
                expected_path: "./charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/79087349856/"),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/79087349856/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/79087349856"),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/79087349856/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/////79087349856/"),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/79087349856/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("./"),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "./charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("."),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "./charts/yolo".to_string(),
            },
        ];

        for tc in test_cases {
            // execute:
            let res = tc.input.to_string();

            // verify:
            assert_eq!(tc.expected_path, res)
        }
    }
}
