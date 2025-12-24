use crate::environment::models::domain::Domain;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::QoveryIdentifier;

pub enum QoveryClusterGatewayOptionsPerKubernetesKind {
    Eks,
    EksSelfManaged,
    EksAnywhere,
    ScwKapsule,
    ScwSelfManaged,
    Gke,
    GkeSelfManaged,
    Aks,
    AksSelfManaged,
    OnPremiseSelfManaged,
}

pub struct QoveryClusterGatewayChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    domain: Domain,
    kubernetes_provider_options: QoveryClusterGatewayOptionsPerKubernetesKind,
    cluster_id: QoveryIdentifier,
    organization_id: QoveryIdentifier,
}

impl QoveryClusterGatewayChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        namespace: HelmChartNamespaces,
        domain: Domain,
        kubernetes_provider_options: QoveryClusterGatewayOptionsPerKubernetesKind,
        cluster_id: QoveryIdentifier,
        organization_id: QoveryIdentifier,
    ) -> Self {
        QoveryClusterGatewayChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryClusterGatewayChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                QoveryClusterGatewayChart::chart_name(),
            ),
            namespace,
            domain,
            kubernetes_provider_options,
            cluster_id,
            organization_id,
        }
    }

    pub fn chart_name() -> String {
        "qovery-cluster-gateway".to_string()
    }
}

impl ToCommonHelmChart for QoveryClusterGatewayChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut chart_set_values = vec![ChartSetValue {
            key: "dns.domain".to_string(),
            value: self.domain.wildcarded().to_string(),
        }];

        match self.kubernetes_provider_options {
            QoveryClusterGatewayOptionsPerKubernetesKind::Eks
            | QoveryClusterGatewayOptionsPerKubernetesKind::EksAnywhere
            | QoveryClusterGatewayOptionsPerKubernetesKind::EksSelfManaged => {
                chart_set_values.push(ChartSetValue {
                    key: "infrastructure.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-name"
                        .to_string(),
                    value: format!("qovery-{}-envoy-gateway", self.cluster_id.short()),
                });

                chart_set_values.push(
                ChartSetValue {
                    key:
                    "infrastructure.annotations.service\\.beta\\.kubernetes\\.io/aws-load-balancer-additional-resource-tags"
                        .to_string(),
                    value: format!(
                        "OrganizationLongId={}\\,OrganizationId={}\\,ClusterLongId={}\\,ClusterId={}",
                        self.organization_id,
                        self.organization_id.short(),
                        self.cluster_id,
                        self.cluster_id.short(),
                    ),
                });
            }
            QoveryClusterGatewayOptionsPerKubernetesKind::Gke
            | QoveryClusterGatewayOptionsPerKubernetesKind::GkeSelfManaged => {
                // No specific values for GKE at the moment
            }
            QoveryClusterGatewayOptionsPerKubernetesKind::Aks
            | QoveryClusterGatewayOptionsPerKubernetesKind::AksSelfManaged => {
                // No specific values for AKS at the moment
            }
            QoveryClusterGatewayOptionsPerKubernetesKind::ScwKapsule
            | QoveryClusterGatewayOptionsPerKubernetesKind::ScwSelfManaged => {
                // No specific values for SCW at the moment
            }
            QoveryClusterGatewayOptionsPerKubernetesKind::OnPremiseSelfManaged => {}
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryClusterGatewayChart::chart_name(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: chart_set_values,
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryClusterGatewayChartInstallationChecker::new())),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct QoveryClusterGatewayChartInstallationChecker {}

impl QoveryClusterGatewayChartInstallationChecker {
    pub fn new() -> Self {
        QoveryClusterGatewayChartInstallationChecker {}
    }
}
impl Default for QoveryClusterGatewayChartInstallationChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartInstallationChecker for QoveryClusterGatewayChartInstallationChecker {
    fn verify_installation(&self, _kube_client: &kube::Client) -> Result<(), CommandError> {
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::models::domain::Domain;
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::qovery_cluster_gateway_chart::{
        QoveryClusterGatewayChart, QoveryClusterGatewayOptionsPerKubernetesKind,
    };
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind;
    use crate::io_models::QoveryIdentifier;
    use std::env;

    fn get_domain() -> Domain {
        Domain::new("qovery.com".to_string())
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_cluster_gateway_chart_directory_exists_test() {
        // setup:
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            QoveryClusterGatewayOptionsPerKubernetesKind::Eks,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            QoveryClusterGatewayChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn qovery_cluster_gateway_chart_values_file_exists_test() {
        // setup:
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            QoveryClusterGatewayOptionsPerKubernetesKind::Eks,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
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
            QoveryClusterGatewayChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn qovery_cluster_gateway_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = QoveryClusterGatewayChart::new(
            None,
            HelmChartNamespaces::Qovery,
            get_domain(),
            QoveryClusterGatewayOptionsPerKubernetesKind::Eks,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
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
                QoveryClusterGatewayChart::chart_name()
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
