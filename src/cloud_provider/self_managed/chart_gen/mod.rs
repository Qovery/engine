use derive_more::Display;
use semver::Version;
use std::path::Path;
use url::Url;

use crate::cloud_provider::kubernetes::KubernetesVersion;

use self::chart_dot_yaml::{ChartDotYamlApiVersion, ChartDotYamlType};
use self::values_dot_yaml::ChartCategory;

pub mod chart_dot_yaml;
pub mod io;
pub mod values_dot_yaml;

#[derive(Clone)]
pub struct QoverySelfManagedChart<'a> {
    #[allow(dead_code)]
    destination: &'a Path,
    name: String,
    description: String,
    api_version: ChartDotYamlApiVersion,
    r#type: ChartDotYamlType,
    version: Version,
    app_version: Version,
    kube_version: KubernetesVersion,
    home: Url,
    icon: Url,
    charts_source_path: Vec<ChartMeta>,
}

impl<'a> QoverySelfManagedChart<'a> {
    pub fn new(
        destination: &'a Path,
        name: String,
        description: String,
        api_version: ChartDotYamlApiVersion,
        r#type: ChartDotYamlType,
        version: Version,
        app_version: Version,
        kube_version: KubernetesVersion,
        home: Url,
        icon: Url,
        charts_source_path: Vec<ChartMeta>,
    ) -> QoverySelfManagedChart<'a> {
        QoverySelfManagedChart {
            destination,
            name,
            description,
            api_version,
            r#type,
            version,
            app_version,
            kube_version,
            home,
            icon,
            charts_source_path,
        }
    }
}

#[derive(Clone, Display, Debug)]
#[display("{} {} {} {}", "name", "category", "source_path", "values_source_path")]
pub struct ChartMeta {
    name: SupportedCharts,
    category: ChartCategory,
    source_path: ChartSourcePath,
    #[allow(dead_code)]
    values_source_path: Option<ValuesSourcePath>,
}

#[derive(Clone, Display, Debug)]
pub enum ChartSourcePath {
    #[display("lib/aws/bootstrap/charts")]
    AwsBootstrapCharts,
    #[display("lib/aws-ec2/bootstrap/charts")]
    AwsEC2BootstrapCharts,
    #[display("lib/common/bootstrap/charts")]
    CommonBoostrapCharts,
    #[display("lib/common/charts")]
    CommonCharts,
    #[display("lib/gcp/bootstrap/charts")]
    GcpBootstrapCharts,
    #[display("lib/scaleway/bootstrap/charts")]
    ScalewayBootstrapCharts,
}

#[derive(Clone, Display, Debug)]
pub enum ValuesSourcePath {
    #[display("lib/aws/bootstrap/chart_values")]
    AwsBootstrapChartValues,
    #[display("lib/aws-ec2/bootstrap/chart_values")]
    AwsEC2BootstrapChartValues,
    #[display("lib/common/bootstrap/chart_values")]
    CommonBoostrapChartValues,
    #[display("lib/gcp/bootstrap/chart_values")]
    GcpBootstrapChartValues,
    #[display("lib/scaleway/bootstrap/chart_values")]
    ScalewayBootstrapChartValues,
}

#[derive(Clone, Display, Debug)]
pub enum SupportedCharts {
    #[display("q-storageclass")]
    QoveryAwsStorageClass,
    #[display("ingress-nginx")]
    IngressNginx,
    #[display("external-dns")]
    ExternalDNS,
    #[display("promtail")]
    Promtail,
    #[display("loki")]
    Loki,
    #[display("cert-manager")]
    CertManager,
    #[display("cert-manager-configs")]
    CertManagerConfigs,
    #[display("qovery-cert-manager-webhook")]
    CertManagerQoveryWebhook,
    #[display("metrics-server")]
    MetricsServer,
    #[display("qovery-cluster-agent")]
    QoveryClusterAgent,
    #[display("qovery-shell-agent")]
    QoveryShellAgent,
    #[display("qovery-engine")]
    QoveryEngine,
    #[display("q-storageclass")]
    QoveryStorageClass,
}

#[cfg(test)]
mod tests {
    use std::{fs, io, path::Path};

    use regex::Regex;

    use super::ChartMeta;

    pub fn copy_recursively(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> io::Result<()> {
        fs::create_dir_all(&destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let filetype = entry.file_type()?;
            if filetype.is_dir() {
                copy_recursively(entry.path(), destination.as_ref().join(entry.file_name()))?;
            } else {
                fs::copy(entry.path(), destination.as_ref().join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    pub fn override_values(
        mut values_file_content: String,
        charts_source_path: Vec<ChartMeta>,
        prefix: String,
    ) -> String {
        // chart override content
        for chart in charts_source_path {
            let string_to_replace = match chart.values_source_path.clone() {
                None => "".to_string(),
                Some(x) => {
                    let mut override_values_file_path = None;

                    let override_values_file_path_with_jinja =
                        format!("{}/{}/{}.j2.yaml", prefix.clone(), x, chart.name);
                    if fs::metadata(&override_values_file_path_with_jinja).is_ok() {
                        override_values_file_path = Some(override_values_file_path_with_jinja)
                    } else {
                        let override_values_file_path_without_jinja =
                            format!("{}/{}/{}.yaml", prefix.clone(), x, chart.name);
                        if fs::metadata(&override_values_file_path_without_jinja).is_ok() {
                            override_values_file_path = Some(override_values_file_path_without_jinja)
                        }
                    }

                    let file = match override_values_file_path {
                        None => {
                            panic!(
                                "for values.yaml, parsing: No file found (j2 or yaml) for chart {}. Debug info: {:?}",
                                chart.name, &chart
                            )
                        }
                        Some(x) => {
                            println!("for values.yaml, parsing: {x}");
                            x
                        }
                    };

                    let override_values = fs::read_to_string(file).unwrap();
                    let replace_values = override_values
                        // add Yaml indentation to validate Yaml
                        .replace('\n', "\n  ")
                        // replace "set-by-engine-code" by "set-by-customer"
                        .replace("set-by-engine-code", "set-by-customer");
                    // replace jinja vars by "set-by-customer"
                    let replace_jinja_vars = Regex::new(r"\{\{.*\}\}").unwrap();
                    let no_jinja_vars = replace_jinja_vars
                        .replace_all(replace_values.as_str(), "set-by-customer")
                        .to_string();
                    // remove empty lines
                    let remove_empty_lines = Regex::new(r"\n\s*\n").unwrap();
                    remove_empty_lines.replace_all(no_jinja_vars.as_str(), "\n").to_string()
                }
            };

            // apply chart override content
            values_file_content = values_file_content.replace(
                format!("override_chart: {}", chart.name).as_str(),
                // use format to have a new line
                &string_to_replace,
            );
            // update Qovery config to use YAML pointers
            let update_qovery_config = Regex::new(r"'(\&|\*)(.+)'").unwrap();
            values_file_content = update_qovery_config
                .replace_all(values_file_content.as_str(), "$1$2")
                .to_string();
        }
        values_file_content
    }

    #[test]
    #[ignore]
    #[cfg(feature = "test-local-kube")]
    pub fn generate_helm_chart() {
        use semver::Version;
        use url::Url;

        use crate::cloud_provider::{
            kubernetes::KubernetesVersion,
            self_managed::chart_gen::{
                chart_dot_yaml::{ChartDotYamlApiVersion, ChartDotYamlType},
                io::ChartDotYaml,
                values_dot_yaml::ValuesFile,
                ChartCategory, ChartMeta, ChartSourcePath, SupportedCharts, ValuesSourcePath,
            },
        };

        use super::QoverySelfManagedChart;
        use std::{fs, path::Path, process::Command};

        // create chart directories
        let prefix = std::env::var("WORKSPACE_ROOT_DIR").unwrap();
        let qovery_chart_path = format!("{}/.qovery-workspace/qovery_chart", &prefix);
        fs::create_dir_all(&qovery_chart_path).unwrap();
        let qovery_chart_templates_path = format!("{}/templates", &qovery_chart_path);
        fs::create_dir_all(qovery_chart_templates_path).unwrap();

        // define Chart.yaml content without dependencies (added later for each cloud providers)
        let minimal_qovery_chart = QoverySelfManagedChart::new(
            Path::new(&qovery_chart_path),
            "qovery".to_string(),
            "Qovery Helm chart - self managed version".to_string(),
            ChartDotYamlApiVersion::V2,
            ChartDotYamlType::Application,
            Version::new(1, 0, 0),
            Version::new(1, 0, 0),
            KubernetesVersion::V1_26 {
                prefix: None,
                patch: None,
                suffix: None,
            },
            Url::parse("https://www.qovery.com").expect("failed to parse Qovery url"),
            Url::parse("https://raw.githubusercontent.com/Qovery/public-resources/master/qovery_square_new_logo.svg")
                .expect("failed to parse Qovery logo url"),
            vec![],
        );

        // AWS define QoverySelfManagedChart with desired charts and override values to add
        // Warning: order is important, you may have replacement issues. Example:
        // cert-manager and cert-manager-configs. Cert-manager-configs should be set before cert-manager
        let mut aws_qovery_chart = minimal_qovery_chart.clone();
        aws_qovery_chart.charts_source_path = vec![
            // ChartMeta {
            //     name: SupportedCharts::QoveryAwsStorageClass,
            //     category: ChartCategory::Aws,
            //     source_path: ChartSourcePath::AwsBootstrapCharts,
            //     values_source_path: None,
            // },
            ChartMeta {
                name: SupportedCharts::IngressNginx,
                category: ChartCategory::Ingress,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::AwsBootstrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::ExternalDNS,
                category: ChartCategory::DNS,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::Promtail,
                category: ChartCategory::Logging,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::Loki,
                category: ChartCategory::Logging,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManagerConfigs,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManager,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManagerQoveryWebhook,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::MetricsServer,
                category: ChartCategory::Observability,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::QoveryClusterAgent,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::QoveryShellAgent,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: None,
            },
            ChartMeta {
                name: SupportedCharts::QoveryEngine,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: None,
            },
        ];

        let mut gcp_qovery_chart = minimal_qovery_chart.clone();
        gcp_qovery_chart.charts_source_path = vec![
            // ChartMeta {
            //     name: SupportedCharts::QoveryAwsStorageClass,
            //     category: ChartCategory::Gcp,
            //     source_path: ChartSourcePath::GcpBootstrapCharts,
            //     values_source_path: None,
            // },
            ChartMeta {
                name: SupportedCharts::IngressNginx,
                category: ChartCategory::Ingress,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::GcpBootstrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::ExternalDNS,
                category: ChartCategory::DNS,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::Promtail,
                category: ChartCategory::Logging,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::Loki,
                category: ChartCategory::Logging,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManagerConfigs,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManager,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManagerQoveryWebhook,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::QoveryClusterAgent,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::QoveryShellAgent,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: None,
            },
            ChartMeta {
                name: SupportedCharts::QoveryEngine,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: None,
            },
        ];

        let mut scaleway_qovery_chart = minimal_qovery_chart.clone();
        scaleway_qovery_chart.charts_source_path = vec![
            // ChartMeta {
            //     name: SupportedCharts::QoveryAwsStorageClass,
            //     category: ChartCategory::Gcp,
            //     source_path: ChartSourcePath::GcpBootstrapCharts,
            //     values_source_path: None,
            // },
            ChartMeta {
                name: SupportedCharts::IngressNginx,
                category: ChartCategory::Ingress,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::ScalewayBootstrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::ExternalDNS,
                category: ChartCategory::DNS,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::Promtail,
                category: ChartCategory::Logging,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::Loki,
                category: ChartCategory::Logging,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManagerConfigs,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManager,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::CertManagerQoveryWebhook,
                category: ChartCategory::Certificates,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::QoveryClusterAgent,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: Some(ValuesSourcePath::CommonBoostrapChartValues),
            },
            ChartMeta {
                name: SupportedCharts::QoveryShellAgent,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: None,
            },
            ChartMeta {
                name: SupportedCharts::QoveryEngine,
                category: ChartCategory::Qovery,
                source_path: ChartSourcePath::CommonBoostrapCharts,
                values_source_path: None,
            },
        ];

        // generate values.yaml
        let values = ValuesFile::new_minimal();
        values
            .save_to_file(minimal_qovery_chart.destination, "values.yaml".to_string())
            .expect("failed to save values.yaml");

        // generate values-aws.yaml
        let values = ValuesFile::new_aws();
        values
            .save_to_file(aws_qovery_chart.destination, "values-aws.yaml".to_string())
            .expect("failed to save values-aws.yaml");

        // add overrided values to values-aws.yaml
        let values_file_path = format!("{}/values-aws.yaml", aws_qovery_chart.destination.to_string_lossy());
        let mut values_file_content = fs::read_to_string(Path::new(&values_file_path)).unwrap();
        values_file_content =
            override_values(values_file_content, aws_qovery_chart.charts_source_path.clone(), prefix.clone());
        fs::write(values_file_path, values_file_content).unwrap();

        // generate values-gcp.yaml
        let values = ValuesFile::new_gcp();
        values
            .save_to_file(aws_qovery_chart.destination, "values-gcp.yaml".to_string())
            .expect("failed to save values-gcp.yaml");
        // add overrided values to values-gcp.yaml
        let values_file_path = format!("{}/values-gcp.yaml", gcp_qovery_chart.destination.to_string_lossy());
        let mut values_file_content = fs::read_to_string(Path::new(&values_file_path)).unwrap();
        values_file_content =
            override_values(values_file_content, gcp_qovery_chart.charts_source_path.clone(), prefix.clone());
        fs::write(values_file_path, values_file_content).unwrap();

        // generate values-scaleway.yaml
        let values = ValuesFile::new_scaleway();
        values
            .save_to_file(aws_qovery_chart.destination, "values-scaleway.yaml".to_string())
            .expect("failed to save values-scaleway.yaml");
        // add overrided values to values-gcp.yaml
        let values_file_path = format!("{}/values-scaleway.yaml", scaleway_qovery_chart.destination.to_string_lossy());
        let mut values_file_content = fs::read_to_string(Path::new(&values_file_path)).unwrap();
        values_file_content = override_values(
            values_file_content,
            scaleway_qovery_chart.charts_source_path.clone(),
            prefix.clone(),
        );
        fs::write(values_file_path, values_file_content).unwrap();

        // generate Chart.yaml
        // TODO(pmavro): support not only AWS but also GCP, Scaleway, etc. in the dependencies
        let chart_dot_yaml = ChartDotYaml::from_qovery_self_managed_chart(prefix.clone(), aws_qovery_chart.clone())
            .map_err(|e| {
                println!("{e}");
            })
            .expect("failed to generate Chart.yaml");
        chart_dot_yaml
            .save_to_file(aws_qovery_chart.destination)
            .expect("failed to save Chart.yaml");

        // copy charts
        for chart in aws_qovery_chart.charts_source_path {
            let source_path = format!("{}/{}", prefix.clone(), chart.source_path);
            let destination_path = format!("{}/charts", aws_qovery_chart.destination.to_string_lossy());
            fs::create_dir_all(&destination_path).unwrap();
            let src = format!("{}/{}", source_path, chart.name);
            let dst = format!("{destination_path}/{}", chart.name);
            println!("copying {src} to {dst}");
            // fs::copy(src, dst).unwrap();
            copy_recursively(src, dst).unwrap();
        }

        // helm lint generated chart
        match Command::new("helm")
            .args(["lint", &aws_qovery_chart.destination.to_string_lossy()])
            .spawn()
        {
            Ok(_) => println!("helm lint ok"),
            Err(e) => panic!("helm lint failed: {e}"),
        }
    }
}
