use semver::Version;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartDotYamlApiVersion {
    V1,
    V2,
}

#[derive(Debug, PartialEq)]
pub struct ChartDotYamlDependencies {
    pub name: String,
    pub alias: Option<String>,
    pub condition: String,
    pub version: Version,
    pub repository: String,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ChartDotYamlType {
    Application,
}

#[derive(Debug, PartialEq)]
pub struct ChartDotYaml {
    pub api_version: ChartDotYamlApiVersion,
    pub name: String,
    pub description: String,
    pub dependencies: Option<Vec<ChartDotYamlDependencies>>,
    pub r#type: Option<ChartDotYamlType>,
    pub version: Version,
    pub app_version: Version,
    pub kube_version: Option<String>,
    pub home: Option<String>,
    pub icon: Option<String>,
}

impl ChartDotYaml {
    pub fn new(
        api_version: ChartDotYamlApiVersion,
        name: String,
        description: String,
        dependencies: Option<Vec<ChartDotYamlDependencies>>,
        r#type: Option<ChartDotYamlType>,
        version: Version,
        app_version: Version,
        kube_version: Option<String>,
        home: Option<String>,
        icon: Option<String>,
    ) -> ChartDotYaml {
        ChartDotYaml {
            api_version,
            name,
            description,
            dependencies,
            r#type,
            version,
            app_version,
            kube_version,
            home,
            icon,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::self_managed::chart_gen::io;

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn test_chart_dot_yaml_generation() {
        use super::ChartDotYaml;
        use crate::cloud_provider::{
            kubernetes::KubernetesVersion,
            self_managed::chart_gen::chart_dot_yaml::{
                ChartDotYamlApiVersion, ChartDotYamlDependencies, ChartDotYamlType,
            },
        };
        use semver::Version;

        // Without dependencies
        let yaml_content = r#"
apiVersion: v2
name: the_name
description: desc
type: application
version: 1.0.0
appVersion: 2.0.0
kubeVersion: ~1.26.0-0
home: https://www.qovery.com
icon: https://raw.githubusercontent.com/Qovery/public-resources/master/qovery_square_new_logo.svg
"#
        .strip_prefix('\n')
        .unwrap();
        let chart_without_dep = ChartDotYaml::new(
            ChartDotYamlApiVersion::V2,
            "the_name".to_string(),
            "desc".to_string(),
            None,
            Some(ChartDotYamlType::Application),
            Version::new(1, 0, 0),
            Version::new(2, 0, 0),
            Some(
                KubernetesVersion::V1_26 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                }
                .to_string(),
            ),
            Some("https://www.qovery.com".to_string()),
            Some(
                "https://raw.githubusercontent.com/Qovery/public-resources/master/qovery_square_new_logo.svg"
                    .to_string(),
            ),
        );
        let chart_generated = io::ChartDotYaml::from_model(chart_without_dep).to_yaml().unwrap();
        assert_eq!(yaml_content, chart_generated);

        // With dependencies
        let yaml_content = r#"
apiVersion: v2
name: the_name
description: desc
dependencies:
- name: minio
  condition: services.minio.enabled
  version: 4.0.12
  repository: https://charts.min.io/
- name: grafana-agent-operator
  alias: grafana-agent-operator
  condition: services.monitoring.selfMonitoring.grafanaAgent.installOperator
  version: 0.2.3
  repository: https://grafana.github.io/helm-charts
type: application
version: 1.0.0
appVersion: 2.0.0
kubeVersion: ~1.26.0-0
home: https://www.qovery.com
icon: https://raw.githubusercontent.com/Qovery/public-resources/master/qovery_square_new_logo.svg
"#
        .strip_prefix('\n')
        .unwrap();
        let chart_without_dep = ChartDotYaml::new(
            ChartDotYamlApiVersion::V2,
            "the_name".to_string(),
            "desc".to_string(),
            Some(vec![
                ChartDotYamlDependencies {
                    name: "minio".to_string(),
                    alias: None,
                    condition: "services.minio.enabled".to_string(),
                    repository: "https://charts.min.io/".to_string(),
                    version: Version::new(4, 0, 12),
                },
                ChartDotYamlDependencies {
                    name: "grafana-agent-operator".to_string(),
                    alias: Some("grafana-agent-operator".to_string()),
                    condition: "services.monitoring.selfMonitoring.grafanaAgent.installOperator".to_string(),
                    repository: "https://grafana.github.io/helm-charts".to_string(),
                    version: Version::new(0, 2, 3),
                },
            ]),
            Some(ChartDotYamlType::Application),
            Version::new(1, 0, 0),
            Version::new(2, 0, 0),
            Some(
                KubernetesVersion::V1_26 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                }
                .to_string(),
            ),
            Some("https://www.qovery.com".to_string()),
            Some(
                "https://raw.githubusercontent.com/Qovery/public-resources/master/qovery_square_new_logo.svg"
                    .to_string(),
            ),
        );
        let chart_generated = io::ChartDotYaml::from_model(chart_without_dep).to_yaml().unwrap();
        assert_eq!(yaml_content, chart_generated);
    }
}
