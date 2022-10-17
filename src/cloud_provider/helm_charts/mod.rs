use crate::cloud_provider::helm::CommonChart;
use std::fmt::{Display, Formatter};

pub mod cluster_autoscaler_chart;
pub mod core_dns_config_chart;
pub mod external_dns_chart;
pub mod qovery_storage_class_chart;

/// Represents chart directory where chart is defined.
pub struct HelmChartPath {
    path: String,
}

impl HelmChartPath {
    pub fn new(
        path_prefix: Option<&str>,
        directory_location: HelmChartDirectoryLocation,
        chart_name: String,
    ) -> HelmChartPath {
        let mut path = format!(
            "{}{}/charts/{}",
            path_prefix.unwrap_or("."),
            match directory_location {
                HelmChartDirectoryLocation::CommonFolder => "/common",
                HelmChartDirectoryLocation::CloudProviderFolder => "/",
            },
            chart_name,
        );

        // TODO(benjaminch: Find a more elegant way to remove consecutives /.
        while path.contains("//") {
            path = path.replace("//", "/");
        }

        HelmChartPath { path }
    }
}

impl Display for HelmChartPath {
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
