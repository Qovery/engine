use crate::environment::models::domain::ToHelmString;
use crate::io_models::models::{CpuArchitecture, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use duration_str::deserialize_duration;
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use serde_with::DisplayFromStr;
use std::fmt;
use std::fmt::Formatter;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KarpenterParameters {
    pub spot_enabled: bool,
    pub max_node_drain_time_in_secs: Option<i32>,
    pub disk_size_in_gib: i32,
    pub default_service_architecture: CpuArchitecture,
    pub qovery_node_pools: KarpenterNodePool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KarpenterNodePool {
    pub requirements: Vec<KarpenterNodePoolRequirement>,
    pub stable_override: KarpenterStableNodePoolOverride,
    pub default_override: Option<KarpenterDefaultNodePoolOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KarpenterNodePoolRequirement {
    pub key: KarpenterNodePoolRequirementKey,
    pub values: Vec<String>,
    pub operator: Option<KarpenterRequirementOperator>,
}

// TODO(benjaminch): refactor this, would be better to have an enum per provider
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum KarpenterNodePoolRequirementKey {
    InstanceType,
    Arch,
    CapacityType,
    Os,

    // AWS specifics
    InstanceCategory,
    InstanceFamily,
    InstanceGeneration,
    InstanceSize,

    // Azure specifics
    // https://learn.microsoft.com/fr-fr/azure/aks/node-autoprovision?tabs=azure-cli
    SkuFamily,
    SkuName,
    SkuVersion,
}

impl KarpenterNodePoolRequirementKey {
    // TODO(benjaminch): refactor this, would be better to have an enum per provider
    pub(crate) fn to_k8s_label(&self) -> String {
        match self {
            KarpenterNodePoolRequirementKey::InstanceType => "node.kubernetes.io/instance-type".to_string(),
            KarpenterNodePoolRequirementKey::Arch => "kubernetes.io/arch".to_string(),
            KarpenterNodePoolRequirementKey::CapacityType => "karpenter.sh/capacity-type".to_string(),
            KarpenterNodePoolRequirementKey::Os => "kubernetes.io/os".to_string(),

            // AWS specifics
            KarpenterNodePoolRequirementKey::InstanceCategory => "karpenter.k8s.aws/instance-category".to_string(),
            KarpenterNodePoolRequirementKey::InstanceFamily => "karpenter.k8s.aws/instance-family".to_string(),
            KarpenterNodePoolRequirementKey::InstanceGeneration => "karpenter.k8s.aws/instance-generation".to_string(),
            KarpenterNodePoolRequirementKey::InstanceSize => "karpenter.k8s.aws/instance-size".to_string(),

            // Azure specifics
            KarpenterNodePoolRequirementKey::SkuFamily => "karpenter.azure.com/sku-family".to_string(),
            KarpenterNodePoolRequirementKey::SkuName => "karpenter.azure.com/sku-name".to_string(),
            KarpenterNodePoolRequirementKey::SkuVersion => "karpenter.azure.com/sku-version".to_string(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum KarpenterRequirementOperator {
    In,
    Gt,
}

impl fmt::Display for KarpenterRequirementOperator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let output = match self {
            KarpenterRequirementOperator::In => "In",
            KarpenterRequirementOperator::Gt => "Gt",
        };
        write!(f, "{}", output)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct KarpenterStableNodePoolOverride {
    pub budgets: Vec<KarpenterNodePoolDisruptionBudget>,
    pub limits: Option<KarpenterNodePoolLimits>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct KarpenterNodePoolDisruptionBudget {
    pub nodes: String,
    pub reasons: Vec<KarpenterNodePoolDisruptionReason>,
    #[serde(deserialize_with = "deserialize_duration")]
    pub duration: Duration,
    pub schedule: KarpenterNodePoolDisruptionBudgetSchedule,
}

pub type KarpenterNodePoolDisruptionBudgetSchedule = String;

impl KarpenterNodePoolDisruptionBudget {
    pub fn get_karpenter_budget_duration_as_string(&self) -> String {
        let total_seconds = self.duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;

        match (hours, minutes) {
            (0, m) => format!("{}m", m),
            (h, 0) => format!("{}h", h),
            (h, m) => format!("{}h{}m", h, m),
        }
    }
}

impl ToHelmString for Vec<KarpenterNodePoolDisruptionReason> {
    fn to_helm_format_string(&self) -> String {
        let reasons_join = self.iter().map(|it| it.to_string()).join(",");
        format!("{{{reasons_join}}}")
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum KarpenterNodePoolDisruptionReason {
    Underutilized,
}

impl fmt::Display for KarpenterNodePoolDisruptionReason {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let output = match self {
            KarpenterNodePoolDisruptionReason::Underutilized => "Underutilized",
        };
        write!(f, "{}", output)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct KarpenterDefaultNodePoolOverride {
    pub limits: Option<KarpenterNodePoolLimits>,
}

#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct KarpenterNodePoolLimits {
    #[serde_as(as = "DisplayFromStr")]
    pub max_cpu: KubernetesCpuResourceUnit,
    #[serde_as(as = "DisplayFromStr")]
    pub max_memory: KubernetesMemoryResourceUnit,
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::models::kubernetes::karpenter::{
        KarpenterDefaultNodePoolOverride, KarpenterNodePoolDisruptionBudget, KarpenterNodePoolDisruptionReason,
        KarpenterNodePoolLimits, KarpenterParameters, KarpenterStableNodePoolOverride,
    };
    use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

    #[test]
    fn should_deserialize_correctly_when_stable_node_pool_override_is_present_with_consolidation() {
        // given
        let karpenter_parameters_json = r#"
        {
          "spot_enabled": true,
          "disk_size_in_gib": 20,
          "default_service_architecture": "AMD64",
          "qovery_node_pools": {
            "requirements": [
              {
                "key": "InstanceFamily",
                "operator": "In",
                "values": [
                  "z1d"
                ]
              },
              {
                "key": "InstanceSize",
                "operator": "In",
                "values": [
                  "10xlarge",
                  "xlarge"
                ]
              },
              {
                "key": "Arch",
                "operator": "In",
                "values": [
                  "AMD64",
                  "ARM64"
                ]
              }
            ],
            "stable_override": {
              "budgets": [
                {
                  "nodes": "0",
                  "reasons": ["Underutilized"],
                  "duration": "22h30m",
                  "schedule": "30 3 * * 1"
                },
                {
                  "nodes": "0",
                  "reasons": ["Underutilized"],
                  "duration": "142h30m",
                  "schedule": "30 3 * * 2"
                }
              ],
              "limits": null
            }
          }
        }
        "#;

        // when
        let result = serde_json::from_str::<KarpenterParameters>(karpenter_parameters_json);

        // then
        assert!(result.is_ok());
        let karpenter_parameters = result.expect("result should be Ok");
        let stable_node_pool_override = karpenter_parameters.qovery_node_pools.stable_override;
        assert_eq!(
            stable_node_pool_override,
            KarpenterStableNodePoolOverride {
                budgets: vec![
                    KarpenterNodePoolDisruptionBudget {
                        nodes: "0".to_string(),
                        reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                        duration: duration_str::parse("22h30m").expect("22h30m should be a valid Duration"),
                        schedule: "30 3 * * 1".to_string(),
                    },
                    KarpenterNodePoolDisruptionBudget {
                        nodes: "0".to_string(),
                        reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                        duration: duration_str::parse("142h30m").expect("142h30m should be a valid Duration"),
                        schedule: "30 3 * * 2".to_string(),
                    }
                ],
                limits: None,
            }
        )
    }

    #[test]
    fn should_fail_to_deserialize_when_budget_duration_is_not_well_formatted() {
        // given
        let wrong_durations = vec!["word", "-/@", "8hh", "123"];

        // when
        wrong_durations.into_iter().for_each(|wrong_duration| {
            let karpenter_node_pool_override_json = r#"
            {
              "budgets": [
                {
                  "nodes": "0",
                  "reasons": ["Underutilized"],
                  "duration": "22h30m",
                  "schedule": "30 3 * * 1"
                },
                {
                  "nodes": "0",
                  "reasons": ["Underutilized"],
                  "duration": {wrong_duration}
                  "schedule": "30 3 * * 2"
                }
              ]
            }
        "#
            .replace("{duration}", wrong_duration);

            // when
            let result =
                serde_json::from_str::<KarpenterStableNodePoolOverride>(karpenter_node_pool_override_json.as_str());

            // then
            assert!(result.is_err());
            let err = result.expect_err("result should be an Err");
            assert!(
                err.to_string()
                    .contains("invalid type: map, expected expect duration string")
            );
        });
    }

    #[test]
    fn should_parse_duration_to_karpenter_budget_format() {
        // given
        let duration_test_cases_with_expectations = vec![
            (duration_str::parse("2h").expect("2h should be a valid Duration"), "2h"),
            (duration_str::parse("20m").expect("20m should be a valid Duration"), "20m"),
            (
                duration_str::parse("72h30m").expect("72h30m should be a valid Duration"),
                "72h30m",
            ),
            (duration_str::parse("180m").expect("180m should be a valid Duration"), "3h"),
            (duration_str::parse("1h180m").expect("1h180m should be a valid Duration"), "4h"),
            (
                duration_str::parse("1h190m").expect("1h190m should be a valid Duration"),
                "4h10m",
            ),
        ];
        duration_test_cases_with_expectations
            .into_iter()
            .for_each(|(duration, expected_karpenter_budget_duration)| {
                let budget = KarpenterNodePoolDisruptionBudget {
                    nodes: "".to_string(),
                    reasons: vec![],
                    duration,
                    schedule: "".to_string(),
                };

                // when
                let karpenter_formatted_duration = budget.get_karpenter_budget_duration_as_string();

                // then
                assert_eq!(karpenter_formatted_duration, expected_karpenter_budget_duration);
            });
    }

    #[test]
    fn should_deserialize_correctly_when_stable_node_pool_override_is_present_with_limits() {
        // given
        let karpenter_parameters_json = r#"
        {
          "spot_enabled": true,
          "disk_size_in_gib": 20,
          "default_service_architecture": "AMD64",
          "qovery_node_pools": {
            "requirements": [
              {
                "key": "InstanceFamily",
                "operator": "In",
                "values": [
                  "z1d"
                ]
              },
              {
                "key": "InstanceSize",
                "operator": "In",
                "values": [
                  "10xlarge",
                  "xlarge"
                ]
              },
              {
                "key": "Arch",
                "operator": "In",
                "values": [
                  "AMD64",
                  "ARM64"
                ]
              }
            ],
            "stable_override": {
              "budgets": [
                {
                  "nodes": "0",
                  "reasons": ["Underutilized"],
                  "duration": "24h",
                  "schedule": "0 0 * * *"
                }
              ],
              "limits": {
                "max_cpu": "6000m",
                "max_memory": "20Gi"
              }
            }
          }
        }
        "#;

        // when
        let result = serde_json::from_str::<KarpenterParameters>(karpenter_parameters_json);

        // then
        assert!(result.is_ok());
        let karpenter_parameters = result.expect("should be Ok");
        let stable_node_pool_override = karpenter_parameters.qovery_node_pools.stable_override;
        assert_eq!(
            stable_node_pool_override,
            KarpenterStableNodePoolOverride {
                budgets: vec![
                    // default budgets from deserialization
                    KarpenterNodePoolDisruptionBudget {
                        nodes: "0".to_string(),
                        reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                        duration: duration_str::parse("24h").expect("24h should be a valid Duration"),
                        schedule: "0 0 * * *".to_string(),
                    },
                ],
                limits: Some(KarpenterNodePoolLimits {
                    max_cpu: KubernetesCpuResourceUnit::MilliCpu(6000),
                    max_memory: KubernetesMemoryResourceUnit::GibiByte(20),
                })
            }
        )
    }

    #[test]
    fn should_deserialize_correctly_when_default_override_is_present_with_limits() {
        // given
        let karpenter_parameters_json = r#"
        {
          "spot_enabled": true,
          "disk_size_in_gib": 20,
          "default_service_architecture": "AMD64",
          "qovery_node_pools": {
            "requirements": [
              {
                "key": "InstanceFamily",
                "operator": "In",
                "values": [
                  "z1d"
                ]
              },
              {
                "key": "InstanceSize",
                "operator": "In",
                "values": [
                  "10xlarge",
                  "xlarge"
                ]
              },
              {
                "key": "Arch",
                "operator": "In",
                "values": [
                  "AMD64",
                  "ARM64"
                ]
              }
            ],
            "stable_override": {
            "budgets": [
                {
                  "nodes": "0",
                  "reasons": ["Underutilized"],
                  "duration": "24h",
                  "schedule": "0 0 * * *"
                }
              ]
            },
            "default_override": {
              "limits": {
                "max_cpu": "6000m",
                "max_memory": "20Gi"
              }
            }
          }
        }
        "#;

        // when
        let result = serde_json::from_str::<KarpenterParameters>(karpenter_parameters_json);

        // then
        assert!(result.is_ok());
        let karpenter_parameters = result.expect("should be Ok");
        let default_node_pool_override = karpenter_parameters
            .qovery_node_pools
            .default_override
            .expect("default_override should be present");
        assert_eq!(
            default_node_pool_override,
            KarpenterDefaultNodePoolOverride {
                limits: Some(KarpenterNodePoolLimits {
                    max_cpu: KubernetesCpuResourceUnit::MilliCpu(6000),
                    max_memory: KubernetesMemoryResourceUnit::GibiByte(20),
                })
            }
        )
    }

    #[test]
    fn should_deserialize_correctly_when_default_override_is_present_without_limits() {
        // given
        let karpenter_parameters_json = r#"
        {
          "spot_enabled": true,
          "disk_size_in_gib": 20,
          "default_service_architecture": "AMD64",
          "qovery_node_pools": {
            "requirements": [
              {
                "key": "InstanceFamily",
                "operator": "In",
                "values": [
                  "z1d"
                ]
              },
              {
                "key": "InstanceSize",
                "operator": "In",
                "values": [
                  "10xlarge",
                  "xlarge"
                ]
              },
              {
                "key": "Arch",
                "operator": "In",
                "values": [
                  "AMD64",
                  "ARM64"
                ]
              }
            ],
            "stable_override": {
            "budgets": [
                {
                  "nodes": "0",
                  "reasons": ["Underutilized"],
                  "duration": "24h",
                  "schedule": "0 0 * * *"
                }
              ]
            },
            "default_override": {
              "limits": null
            }
          }
        }
        "#;

        // when
        let result = serde_json::from_str::<KarpenterParameters>(karpenter_parameters_json);

        // then
        assert!(result.is_ok());
        let karpenter_parameters = result.expect("should be Ok");
        let default_node_pool_override = karpenter_parameters
            .qovery_node_pools
            .default_override
            .expect("default_override should be present");
        assert_eq!(default_node_pool_override, KarpenterDefaultNodePoolOverride { limits: None })
    }
}
