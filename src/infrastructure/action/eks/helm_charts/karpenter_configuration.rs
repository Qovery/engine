use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::domain::ToHelmString;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInfoUpgradeRetry, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError,
    HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::cloud_provider::aws::ec2_ami::Ec2Ami;
use crate::infrastructure::models::kubernetes::KubernetesVersion;
use crate::infrastructure::models::kubernetes::aws::{AwsStorageType, UserNetworkConfig};
use crate::infrastructure::models::kubernetes::karpenter::{
    KarpenterNodePoolDisruptionBudget, KarpenterNodePoolLimits, KarpenterNodePoolRequirement,
    KarpenterNodePoolRequirementKey, KarpenterParameters, KarpenterRequirementOperator,
};
use itertools::Itertools;
use kube::Client;
use std::collections::HashMap;

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
    region: String,
    karpenter_parameters: KarpenterParameters,
    kubernetes_version: KubernetesVersion,
    explicit_subnet_ids: Vec<String>,
    eks_ec2_ami: Ec2Ami,
    aws_storage_type: AwsStorageType,
    pleco_resources_ttl: i32,
    resource_tags: HashMap<String, String>,
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
        eks_ec2_ami: Ec2Ami,
        aws_storage_type: AwsStorageType,
        pleco_resources_ttl: i32,
        resource_tags: HashMap<String, String>,
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
            region: region.to_string(),
            kubernetes_version: kubernetes_version.clone(),
            karpenter_parameters,
            explicit_subnet_ids: if let Some(user_network_config) = &user_network_config {
                match user_network_config.eks_create_nodes_in_private_subnet {
                    true => [
                        &user_network_config.eks_private_subnets_zone_a_ids,
                        &user_network_config.eks_private_subnets_zone_b_ids,
                        &user_network_config.eks_private_subnets_zone_c_ids,
                    ],
                    false => [
                        &user_network_config.eks_subnets_zone_a_ids,
                        &user_network_config.eks_subnets_zone_b_ids,
                        &user_network_config.eks_subnets_zone_c_ids,
                    ],
                }
                .iter()
                .flat_map(|v| v.iter())
                .cloned()
                .collect_vec()
            } else {
                Vec::with_capacity(0)
            },
            // TODO(benjaminch): once 1.33 is fully released, we can remove this override
            eks_ec2_ami: match eks_ec2_ami {
                Ec2Ami::AmazonLinux2 => Ec2Ami::AmazonLinux2,
                Ec2Ami::Bottlerocket => Ec2Ami::Bottlerocket,
                // Custom AMIs bypass version-based downgrade logic
                Ec2Ami::Custom(v) => Ec2Ami::Custom(v),
                // Just making sure not to switch to AmazonLinux2023 for earlier k8s versions avoiding node replacement
                // AL2023 is the new default
                Ec2Ami::AmazonLinux2023 => match kubernetes_version {
                    KubernetesVersion::V1_23 { .. }
                    | KubernetesVersion::V1_24 { .. }
                    | KubernetesVersion::V1_25 { .. }
                    | KubernetesVersion::V1_26 { .. }
                    | KubernetesVersion::V1_27 { .. }
                    | KubernetesVersion::V1_28 { .. }
                    | KubernetesVersion::V1_29 { .. }
                    | KubernetesVersion::V1_30 { .. }
                    | KubernetesVersion::V1_31 { .. }
                    | KubernetesVersion::V1_32 { .. } => Ec2Ami::AmazonLinux2,
                    KubernetesVersion::V1_33 { .. } | KubernetesVersion::V1_34 { .. } => Ec2Ami::AmazonLinux2023,
                },
            },
            aws_storage_type,
            pleco_resources_ttl,
            resource_tags,
        }
    }

    pub fn chart_name() -> String {
        "karpenter-configuration".to_string()
    }

    fn enrich_karpenter_requirements(
        spot_enabled: bool,
        node_pool_requirements: &[KarpenterNodePoolRequirement],
    ) -> Vec<KarpenterNodePoolRequirement> {
        let mut requirements = node_pool_requirements.to_vec();
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

    fn push_requirements_values(
        values: &mut Vec<ChartSetValue>,
        pool_prefix: &str,
        requirements: &[KarpenterNodePoolRequirement],
    ) {
        requirements.iter().enumerate().for_each(|(index, requirement)| {
            let prefix = format!("{pool_prefix}.requirements[{index}]");

            let formated_values = if requirement.key == KarpenterNodePoolRequirementKey::Arch {
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
    }

    fn push_budgets_values(
        values: &mut Vec<ChartSetValue>,
        pool_prefix: &str,
        budgets: &[KarpenterNodePoolDisruptionBudget],
    ) {
        budgets.iter().enumerate().for_each(|(index, it)| {
            let prefix = format!("{pool_prefix}.consolidation.budgets[{index}]");

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
    }

    fn push_limits_values(values: &mut Vec<ChartSetValue>, pool_prefix: &str, limits: &KarpenterNodePoolLimits) {
        values.push(ChartSetValue {
            key: format!("{pool_prefix}.limits.maxCpu"),
            value: limits.max_cpu.to_string(),
        });
        values.push(ChartSetValue {
            key: format!("{pool_prefix}.limits.maxMemory"),
            value: limits.max_memory.to_string(),
        });
    }
}

impl ToCommonHelmChart for KarpenterConfigurationChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        // GPU nodes always use the standard AMI (not affected by custom AMI setting)
        let gpu_ec2_ami = if self.eks_ec2_ami.is_custom() {
            match self.kubernetes_version {
                KubernetesVersion::V1_33 { .. } | KubernetesVersion::V1_34 { .. } => Ec2Ami::AmazonLinux2023,
                _ => Ec2Ami::AmazonLinux2,
            }
        } else {
            match &self.eks_ec2_ami {
                Ec2Ami::AmazonLinux2 => Ec2Ami::AmazonLinux2,
                Ec2Ami::AmazonLinux2023 => Ec2Ami::AmazonLinux2023,
                Ec2Ami::Bottlerocket => Ec2Ami::Bottlerocket,
                Ec2Ami::Custom(_) => unreachable!(),
            }
        };

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
                value: self.eks_ec2_ami.ami_selector_terms_alias().unwrap_or("").to_string(),
            },
            ChartSetValue {
                key: "gpuAmiSelectorTermsName".to_string(),
                value: gpu_ec2_ami.ami_selector_terms_name(&self.kubernetes_version, None, true),
            },
            ChartSetValue {
                key: "gpuAmiFamily".to_string(),
                value: gpu_ec2_ami.karpenter_ami_family().to_string(),
            },
            ChartSetValue {
                key: "gpuIsBottlerocket".to_string(),
                value: gpu_ec2_ami.is_bottlerocket().to_string(),
            },
            ChartSetValue {
                key: "kubernetesVersion".to_string(),
                value: self.kubernetes_version.to_string(),
            },
            ChartSetValue {
                key: "isBottlerocket".to_string(),
                value: self.eks_ec2_ami.is_bottlerocket().to_string(),
            },
            ChartSetValue {
                key: "amiFamily".to_string(),
                value: self.eks_ec2_ami.to_string(),
            },
            ChartSetValue {
                key: "diskSize".to_string(),
                value: self.karpenter_parameters.disk_size.to_gib_string(),
            },
            ChartSetValue {
                key: "storageClass".to_string(),
                value: self.aws_storage_type.to_cloud_provider_format().to_string(),
            },
            ChartSetValue {
                key: "isCustomAmi".to_string(),
                value: self.eks_ec2_ami.is_custom().to_string(),
            },
        ];

        // Custom AMI: set either customAmiId or customAmiName using the reference (without family prefix)
        if let Some(ami_ref) = self.eks_ec2_ami.custom_ami_reference() {
            if self.eks_ec2_ami.is_ami_id() {
                values.push(ChartSetValue {
                    key: "customAmiId".to_string(),
                    value: ami_ref.to_string(),
                });
            } else {
                values.push(ChartSetValue {
                    key: "customAmiName".to_string(),
                    value: ami_ref.to_string(),
                });
            }
            // Override amiFamily for custom AMIs (Karpenter needs explicit amiFamily when using id/name selectors)
            values.push(ChartSetValue {
                key: "amiFamily".to_string(),
                value: self.eks_ec2_ami.karpenter_ami_family().to_string(),
            });
        }

        // Add custom resource tags first (before system tags).
        // This ensures system tags (ClusterId, OrganizationId, etc.) take precedence
        // and cannot be overridden by user-provided tags, similar to Terraform merge logic.
        for (key, value) in &self.resource_tags {
            values.push(ChartSetValue {
                key: format!("tags.{}", key),
                value: value.clone(),
            });
        }

        // System tags are added after custom tags to ensure they take precedence
        values.extend(vec![
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
        ]);

        // Only set IOPS and throughput for gp3 volumes
        if matches!(self.aws_storage_type, AwsStorageType::GP3) {
            if let Some(iops) = self.karpenter_parameters.disk_iops {
                values.push(ChartSetValue {
                    key: "diskIops".to_string(),
                    value: iops.to_string(),
                });
            }

            if let Some(throughput) = self.karpenter_parameters.disk_throughput {
                values.push(ChartSetValue {
                    key: "diskThroughput".to_string(),
                    value: throughput.to_string(),
                });
            }
        }

        if !self.explicit_subnet_ids.is_empty() {
            values.push(ChartSetValue {
                key: "explicitSubnetIds".to_string(),
                value: format!("{{{}}}", self.explicit_subnet_ids.join(",")),
            });
        }

        // Resolve per-pool spot settings, falling back to the global spot_enabled
        let default_spot = self
            .karpenter_parameters
            .qovery_node_pools
            .default_override
            .as_ref()
            .and_then(|o| o.spot_enabled)
            .unwrap_or(self.karpenter_parameters.spot_enabled);

        let stable_spot = self
            .karpenter_parameters
            .qovery_node_pools
            .stable_override
            .spot_enabled
            .unwrap_or(self.karpenter_parameters.spot_enabled);

        // Default node pool requirements
        let default_requirements = Self::enrich_karpenter_requirements(
            default_spot,
            &self.karpenter_parameters.qovery_node_pools.requirements,
        );
        Self::push_requirements_values(&mut values, "defaultNodePool", &default_requirements);

        // Stable node pool requirements
        let stable_requirements =
            Self::enrich_karpenter_requirements(stable_spot, &self.karpenter_parameters.qovery_node_pools.requirements);
        Self::push_requirements_values(&mut values, "stableNodePool", &stable_requirements);

        // Stable node pool consolidation
        let stable_pool_override = &self.karpenter_parameters.qovery_node_pools.stable_override;
        Self::push_budgets_values(&mut values, "stableNodePool", &stable_pool_override.budgets);

        // Stable node pool limits
        if let Some(limits) = &stable_pool_override.limits {
            Self::push_limits_values(&mut values, "stableNodePool", limits);
        }

        // Stable node pool consolidateAfter
        if let Some(consolidate_after_in_seconds) = stable_pool_override.consolidate_after_in_seconds {
            values.push(ChartSetValue {
                key: "stableNodePool.consolidateAfter".to_string(),
                value: format!("{}s", consolidate_after_in_seconds),
            });
        }

        // GPU node pool
        match &self.karpenter_parameters.qovery_node_pools.gpu_override {
            Some(gpu_pool_override) => {
                values.push(ChartSetValue {
                    key: "gpuNodePool.enable".to_string(),
                    value: true.to_string(),
                });

                // Requirements
                let gpu_spot = gpu_pool_override
                    .spot_enabled
                    .unwrap_or(self.karpenter_parameters.spot_enabled);
                let requirements = Self::enrich_karpenter_requirements(
                    gpu_spot,
                    gpu_pool_override.requirements.as_deref().unwrap_or(&[]),
                );
                Self::push_requirements_values(&mut values, "gpuNodePool", &requirements);

                // Node pool consolidation
                Self::push_budgets_values(&mut values, "gpuNodePool", &gpu_pool_override.budgets);

                // Node pool limits
                if let Some(limits) = &gpu_pool_override.limits {
                    Self::push_limits_values(&mut values, "gpuNodePool", limits);
                }

                // Disk size
                values.push(ChartSetValue {
                    key: "gpuNodePool.diskSize".to_string(),
                    value: gpu_pool_override.disk_size.to_gib_string(),
                });

                // Disk IOPS and throughput (GPU nodes always use gp3 storage)
                if let Some(iops) = gpu_pool_override.disk_iops {
                    values.push(ChartSetValue {
                        key: "gpuNodePool.diskIops".to_string(),
                        value: iops.to_string(),
                    });
                }

                if let Some(throughput) = gpu_pool_override.disk_throughput {
                    values.push(ChartSetValue {
                        key: "gpuNodePool.diskThroughput".to_string(),
                        value: throughput.to_string(),
                    });
                }

                // GPU node pool consolidateAfter
                if let Some(consolidate_after_in_seconds) = gpu_pool_override.consolidate_after_in_seconds {
                    values.push(ChartSetValue {
                        key: "gpuNodePool.consolidateAfter".to_string(),
                        value: format!("{}s", consolidate_after_in_seconds),
                    });
                }
            }
            None => {
                values.push(ChartSetValue {
                    key: "gpuNodePool.enable".to_string(),
                    value: "false".to_string(),
                });
            }
        }

        // Default node pool limits
        if let Some(limits) = self
            .karpenter_parameters
            .qovery_node_pools
            .default_override
            .as_ref()
            .and_then(|o| o.limits.as_ref())
        {
            Self::push_limits_values(&mut values, "defaultNodePool", limits);
        }

        // Default node pool consolidateAfter
        if let Some(default_override) = &self.karpenter_parameters.qovery_node_pools.default_override
            && let Some(consolidate_after_in_seconds) = default_override.consolidate_after_in_seconds
        {
            values.push(ChartSetValue {
                key: "defaultNodePool.consolidateAfter".to_string(),
                value: format!("{}s", consolidate_after_in_seconds),
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
                // Retry helm install in case CRDs are not yet fully propagated in the API server
                upgrade_retry: Some(ChartInfoUpgradeRetry {
                    nb_retry: 3,
                    delay_in_milli_sec: 10_000, // 10 seconds between retries = 30s total
                }),
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KarpenterChartChecker::new())),
            vertical_pod_autoscaler: None, // enabled in the chart configuration
            pre_execute_action: None,
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
    use crate::infrastructure::models::cloud_provider::aws::ec2_ami::Ec2Ami;
    use crate::infrastructure::models::disk_size::DiskSize;
    use crate::infrastructure::models::kubernetes::aws::{AwsStorageType, UserNetworkConfig};
    use crate::infrastructure::models::kubernetes::karpenter::{
        KarpenterDefaultNodePoolOverride, KarpenterGpuNodePoolOverride, KarpenterNodePool,
        KarpenterNodePoolDisruptionBudget, KarpenterNodePoolDisruptionReason, KarpenterNodePoolLimits,
        KarpenterNodePoolRequirement, KarpenterNodePoolRequirementKey, KarpenterParameters,
        KarpenterRequirementOperator, KarpenterStableNodePoolOverride,
    };
    use crate::infrastructure::models::kubernetes::{Kind as KubernetesKind, KubernetesVersion};
    use crate::io_models::models::CpuArchitecture::ARM64;
    use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

    const KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_33 {
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
                    spot_enabled: None,
                    budgets: vec![],
                    limits: None,
                    consolidate_after_in_seconds: None,
                },
                default_override: None,
                gpu_override: None,
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
                    spot_enabled: None,
                    budgets: vec![],
                    limits: None,
                    consolidate_after_in_seconds: None,
                },
                default_override: None,
                gpu_override: None,
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
                    spot_enabled: None,
                    budgets: vec![],
                    limits: None,
                    consolidate_after_in_seconds: None,
                },
                default_override: None,
                gpu_override: None,
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
                        spot_enabled: None,
                        budgets: vec![KarpenterNodePoolDisruptionBudget {
                            nodes: "0".to_string(),
                            reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                            duration: duration_str::parse("2h").unwrap(),
                            schedule: "0 1 * * 3".to_string(),
                        }],
                        limits: None,
                        consolidate_after_in_seconds: None,
                    },
                    default_override: None,
                    gpu_override: None,
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
                        spot_enabled: None,
                        budgets: vec![KarpenterNodePoolDisruptionBudget {
                            nodes: "0".to_string(),
                            reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                            duration: duration_str::parse("2h").unwrap(),
                            schedule: "0 1 * * 3".to_string(),
                        }],
                        limits: None,
                        consolidate_after_in_seconds: None,
                    },
                    default_override: None,
                    gpu_override: None,
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
                        spot_enabled: None,
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
                        consolidate_after_in_seconds: None,
                    },
                    default_override: None,
                    gpu_override: None,
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
                        spot_enabled: None,
                        budgets: vec![KarpenterNodePoolDisruptionBudget {
                            nodes: "0".to_string(),
                            reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                            duration: duration_str::parse("2h").unwrap(),
                            schedule: "0 1 * * 3".to_string(),
                        }],
                        limits: None,
                        consolidate_after_in_seconds: None,
                    },
                    default_override: Some(KarpenterDefaultNodePoolOverride {
                        spot_enabled: None,
                        limits: Some(KarpenterNodePoolLimits {
                            max_cpu: KubernetesCpuResourceUnit::MilliCpu(30_000),
                            max_memory: KubernetesMemoryResourceUnit::GibiByte(40),
                        }),
                        consolidate_after_in_seconds: None,
                    }),
                    gpu_override: Some(KarpenterGpuNodePoolOverride {
                        spot_enabled: Some(true),
                        budgets: vec![KarpenterNodePoolDisruptionBudget {
                            nodes: "0".to_string(),
                            reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                            duration: duration_str::parse("2h").unwrap(),
                            schedule: "0 1 * * 3".to_string(),
                        }],
                        limits: None,
                        disk_size: DiskSize::Gib(100),
                        disk_iops: None,
                        disk_throughput: None,
                        requirements: Some(vec![
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
                                values: vec!["on-demand".to_string()],
                            },
                        ]),
                        consolidate_after_in_seconds: None,
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
            let has_gpu_node_pool = test_case.qovery_node_pools.gpu_override.is_some();

            let yaml = generate_chart_yaml(KUBERNETES_VERSION, with_spot, test_case.qovery_node_pools);

            (test_case.verify_fn)(
                &yaml,
                with_spot,
                has_default_node_pool_limits,
                has_stable_node_pool_limits,
                has_gpu_node_pool,
            );
        }
    }

    #[derive(Debug)]
    struct TestCase {
        with_spot: bool,
        qovery_node_pools: KarpenterNodePool,
        verify_fn: fn(&str, bool, bool, bool, bool),
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
                disk_size: DiskSize::Gib(50),
                disk_iops: None,
                disk_throughput: None,
                default_service_architecture: ARM64,
                qovery_node_pools,
            },
            None,
            Ec2Ami::AmazonLinux2023,
            AwsStorageType::GP3,
            0,
            std::collections::HashMap::new(),
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
        has_gpu_node_pool: bool,
    ) {
        let deserializer = serde_yaml::Deserializer::from_str(yaml);

        let node_pools: Vec<_> = deserializer
            .map(|document| {
                let value: Value = Value::deserialize(document).expect("Failed to deserialize YAML document");
                serde_yaml::from_value::<NodePool>(value)
            })
            .filter_map(Result::ok)
            .collect();

        let expected_node_pool_count = if has_gpu_node_pool { 3 } else { 2 };
        assert_eq!(
            node_pools.len(),
            expected_node_pool_count,
            "Expected exactly {expected_node_pool_count} node pools"
        );
        assert_eq!(
            node_pools
                .iter()
                .map(|node_pool| node_pool.metadata.name.clone())
                .collect_vec(),
            match has_gpu_node_pool {
                true => vec!["gpu".to_string(), "default".to_string(), "stable".to_string(),],
                false => vec!["default".to_string(), "stable".to_string()],
            },
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

    #[test]
    fn test_karpenter_configuration_with_custom_vpc_private_subnets() {
        // setup:
        let user_network_config = UserNetworkConfig {
            documentdb_subnets_zone_a_ids: vec!["subnet-docdb-a".to_string()],
            documentdb_subnets_zone_b_ids: vec!["subnet-docdb-b".to_string()],
            documentdb_subnets_zone_c_ids: vec!["subnet-docdb-c".to_string()],
            elasticache_subnets_zone_a_ids: vec!["subnet-elastic-a".to_string()],
            elasticache_subnets_zone_b_ids: vec!["subnet-elastic-b".to_string()],
            elasticache_subnets_zone_c_ids: vec!["subnet-elastic-c".to_string()],
            rds_subnets_zone_a_ids: vec!["subnet-rds-a".to_string()],
            rds_subnets_zone_b_ids: vec!["subnet-rds-b".to_string()],
            rds_subnets_zone_c_ids: vec!["subnet-rds-c".to_string()],
            aws_vpc_eks_id: "vpc-custom-12345".to_string(),
            eks_subnets_zone_a_ids: vec!["subnet-public-a-1".to_string(), "subnet-public-a-2".to_string()],
            eks_subnets_zone_b_ids: vec!["subnet-public-b-1".to_string(), "subnet-public-b-2".to_string()],
            eks_subnets_zone_c_ids: vec!["subnet-public-c-1".to_string(), "subnet-public-c-2".to_string()],
            eks_private_subnets_zone_a_ids: vec!["subnet-private-a-1".to_string(), "subnet-private-a-2".to_string()],
            eks_private_subnets_zone_b_ids: vec!["subnet-private-b-1".to_string(), "subnet-private-b-2".to_string()],
            eks_private_subnets_zone_c_ids: vec!["subnet-private-c-1".to_string(), "subnet-private-c-2".to_string()],
            eks_create_nodes_in_private_subnet: true,
        };

        // execute:
        let chart = KarpenterConfigurationChart::new(
            None,
            "test-cluster".to_string(),
            true,
            "sg-12345".to_string(),
            "cluster-id",
            Uuid::new_v4(),
            "org-id",
            Uuid::new_v4(),
            KUBERNETES_VERSION,
            "us-east-1",
            KarpenterParameters {
                spot_enabled: false,
                max_node_drain_time_in_secs: None,
                disk_size: DiskSize::Gib(50),
                disk_iops: None,
                disk_throughput: None,
                default_service_architecture: ARM64,
                qovery_node_pools: KarpenterNodePool {
                    requirements: vec![],
                    stable_override: KarpenterStableNodePoolOverride {
                        spot_enabled: None,
                        budgets: vec![],
                        limits: None,
                        consolidate_after_in_seconds: None,
                    },
                    default_override: None,
                    gpu_override: None,
                },
            },
            Some(&user_network_config),
            Ec2Ami::AmazonLinux2023,
            AwsStorageType::GP3,
            0,
            std::collections::HashMap::new(),
        );

        // verify:
        let expected_private_subnets = vec![
            "subnet-private-a-1".to_string(),
            "subnet-private-a-2".to_string(),
            "subnet-private-b-1".to_string(),
            "subnet-private-b-2".to_string(),
            "subnet-private-c-1".to_string(),
            "subnet-private-c-2".to_string(),
        ];
        assert_eq!(
            chart.explicit_subnet_ids, expected_private_subnets,
            "When eks_create_nodes_in_private_subnet is true, explicit_subnet_ids should contain private subnets"
        );

        // verify:
        let common_chart = chart.to_common_helm_chart().unwrap();
        let explicit_subnet_value = common_chart
            .chart_info
            .values
            .iter()
            .find(|v| v.key == "explicitSubnetIds")
            .expect("explicitSubnetIds should be set in chart values");

        assert_eq!(
            explicit_subnet_value.value,
            "{subnet-private-a-1,subnet-private-a-2,subnet-private-b-1,subnet-private-b-2,subnet-private-c-1,subnet-private-c-2}",
            "explicitSubnetIds helm value should be formatted correctly with private subnets"
        );
    }

    #[test]
    fn test_karpenter_configuration_with_custom_vpc_public_subnets() {
        // setup:
        let user_network_config = UserNetworkConfig {
            documentdb_subnets_zone_a_ids: vec!["subnet-docdb-a".to_string()],
            documentdb_subnets_zone_b_ids: vec!["subnet-docdb-b".to_string()],
            documentdb_subnets_zone_c_ids: vec!["subnet-docdb-c".to_string()],
            elasticache_subnets_zone_a_ids: vec!["subnet-elastic-a".to_string()],
            elasticache_subnets_zone_b_ids: vec!["subnet-elastic-b".to_string()],
            elasticache_subnets_zone_c_ids: vec!["subnet-elastic-c".to_string()],
            rds_subnets_zone_a_ids: vec!["subnet-rds-a".to_string()],
            rds_subnets_zone_b_ids: vec!["subnet-rds-b".to_string()],
            rds_subnets_zone_c_ids: vec!["subnet-rds-c".to_string()],
            aws_vpc_eks_id: "vpc-custom-12345".to_string(),
            eks_subnets_zone_a_ids: vec!["subnet-public-a-1".to_string(), "subnet-public-a-2".to_string()],
            eks_subnets_zone_b_ids: vec!["subnet-public-b-1".to_string(), "subnet-public-b-2".to_string()],
            eks_subnets_zone_c_ids: vec!["subnet-public-c-1".to_string(), "subnet-public-c-2".to_string()],
            eks_private_subnets_zone_a_ids: vec!["subnet-private-a-1".to_string(), "subnet-private-a-2".to_string()],
            eks_private_subnets_zone_b_ids: vec!["subnet-private-b-1".to_string(), "subnet-private-b-2".to_string()],
            eks_private_subnets_zone_c_ids: vec!["subnet-private-c-1".to_string(), "subnet-private-c-2".to_string()],
            eks_create_nodes_in_private_subnet: false,
        };

        // execute:
        let chart = KarpenterConfigurationChart::new(
            None,
            "test-cluster".to_string(),
            true,
            "sg-12345".to_string(),
            "cluster-id",
            Uuid::new_v4(),
            "org-id",
            Uuid::new_v4(),
            KUBERNETES_VERSION,
            "us-east-1",
            KarpenterParameters {
                spot_enabled: false,
                max_node_drain_time_in_secs: None,
                disk_size: DiskSize::Gib(50),
                disk_iops: None,
                disk_throughput: None,
                default_service_architecture: ARM64,
                qovery_node_pools: KarpenterNodePool {
                    requirements: vec![],
                    stable_override: KarpenterStableNodePoolOverride {
                        spot_enabled: None,
                        budgets: vec![],
                        limits: None,
                        consolidate_after_in_seconds: None,
                    },
                    default_override: None,
                    gpu_override: None,
                },
            },
            Some(&user_network_config),
            Ec2Ami::AmazonLinux2023,
            AwsStorageType::GP3,
            0,
            std::collections::HashMap::new(),
        );

        // verify:
        let expected_public_subnets = vec![
            "subnet-public-a-1".to_string(),
            "subnet-public-a-2".to_string(),
            "subnet-public-b-1".to_string(),
            "subnet-public-b-2".to_string(),
            "subnet-public-c-1".to_string(),
            "subnet-public-c-2".to_string(),
        ];
        assert_eq!(
            chart.explicit_subnet_ids, expected_public_subnets,
            "When eks_create_nodes_in_private_subnet is false, explicit_subnet_ids should contain public subnets"
        );

        // verify:
        let common_chart = chart.to_common_helm_chart().unwrap();
        let explicit_subnet_value = common_chart
            .chart_info
            .values
            .iter()
            .find(|v| v.key == "explicitSubnetIds")
            .expect("explicitSubnetIds should be set in chart values");

        assert_eq!(
            explicit_subnet_value.value,
            "{subnet-public-a-1,subnet-public-a-2,subnet-public-b-1,subnet-public-b-2,subnet-public-c-1,subnet-public-c-2}",
            "explicitSubnetIds helm value should be formatted correctly with public subnets"
        );
    }

    // --- EC2NodeClass template rendering tests for custom AMI ---

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Ec2NodeClassSpec {
        ami_selector_terms: Vec<AmiSelectorTerm>,
        ami_family: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct AmiSelectorTerm {
        alias: Option<String>,
        id: Option<String>,
        name: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct Ec2NodeClass {
        #[allow(dead_code)]
        kind: String,
        metadata: Metadata,
        spec: Ec2NodeClassSpec,
    }

    fn generate_chart_yaml_with_ami(ec2_ami: Ec2Ami) -> String {
        let chart = KarpenterConfigurationChart::new(
            None,
            "test-cluster".to_string(),
            true,
            "sg-12345".to_string(),
            "cluster-id",
            Uuid::new_v4(),
            "org-id",
            Uuid::new_v4(),
            KUBERNETES_VERSION,
            "us-east-1",
            KarpenterParameters {
                spot_enabled: false,
                max_node_drain_time_in_secs: None,
                disk_size: DiskSize::Gib(50),
                disk_iops: None,
                disk_throughput: None,
                default_service_architecture: ARM64,
                qovery_node_pools: KarpenterNodePool {
                    requirements: vec![],
                    stable_override: KarpenterStableNodePoolOverride {
                        spot_enabled: None,
                        budgets: vec![],
                        limits: None,
                        consolidate_after_in_seconds: None,
                    },
                    default_override: None,
                    gpu_override: None,
                },
            },
            None,
            ec2_ami,
            AwsStorageType::GP3,
            0,
            std::collections::HashMap::new(),
        );

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

        helm.get_template(&chart_path, &common_chart.chart_info)
            .expect("Failed to get Helm template")
    }

    fn parse_ec2_node_classes(yaml: &str) -> Vec<Ec2NodeClass> {
        serde_yaml::Deserializer::from_str(yaml)
            .filter_map(|doc| {
                let value: Value = Value::deserialize(doc).ok()?;
                // Only keep EC2NodeClass documents
                if value.get("kind")?.as_str()? == "EC2NodeClass" {
                    serde_yaml::from_value::<Ec2NodeClass>(value).ok()
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn test_ec2nodeclass_template_standard_ami_uses_alias() {
        let yaml = generate_chart_yaml_with_ami(Ec2Ami::AmazonLinux2023);
        let node_classes = parse_ec2_node_classes(&yaml);

        let default = node_classes.iter().find(|nc| nc.metadata.name == "default").unwrap();

        // Standard AMI: should use alias, no amiFamily
        assert!(default.spec.ami_family.is_none(), "Standard AMI should not set amiFamily");
        assert_eq!(default.spec.ami_selector_terms.len(), 1);
        assert_eq!(default.spec.ami_selector_terms[0].alias.as_deref(), Some("al2023@latest"));
        assert!(default.spec.ami_selector_terms[0].id.is_none());
        assert!(default.spec.ami_selector_terms[0].name.is_none());
    }

    #[test]
    fn test_ec2nodeclass_template_bottlerocket_uses_alias() {
        let yaml = generate_chart_yaml_with_ami(Ec2Ami::Bottlerocket);
        let node_classes = parse_ec2_node_classes(&yaml);

        let default = node_classes.iter().find(|nc| nc.metadata.name == "default").unwrap();

        assert!(default.spec.ami_family.is_none(), "Standard AMI should not set amiFamily");
        assert_eq!(default.spec.ami_selector_terms[0].alias.as_deref(), Some("bottlerocket@latest"));
    }

    #[test]
    fn test_ec2nodeclass_template_custom_ami_id() {
        let yaml = generate_chart_yaml_with_ami(Ec2Ami::Custom("ami-0123456789abcdef0".to_string()));
        let node_classes = parse_ec2_node_classes(&yaml);

        let default = node_classes.iter().find(|nc| nc.metadata.name == "default").unwrap();

        // Custom AMI ID: should use id selector + explicit amiFamily, no alias
        assert_eq!(default.spec.ami_family.as_deref(), Some("AL2023"));
        assert_eq!(default.spec.ami_selector_terms.len(), 1);
        assert!(default.spec.ami_selector_terms[0].alias.is_none());
        assert_eq!(default.spec.ami_selector_terms[0].id.as_deref(), Some("ami-0123456789abcdef0"));
        assert!(default.spec.ami_selector_terms[0].name.is_none());
    }

    #[test]
    fn test_ec2nodeclass_template_custom_ami_name_pattern() {
        let yaml = generate_chart_yaml_with_ami(Ec2Ami::Custom("my-hardened-ami-*".to_string()));
        let node_classes = parse_ec2_node_classes(&yaml);

        let default = node_classes.iter().find(|nc| nc.metadata.name == "default").unwrap();

        // Custom AMI name: should use name selector + explicit amiFamily, no alias
        assert_eq!(default.spec.ami_family.as_deref(), Some("AL2023"));
        assert_eq!(default.spec.ami_selector_terms.len(), 1);
        assert!(default.spec.ami_selector_terms[0].alias.is_none());
        assert!(default.spec.ami_selector_terms[0].id.is_none());
        assert_eq!(default.spec.ami_selector_terms[0].name.as_deref(), Some("my-hardened-ami-*"));
    }

    #[test]
    fn test_ec2nodeclass_template_custom_ami_with_al2_prefix() {
        let yaml = generate_chart_yaml_with_ami(Ec2Ami::Custom("al2:ami-0123456789abcdef0".to_string()));
        let node_classes = parse_ec2_node_classes(&yaml);

        let default = node_classes.iter().find(|nc| nc.metadata.name == "default").unwrap();

        // al2: prefix → amiFamily should be AL2, reference without prefix
        assert_eq!(default.spec.ami_family.as_deref(), Some("AL2"));
        assert_eq!(default.spec.ami_selector_terms[0].id.as_deref(), Some("ami-0123456789abcdef0"));
    }

    #[test]
    fn test_ec2nodeclass_template_custom_ami_with_bottlerocket_prefix() {
        let yaml = generate_chart_yaml_with_ami(Ec2Ami::Custom("bottlerocket:ami-0123456789abcdef0".to_string()));
        let node_classes = parse_ec2_node_classes(&yaml);

        let default = node_classes.iter().find(|nc| nc.metadata.name == "default").unwrap();

        // bottlerocket: prefix → amiFamily should be Bottlerocket
        assert_eq!(default.spec.ami_family.as_deref(), Some("Bottlerocket"));
        assert_eq!(default.spec.ami_selector_terms[0].id.as_deref(), Some("ami-0123456789abcdef0"));
    }

    #[test]
    fn test_ec2nodeclass_template_custom_ami_does_not_affect_gpu() {
        let yaml = generate_chart_yaml_with_ami(Ec2Ami::Custom("bottlerocket:ami-custom123".to_string()));
        let node_classes = parse_ec2_node_classes(&yaml);

        // GPU nodeclass should not exist (gpu_override is None), but if it did,
        // let's at least verify the default nodeclass uses the custom AMI
        let default = node_classes.iter().find(|nc| nc.metadata.name == "default").unwrap();
        assert_eq!(default.spec.ami_family.as_deref(), Some("Bottlerocket"));
        assert_eq!(default.spec.ami_selector_terms[0].id.as_deref(), Some("ami-custom123"));

        // No GPU nodeclass should be rendered (gpu not enabled)
        assert!(
            node_classes.iter().all(|nc| nc.metadata.name != "gpu"),
            "GPU nodeclass should not be rendered when gpu_override is None"
        );
    }
}
