use crate::cloud_provider::aws::regions::AwsRegion;
use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartResourcesConstraintType,
    HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::cloud_provider::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::errors::CommandError;
use crate::runtime::block_on;
use chrono::Duration;
use k8s_openapi::api::rbac::v1::RoleBinding;
use kube::core::params::ListParams;
use kube::{Api, Client};

pub struct AwsIamEksUserMapperChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    aws_region: AwsRegion,
    aws_service_account_name: String,
    aws_iam_eks_user_mapper_role_arn: String,
    aws_iam_user_mapper_groups_names_mappings: String,
    refresh_interval: Duration,
    chart_resources: HelmChartResources,
}

impl AwsIamEksUserMapperChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        aws_region: AwsRegion,
        aws_service_account_name: String,
        aws_iam_eks_user_mapper_role_arn: String,
        aws_iam_user_mapper_groups_names_mappings: String, // iam_group1->k8s_group1,iam_group2->k8s_group2,
        refresh_interval: Duration,
        chart_resources: HelmChartResourcesConstraintType,
    ) -> AwsIamEksUserMapperChart {
        AwsIamEksUserMapperChart {
            aws_region,
            aws_service_account_name,
            aws_iam_eks_user_mapper_role_arn,
            aws_iam_user_mapper_groups_names_mappings,
            refresh_interval,
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                AwsIamEksUserMapperChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                AwsIamEksUserMapperChart::chart_name(),
            ),
            chart_resources: match chart_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(10),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(32),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(20),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(32),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
        }
    }

    pub fn chart_name() -> String {
        "iam-eks-user-mapper".to_string()
    }
}

impl ToCommonHelmChart for AwsIamEksUserMapperChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: AwsIamEksUserMapperChart::chart_name(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "aws.defaultRegion".to_string(),
                        value: self.aws_region.to_aws_format().to_string(),
                    },
                    ChartSetValue {
                        key: "aws.roleArn".to_string(),
                        value: self.aws_iam_eks_user_mapper_role_arn.to_string(),
                    },
                    ChartSetValue {
                        // we use string templating (r"...") to escape dot in annotation's key
                        key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: self.aws_iam_eks_user_mapper_role_arn.to_string(),
                    },
                    ChartSetValue {
                        key: "iamK8sGroups".to_string(),
                        value: self
                            .aws_iam_user_mapper_groups_names_mappings
                            .to_string()
                            .replace(',', r"\,"), // commas should be escaped in --set otherwise it's wrongly interpreted by Helm
                    },
                    ChartSetValue {
                        key: "refreshIntervalSeconds".to_string(),
                        value: self.refresh_interval.num_seconds().to_string(),
                    },
                    ChartSetValue {
                        key: "serviceAccount.name".to_string(),
                        value: self.aws_service_account_name.to_string(),
                    },
                    // resources limits
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: self.chart_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: self.chart_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: self.chart_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: self.chart_resources.request_memory.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(AwsIamEksUserMapperChecker::new())),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct AwsIamEksUserMapperChecker {}

impl AwsIamEksUserMapperChecker {
    pub fn new() -> Self {
        AwsIamEksUserMapperChecker {}
    }
}

impl Default for AwsIamEksUserMapperChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartInstallationChecker for AwsIamEksUserMapperChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        // Check user 'iam-eks-user-mapper' is properly set
        let iam_user_mapper_role: Api<RoleBinding> = Api::all(kube_client.clone());

        match block_on(
            iam_user_mapper_role
                .list(&ListParams::default().fields("metadata.name=eks-configmap-modifier-rolebinding")),
        ) {
            Ok(iam_user_mapper_role_result) => {
                if iam_user_mapper_role_result.items.is_empty() {
                    return Err(CommandError::new_from_safe_message(
                        format!("Required role binding `eks-configmap-modifier-role` created by `{}` chart not found, chart is not installed properly.", AwsIamEksUserMapperChart::chart_name()),
                    ));
                }

                for role_binding in iam_user_mapper_role_result.items {
                    // Check if it references the proper role
                    if role_binding.role_ref.name.to_lowercase() != "eks-configmap-modifier-role" {
                        return Err(CommandError::new_from_safe_message(
                            format!("Role binding `eks-configmap-modifier-rolebinding` created by `{}` chart, not installed properly: it should references `eks-configmap-modifier-role` role.", AwsIamEksUserMapperChart::chart_name()),
                        ));
                    }

                    // Check if contains the subject
                    if let Some(subjects) = role_binding.subjects {
                        if !subjects.iter().any(|e| {
                            e.name.to_lowercase() == "iam-eks-user-mapper" && e.kind.to_lowercase() == "serviceaccount"
                        }) {
                            return Err(CommandError::new_from_safe_message(
                                format!("Role binding `eks-configmap-modifier-rolebinding` created by `{}` chart, not installed properly: it should have `iam-eks-user-mapper` subject.", AwsIamEksUserMapperChart::chart_name()),
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                return Err(CommandError::new(
                    "Error trying to get role binding `eks-configmap-modifier-role`".to_string(),
                    Some(e.to_string()),
                    None,
                ));
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
    use crate::cloud_provider::aws::kubernetes::helm_charts::aws_iam_eks_user_mapper_chart::AwsIamEksUserMapperChart;
    use crate::cloud_provider::aws::regions::AwsRegion;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartResourcesConstraintType, HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use chrono::Duration;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn aws_iam_eks_user_mapper_chart_directory_exists_test() {
        // setup:
        let chart = AwsIamEksUserMapperChart::new(
            None,
            AwsRegion::EuWest3,
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
            Duration::seconds(30),
            HelmChartResourcesConstraintType::ChartDefault,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            AwsIamEksUserMapperChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn aws_iam_eks_user_mapper_chart_values_file_exists_test() {
        // setup:
        let chart = AwsIamEksUserMapperChart::new(
            None,
            AwsRegion::EuWest3,
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
            Duration::seconds(30),
            HelmChartResourcesConstraintType::ChartDefault,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            AwsIamEksUserMapperChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn aws_iam_eks_user_mapper_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = AwsIamEksUserMapperChart::new(
            None,
            AwsRegion::EuWest3,
            "whatever".to_string(),
            "whatever".to_string(),
            "whatever".to_string(),
            Duration::seconds(30),
            HelmChartResourcesConstraintType::ChartDefault,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
                ),
                AwsIamEksUserMapperChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}
