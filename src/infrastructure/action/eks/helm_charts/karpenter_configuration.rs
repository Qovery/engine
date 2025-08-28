use crate::environment::models::domain::ToHelmString;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::kubernetes::KubernetesVersion;
use crate::infrastructure::models::kubernetes::aws::UserNetworkConfig;
use crate::infrastructure::models::kubernetes::karpenter::{
    KarpenterNodePool, KarpenterNodePoolRequirement, KarpenterNodePoolRequirementKey, KarpenterParameters,
    KarpenterRequirementOperator,
};
use itertools::Itertools;
use kube::Client;

pub struct KarpenterConfigurationChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    cluster_name: String,
    replace_cluster_autoscaler: bool,
    security_group_id: String,
    cluster_id: String,
    cluster_long_id: String,
    organization_id: String,
    organization_long_id: String,
    kubernetes_version: KubernetesVersion,
    region: String,
    karpenter_parameters: KarpenterParameters,
    explicit_subnet_ids: Vec<String>,
    pleco_resources_ttl: i32,
}

impl KarpenterConfigurationChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        cluster_name: String,
        replace_cluster_autoscaler: bool,
        cluster_security_group_id: String,
        cluster_id: &str,
        cluster_long_id: uuid::Uuid,
        organization_id: &str,
        organization_long_id: uuid::Uuid,
        kubernetes_version: KubernetesVersion,
        region: &str,
        karpenter_parameters: KarpenterParameters,
        user_network_config: Option<&UserNetworkConfig>,
        pleco_resources_ttl: i32,
    ) -> Self {
        KarpenterConfigurationChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterConfigurationChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterConfigurationChart::chart_name(),
            ),
            cluster_name,
            replace_cluster_autoscaler,
            security_group_id: cluster_security_group_id,
            cluster_id: cluster_id.to_string(),
            cluster_long_id: cluster_long_id.to_string(),
            organization_id: organization_id.to_string(),
            organization_long_id: organization_long_id.to_string(),
            kubernetes_version,
            region: region.to_string(),
            karpenter_parameters,
            explicit_subnet_ids: if let Some(user_network_config) = &user_network_config {
                [
                    &user_network_config.eks_subnets_zone_a_ids,
                    &user_network_config.eks_subnets_zone_b_ids,
                    &user_network_config.eks_subnets_zone_c_ids,
                ]
                .iter()
                .flat_map(|v| v.iter())
                .cloned()
                .collect_vec()
            } else {
                vec![]
            },
            pleco_resources_ttl,
        }
    }

    pub fn chart_name() -> String {
        "karpenter-configuration".to_string()
    }

    fn enrich_karpenter_requirements(
        spot_enabled: bool,
        qovery_node_pools: KarpenterNodePool,
    ) -> Vec<KarpenterNodePoolRequirement> {
        let mut requirements = qovery_node_pools.requirements;
        requirements.push(KarpenterNodePoolRequirement {
            key: KarpenterNodePoolRequirementKey::CapacityType,
            operator: Some(KarpenterRequirementOperator::In),
            values: if spot_enabled {
                vec!["spot".to_string(), "on-demand".to_string()]
            } else {
                vec!["on-demand".to_string()]
            },
        });

        requirements
    }
}

impl ToCommonHelmChart for KarpenterConfigurationChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values = vec![
            ChartSetValue {
                key: "clusterName".to_string(),
                value: self.cluster_name.clone(),
            },
            ChartSetValue {
                key: "securityGroupId".to_string(),
                value: self.security_group_id.clone(),
            },
            ChartSetValue {
                key: "amiSelectorTermsAlias".to_string(),
                value: match self.kubernetes_version {
                    KubernetesVersion::V1_23 { .. }
                    | KubernetesVersion::V1_24 { .. }
                    | KubernetesVersion::V1_25 { .. }
                    | KubernetesVersion::V1_26 { .. }
                    | KubernetesVersion::V1_27 { .. }
                    | KubernetesVersion::V1_28 { .. }
                    | KubernetesVersion::V1_29 { .. }
                    | KubernetesVersion::V1_30 { .. }
                    | KubernetesVersion::V1_31 { .. }
                    | KubernetesVersion::V1_32 { .. } => "al2@latest",
                    KubernetesVersion::V1_33 { .. } => "al2023@latest",
                }
                .to_string(),
            },
            ChartSetValue {
                key: "diskSizeInGib".to_string(),
                value: format!("{}Gi", self.karpenter_parameters.disk_size_in_gib),
            },
            ChartSetValue {
                key: "tags.ClusterId".to_string(),
                value: self.cluster_id.clone(),
            },
            ChartSetValue {
                key: "tags.ClusterLongId".to_string(),
                value: self.cluster_long_id.clone(),
            },
            ChartSetValue {
                key: "tags.OrganizationId".to_string(),
                value: self.organization_id.clone(),
            },
            ChartSetValue {
                key: "tags.OrganizationLongId".to_string(),
                value: self.organization_long_id.clone(),
            },
            ChartSetValue {
                key: "tags.Region".to_string(),
                value: self.region.clone(),
            },
        ];

        if !self.explicit_subnet_ids.is_empty() {
            values.push(ChartSetValue {
                key: "explicitSubnetIds".to_string(),
                value: format!("{{{}}}", self.explicit_subnet_ids.join(",")),
            });
        }

        let karpenter_node_pools_requirements = Self::enrich_karpenter_requirements(
            self.karpenter_parameters.spot_enabled,
            self.karpenter_parameters.qovery_node_pools.clone(),
        );

        karpenter_node_pools_requirements
            .iter()
            .enumerate()
            .for_each(|(index, requirement)| {
                let prefix = format!("global_node_pools.requirements[{index}]");

                let formated_values = if requirement.key == KarpenterNodePoolRequirementKey::Arch {
                    // The nodepool support only lowercase value for arch
                    requirement.values.iter().map(|value| value.to_lowercase()).join(",")
                } else {
                    requirement.values.join(",")
                };

                values.push(ChartSetValue {
                    key: format!("{prefix}.key"),
                    value: requirement.key.to_k8s_label(),
                });
                values.push(ChartSetValue {
                    key: format!("{prefix}.operator"),
                    value: requirement
                        .operator
                        .as_ref()
                        .unwrap_or(&KarpenterRequirementOperator::In)
                        .to_string(),
                });
                values.push(ChartSetValue {
                    key: format!("{prefix}.values"),
                    value: format!("{{{formated_values}}}"),
                });
            });

        // Stable node pool consolidation
        let stable_pool_override = self.karpenter_parameters.qovery_node_pools.stable_override.clone();
        stable_pool_override.budgets.iter().enumerate().for_each(|(index, it)| {
            let prefix = format!("stableNodePool.consolidation.budgets[{index}]");

            values.push(ChartSetValue {
                key: format!("{prefix}.nodes"),
                value: it.nodes.to_string(),
            });
            values.push(ChartSetValue {
                key: format!("{prefix}.reasons"),
                value: it.reasons.to_helm_format_string().to_string(),
            });
            values.push(ChartSetValue {
                key: format!("{prefix}.duration"),
                value: it.get_karpenter_budget_duration_as_string(),
            });
            values.push(ChartSetValue {
                key: format!("{prefix}.schedule"),
                value: it.schedule.to_string(),
            });
        });

        // Stable node pool limits
        if let Some(limits) = &stable_pool_override.limits {
            values.push(ChartSetValue {
                key: "stableNodePool.limits.maxCpu".to_string(),
                value: limits.max_cpu.to_string(),
            });
            values.push(ChartSetValue {
                key: "stableNodePool.limits.maxMemory".to_string(),
                value: limits.max_memory.to_string(),
            });
        }

        // Default node pool limits
        if let Some(Some(default_node_pool_limits)) = self
            .karpenter_parameters
            .qovery_node_pools
            .default_override
            .clone()
            .map(|default_override| default_override.limits)
        {
            values.push(ChartSetValue {
                key: "defaultNodePool.limits.maxCpu".to_string(),
                value: default_node_pool_limits.max_cpu.to_string(),
            });
            values.push(ChartSetValue {
                key: "defaultNodePool.limits.maxMemory".to_string(),
                value: default_node_pool_limits.max_memory.to_string(),
            });
        }

        let mut values_string: Vec<ChartSetValue> = vec![];
        if self.pleco_resources_ttl > 0 {
            values_string.push(ChartSetValue {
                key: "tags.ttl".to_string(),
                value: format!("\"{}\"", self.pleco_resources_ttl),
            });
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KarpenterConfigurationChart::chart_name(),
                action: match self.replace_cluster_autoscaler {
                    true => HelmAction::Deploy,
                    false => HelmAction::Destroy,
                },
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values,
                values_string,
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KarpenterChartChecker::new())),
            vertical_pod_autoscaler: None, // enabled in the chart configuration
        })
    }
}

#[derive(Clone)]
pub struct KarpenterChartChecker {}

impl KarpenterChartChecker {
    pub fn new() -> KarpenterChartChecker {
        KarpenterChartChecker {}
    }
}

impl Default for KarpenterChartChecker {
    fn default() -> Self {
        KarpenterChartChecker::new()
    }
}

impl ChartInstallationChecker for KarpenterChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1366): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use serde::Deserialize;
    use serde_yaml::{self, Value};
    use std::env;
    use uuid::Uuid;

    use crate::cmd::helm::Helm;
    use crate::infrastructure::action::eks::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::karpenter::{
        KarpenterDefaultNodePoolOverride, KarpenterNodePool, KarpenterNodePoolDisruptionBudget,
        KarpenterNodePoolDisruptionReason, KarpenterNodePoolLimits, KarpenterNodePoolRequirement,
        KarpenterNodePoolRequirementKey, KarpenterParameters, KarpenterRequirementOperator,
        KarpenterStableNodePoolOverride,
    };
    use crate::infrastructure::models::kubernetes::{Kind as KubernetesKind, KubernetesVersion};
    use crate::io_models::models::CpuArchitecture::ARM64;
    use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

    const KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_32 {
        prefix: None,
        patch: None,
        suffix: None,
    };

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn karpenter_configuration_chart_directory_exists_test() {
        // setup:
        let chart = create_chart(
            KUBERNETES_VERSION,
            true,
            KarpenterNodePool {
                requirements: vec![],
                stable_override: KarpenterStableNodePoolOverride {
                    budgets: vec![],
                    limits: None,
                },
                default_override: None,
            },
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
            KarpenterConfigurationChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn karpenter_configuration_chart_values_file_exists_test() {
        // setup:
        let chart = create_chart(
            KUBERNETES_VERSION,
            true,
            KarpenterNodePool {
                requirements: vec![],
                stable_override: KarpenterStableNodePoolOverride {
                    budgets: vec![],
                    limits: None,
                },
                default_override: None,
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
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            KarpenterConfigurationChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn karpenter_configuration_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = create_chart(
            KUBERNETES_VERSION,
            false,
            KarpenterNodePool {
                requirements: vec![],
                stable_override: KarpenterStableNodePoolOverride {
                    budgets: vec![],
                    limits: None,
                },
                default_override: None,
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
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
                ),
                KarpenterConfigurationChart::chart_name()
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }

    #[test]
    fn test_karpenter_configuration() {
        // Define your test cases
        let test_cases = vec![
            TestCase {
                with_spot: false,
                qovery_node_pools: KarpenterNodePool {
                    requirements: vec![
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::InstanceCategory,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["c".to_string()],
                        },
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::Arch,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["AMD64".to_string()],
                        },
                    ],
                    stable_override: KarpenterStableNodePoolOverride {
                        budgets: vec![KarpenterNodePoolDisruptionBudget {
                            nodes: "0".to_string(),
                            reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                            duration: duration_str::parse("2h").unwrap(),
                            schedule: "0 1 * * 3".to_string(),
                        }],
                        limits: None,
                    },
                    default_override: None,
                },
                verify_fn: verify_custom_node_pools,
            },
            TestCase {
                with_spot: true,
                qovery_node_pools: KarpenterNodePool {
                    requirements: vec![
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::InstanceCategory,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["c".to_string()],
                        },
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::Arch,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["AMD64".to_string()],
                        },
                    ],
                    stable_override: KarpenterStableNodePoolOverride {
                        budgets: vec![KarpenterNodePoolDisruptionBudget {
                            nodes: "0".to_string(),
                            reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                            duration: duration_str::parse("2h").unwrap(),
                            schedule: "0 1 * * 3".to_string(),
                        }],
                        limits: None,
                    },
                    default_override: None,
                },
                verify_fn: verify_custom_node_pools,
            },
            TestCase {
                with_spot: false,
                qovery_node_pools: KarpenterNodePool {
                    requirements: vec![
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::InstanceCategory,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["c".to_string()],
                        },
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::Arch,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["AMD64".to_string()],
                        },
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::CapacityType,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["spot".to_string()],
                        },
                    ],
                    stable_override: KarpenterStableNodePoolOverride {
                        budgets: vec![KarpenterNodePoolDisruptionBudget {
                            nodes: "0".to_string(),
                            reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                            duration: duration_str::parse("2h").unwrap(),
                            schedule: "0 1 * * 3".to_string(),
                        }],
                        limits: Some(KarpenterNodePoolLimits {
                            max_cpu: KubernetesCpuResourceUnit::MilliCpu(10_000),
                            max_memory: KubernetesMemoryResourceUnit::GibiByte(20),
                        }),
                    },
                    default_override: None,
                },
                verify_fn: verify_custom_node_pools,
            },
            TestCase {
                with_spot: false,
                qovery_node_pools: KarpenterNodePool {
                    requirements: vec![
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::InstanceCategory,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["c".to_string()],
                        },
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::Arch,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["AMD64".to_string()],
                        },
                        KarpenterNodePoolRequirement {
                            key: KarpenterNodePoolRequirementKey::CapacityType,
                            operator: Some(KarpenterRequirementOperator::In),
                            values: vec!["spot".to_string()],
                        },
                    ],
                    stable_override: KarpenterStableNodePoolOverride {
                        budgets: vec![KarpenterNodePoolDisruptionBudget {
                            nodes: "0".to_string(),
                            reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                            duration: duration_str::parse("2h").unwrap(),
                            schedule: "0 1 * * 3".to_string(),
                        }],
                        limits: None,
                    },
                    default_override: Some(KarpenterDefaultNodePoolOverride {
                        limits: Some(KarpenterNodePoolLimits {
                            max_cpu: KubernetesCpuResourceUnit::MilliCpu(30_000),
                            max_memory: KubernetesMemoryResourceUnit::GibiByte(40),
                        }),
                    }),
                },
                verify_fn: verify_custom_node_pools,
            },
        ];

        // Iterate through each test case
        for test_case in test_cases {
            let with_spot = test_case.with_spot;
            let has_default_node_pool_limits = test_case.qovery_node_pools.default_override.is_some();
            let has_stable_node_pool_limits = test_case.qovery_node_pools.stable_override.limits.is_some();

            let yaml = generate_chart_yaml(KUBERNETES_VERSION, with_spot, test_case.qovery_node_pools);

            (test_case.verify_fn)(&yaml, with_spot, has_default_node_pool_limits, has_stable_node_pool_limits);
        }
    }

    #[derive(Debug)]
    struct TestCase {
        with_spot: bool,
        qovery_node_pools: KarpenterNodePool,
        verify_fn: fn(&str, bool, bool, bool),
    }

    #[derive(Debug, Deserialize)]
    struct Limits {
        cpu: String,
        memory: String,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Budget {
        nodes: String,
        reasons: Option<Vec<String>>,
        duration: Option<String>,
        schedule: Option<String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Requirement {
        key: String,
        operator: String,
        values: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct SpecT {
        requirements: Vec<Requirement>,
    }

    #[derive(Debug, Deserialize)]
    struct Disruption {
        budgets: Vec<Budget>,
    }

    #[derive(Debug, Deserialize)]
    struct Template {
        spec: SpecT,
    }

    #[derive(Debug, Deserialize)]
    struct Spec {
        template: Template,
        disruption: Disruption,
        limits: Option<Limits>,
    }

    #[derive(Debug, Deserialize)]
    struct Metadata {
        name: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    struct NodePool {
        // apiVersion: String,
        kind: String,
        spec: Spec,
        metadata: Metadata,
    }

    fn create_chart(
        kubernetes_version: KubernetesVersion,
        with_spot: bool,
        qovery_node_pools: KarpenterNodePool,
    ) -> KarpenterConfigurationChart {
        KarpenterConfigurationChart::new(
            None,
            "whatever".to_string(),
            true,
            "security_group".to_string(),
            "cluster_id",
            Uuid::new_v4(),
            "organization_id",
            Uuid::new_v4(),
            kubernetes_version,
            "region",
            KarpenterParameters {
                spot_enabled: with_spot,
                max_node_drain_time_in_secs: None,
                disk_size_in_gib: 50,
                default_service_architecture: ARM64,
                qovery_node_pools,
            },
            None,
            0,
        )
    }

    fn generate_chart_yaml(
        kubernetes_version: KubernetesVersion,
        with_spot: bool,
        qovery_node_pools: KarpenterNodePool,
    ) -> String {
        // setup:
        let chart = create_chart(kubernetes_version, with_spot, qovery_node_pools);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            KarpenterConfigurationChart::chart_name(),
        );

        let helm = Helm::new::<String>(None, &[]).expect("Failed to initialize Helm");
        let common_chart = chart.to_common_helm_chart().expect("Failed to convert to common chart");

        // execute
        helm.get_template(&chart_path, &common_chart.chart_info)
            .expect("Failed to get Helm template")
    }

    fn verify_custom_node_pools(
        yaml: &str,
        with_spot: bool,
        has_default_node_pool_limits: bool,
        has_stable_node_pool_limits: bool,
    ) {
        let deserializer = serde_yaml::Deserializer::from_str(yaml);

        let node_pools: Vec<_> = deserializer
            .map(|document| {
                let value: Value = Value::deserialize(document).expect("Failed to deserialize YAML document");
                serde_yaml::from_value::<NodePool>(value)
            })
            .filter_map(Result::ok)
            .collect();

        assert_eq!(node_pools.len(), 2, "Expected exactly 2 node pools");
        assert_eq!(
            node_pools
                .iter()
                .map(|node_pool| node_pool.metadata.name.clone())
                .collect_vec(),
            vec!["default".to_string(), "stable".to_string()],
            "Expected default and stable"
        );
        for node_pool in node_pools {
            assert_eq!(node_pool.kind, "NodePool");

            // Check requirements
            let reqs = &node_pool.spec.template.spec.requirements;

            assert_requirement_exists(reqs, "karpenter.k8s.aws/instance-category", "In", vec!["c".to_string()]);
            assert_requirement_exists(reqs, "kubernetes.io/arch", "In", vec!["amd64".to_string()]);
            assert_requirement_exists(
                reqs,
                "karpenter.sh/capacity-type",
                "In",
                if with_spot {
                    vec!["spot".to_string(), "on-demand".to_string()]
                } else {
                    vec!["on-demand".to_string()]
                },
            );

            // Check stable node pool
            if node_pool.metadata.name == "stable" {
                // Consolidation
                assert_stable_node_pool_exists(&node_pool.spec.disruption.budgets, "10%", None, None, None);
                assert_stable_node_pool_exists(
                    &node_pool.spec.disruption.budgets,
                    "0",
                    Some(vec!["Underutilized".to_string()]),
                    Some("2h".to_string()),
                    Some("0 1 * * 3".to_string()),
                );

                // Limits
                if has_stable_node_pool_limits {
                    let limits = node_pool
                        .spec
                        .limits
                        .as_ref()
                        .expect("should have stable node pool limits");
                    assert_eq!(&limits.cpu, "10000m");
                    assert_eq!(&limits.memory, "20Gi");
                } else {
                    assert!(node_pool.spec.limits.is_none());
                }
            }

            // Check default node pool
            if node_pool.metadata.name == "default" {
                if has_default_node_pool_limits {
                    let limits = node_pool.spec.limits.expect("should have default node pool limits");
                    assert_eq!(&limits.cpu, "30000m");
                    assert_eq!(&limits.memory, "40Gi");
                } else {
                    assert!(node_pool.spec.limits.is_none());
                }
            }
        }
    }

    fn assert_requirement_exists(reqs: &[Requirement], key: &str, operator: &str, values: Vec<String>) {
        assert!(
            reqs.contains(&Requirement {
                key: key.to_string(),
                operator: operator.to_string(),
                values,
            }),
            "Expected {key} requirement to be present"
        );
    }

    fn assert_stable_node_pool_exists(
        budgets: &[Budget],
        nodes: &str,
        reasons: Option<Vec<String>>,
        duration: Option<String>,
        schedule: Option<String>,
    ) {
        assert!(
            budgets.contains(&Budget {
                nodes: nodes.to_string(),
                reasons: reasons.clone(),
                duration: duration.clone(),
                schedule: schedule.clone(),
            }),
            "Expected ({}-{}-{}-{}) budget to be present",
            nodes,
            reasons.unwrap_or(vec!["NO_REASONS".to_string()]).join(","),
            duration.unwrap_or("NO_DURATION".to_string()),
            schedule.unwrap_or("NO_SCHEDULE".to_string()),
        )
    }
}
