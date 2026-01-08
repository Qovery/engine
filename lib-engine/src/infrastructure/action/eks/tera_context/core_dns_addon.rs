use crate::{
    infrastructure::{
        helm_charts::HelmChartResources,
        models::{cloud_provider::io::ClusterProfile, kubernetes::KubernetesVersion},
    },
    io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit},
};
use serde_derive::Serialize;

/// AWS COREDNS addon
/// https://docs.aws.amazon.com/eks/latest/userguide/managing-coredns.html
#[derive(Debug, PartialEq, Serialize)]
pub struct AwsCoreDnsAddon {
    version: String,
    replica_count: u8,
    resources: HelmChartResources,
}

impl AwsCoreDnsAddon {
    pub fn new_from_k8s_version(k8s_version: KubernetesVersion, cluster_profile: ClusterProfile) -> Self {
        AwsCoreDnsAddon {
            // Get current default build of an aws-codedns add-on:
            // https://docs.aws.amazon.com/eks/latest/userguide/managing-coredns.html
            // aws eks describe-addon-versions --kubernetes-version 1.22 --addon-name aws-coredns | jq -r '.addons[].addonVersions[] | select(.compatibilities[].defaultVersion == true) | .addonVersion'
            version: match k8s_version {
                KubernetesVersion::V1_23 { .. } => "v1.8.7-eksbuild.10",
                KubernetesVersion::V1_24 { .. } => "v1.9.3-eksbuild.11",
                KubernetesVersion::V1_25 { .. } => "v1.9.3-eksbuild.11",
                KubernetesVersion::V1_26 { .. } => "v1.9.3-eksbuild.11",
                KubernetesVersion::V1_27 { .. } => "v1.10.1-eksbuild.7",
                KubernetesVersion::V1_28 { .. } => "v1.10.1-eksbuild.7",
                KubernetesVersion::V1_29 { .. } => "v1.10.1-eksbuild.7",
                KubernetesVersion::V1_30 { .. } => "v1.11.3-eksbuild.1",
                KubernetesVersion::V1_31 { .. } => "v1.11.4-eksbuild.2",
                KubernetesVersion::V1_32 { .. } => "v1.11.4-eksbuild.2",
                KubernetesVersion::V1_33 { .. } => "v1.12.2-eksbuild.4",
            }
            .to_string(),
            resources: AwsCoreDnsAddon::compute_resources(&cluster_profile),
            replica_count: AwsCoreDnsAddon::compute_replica_count(&cluster_profile),
        }
    }

    pub fn new_with_overridden_version(addon_version: &str, cluster_profile: ClusterProfile) -> Self {
        AwsCoreDnsAddon {
            version: addon_version.to_string(),
            resources: AwsCoreDnsAddon::compute_resources(&cluster_profile),
            replica_count: AwsCoreDnsAddon::compute_replica_count(&cluster_profile),
        }
    }

    fn compute_resources(cluster_profile: &ClusterProfile) -> HelmChartResources {
        match cluster_profile {
            ClusterProfile::Small => HelmChartResources {
                limit_cpu: None,
                request_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                limit_memory: Some(KubernetesMemoryResourceUnit::MebiByte(170)),
                request_memory: Some(KubernetesMemoryResourceUnit::MebiByte(70)),
            },
            ClusterProfile::Medium => HelmChartResources {
                limit_cpu: None,
                request_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(200)),
                limit_memory: Some(KubernetesMemoryResourceUnit::MebiByte(256)),
                request_memory: Some(KubernetesMemoryResourceUnit::MebiByte(140)),
            },
            ClusterProfile::Large => HelmChartResources {
                limit_cpu: None,
                request_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(300)),
                limit_memory: Some(KubernetesMemoryResourceUnit::MebiByte(384)),
                request_memory: Some(KubernetesMemoryResourceUnit::MebiByte(200)),
            },
            ClusterProfile::ExtraLarge => HelmChartResources {
                limit_cpu: None,
                request_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(500)),
                limit_memory: Some(KubernetesMemoryResourceUnit::MebiByte(512)),
                request_memory: Some(KubernetesMemoryResourceUnit::MebiByte(256)),
            },
        }
    }

    fn compute_replica_count(cluster_profile: &ClusterProfile) -> u8 {
        match cluster_profile {
            ClusterProfile::Small => 2u8,
            ClusterProfile::Medium => 2u8,
            ClusterProfile::Large => 4u8,
            ClusterProfile::ExtraLarge => 5u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::models::kubernetes::KubernetesVersion;

    #[test]
    fn aws_addon_coredns_new_test() {
        // setup:
        struct TestCase {
            k8s_version: KubernetesVersion,
            cluster_profile: ClusterProfile,
            expected_addon_version: String,
        }

        let tests_cases = vec![
            TestCase {
                k8s_version: KubernetesVersion::V1_23 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.8.7-eksbuild.10".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_24 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.9.3-eksbuild.11".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_25 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.9.3-eksbuild.11".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_26 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.9.3-eksbuild.11".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_27 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.10.1-eksbuild.7".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_28 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.10.1-eksbuild.7".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_29 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.10.1-eksbuild.7".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_30 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.11.3-eksbuild.1".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_31 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.11.4-eksbuild.2".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_32 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.11.4-eksbuild.2".to_string(),
            },
            TestCase {
                k8s_version: KubernetesVersion::V1_33 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                cluster_profile: ClusterProfile::Medium,
                expected_addon_version: "v1.12.2-eksbuild.4".to_string(),
            },
        ];

        for tc in tests_cases {
            // execute:
            let result = AwsCoreDnsAddon::new_from_k8s_version(tc.k8s_version, tc.cluster_profile);

            // verify:
            assert_eq!(tc.expected_addon_version, result.version);
        }
    }

    #[test]
    fn aws_addon_coredns_new_with_overridden_version() {
        // setup:
        let tests_cases = vec!["toto", "v1.8.7-eksbuild.10", "v1.8.7-eksbuild.11"];

        for tc in tests_cases {
            // execute:
            let result = AwsCoreDnsAddon::new_with_overridden_version(tc, ClusterProfile::Medium);

            // verify:
            assert_eq!(tc.to_string(), result.version);
        }
    }

    #[test]
    fn aws_addon_coredns_compute_resources_small_cluster() {
        // execute:
        let result = AwsCoreDnsAddon::compute_resources(&ClusterProfile::Small);

        // verify:
        assert_eq!(result.limit_cpu, None);
        assert_eq!(result.request_cpu, Some(KubernetesCpuResourceUnit::MilliCpu(100)));
        assert_eq!(result.limit_memory, Some(KubernetesMemoryResourceUnit::MebiByte(170)));
        assert_eq!(result.request_memory, Some(KubernetesMemoryResourceUnit::MebiByte(70)));
    }

    #[test]
    fn aws_addon_coredns_compute_resources_medium_cluster() {
        // execute:
        let result = AwsCoreDnsAddon::compute_resources(&ClusterProfile::Medium);

        // verify:
        assert_eq!(result.limit_cpu, None);
        assert_eq!(result.request_cpu, Some(KubernetesCpuResourceUnit::MilliCpu(200)));
        assert_eq!(result.limit_memory, Some(KubernetesMemoryResourceUnit::MebiByte(256)));
        assert_eq!(result.request_memory, Some(KubernetesMemoryResourceUnit::MebiByte(140)));
    }

    #[test]
    fn aws_addon_coredns_compute_resources_large_cluster() {
        // execute:
        let result = AwsCoreDnsAddon::compute_resources(&ClusterProfile::Large);

        // verify:
        assert_eq!(result.limit_cpu, None);
        assert_eq!(result.request_cpu, Some(KubernetesCpuResourceUnit::MilliCpu(300)));
        assert_eq!(result.limit_memory, Some(KubernetesMemoryResourceUnit::MebiByte(384)));
        assert_eq!(result.request_memory, Some(KubernetesMemoryResourceUnit::MebiByte(200)));
    }

    #[test]
    fn aws_addon_coredns_compute_resources_extra_large_cluster() {
        // execute:
        let result = AwsCoreDnsAddon::compute_resources(&ClusterProfile::ExtraLarge);

        // verify:
        assert_eq!(result.limit_cpu, None);
        assert_eq!(result.request_cpu, Some(KubernetesCpuResourceUnit::MilliCpu(500)));
        assert_eq!(result.limit_memory, Some(KubernetesMemoryResourceUnit::MebiByte(512)));
        assert_eq!(result.request_memory, Some(KubernetesMemoryResourceUnit::MebiByte(256)));
    }

    #[test]
    fn aws_addon_coredns_compute_replica_count_small_cluster() {
        // execute:
        let result = AwsCoreDnsAddon::compute_replica_count(&ClusterProfile::Small);

        // verify:
        assert_eq!(result, 2u8);
    }

    #[test]
    fn aws_addon_coredns_compute_replica_count_medium_cluster() {
        // execute:
        let result = AwsCoreDnsAddon::compute_replica_count(&ClusterProfile::Medium);

        // verify:
        assert_eq!(result, 2u8);
    }

    #[test]
    fn aws_addon_coredns_compute_replica_count_large_cluster() {
        // execute:
        let result = AwsCoreDnsAddon::compute_replica_count(&ClusterProfile::Large);

        // verify:
        assert_eq!(result, 4u8);
    }

    #[test]
    fn aws_addon_coredns_compute_replica_count_extra_large_cluster() {
        // execute:
        let result = AwsCoreDnsAddon::compute_replica_count(&ClusterProfile::ExtraLarge);

        // verify:
        assert_eq!(result, 5u8);
    }

    #[test]
    fn aws_addon_coredns_new_from_k8s_version_contains_correct_resources_and_replica_count() {
        // setup:
        let test_cases = vec![
            (ClusterProfile::Small, 2u8, 100, 170, 70),
            (ClusterProfile::Medium, 2u8, 200, 256, 140),
            (ClusterProfile::Large, 4u8, 300, 384, 200),
            (ClusterProfile::ExtraLarge, 5u8, 500, 512, 256),
        ];

        for (profile, expected_replicas, expected_cpu, expected_mem_limit, expected_mem_request) in test_cases {
            // execute:
            let result = AwsCoreDnsAddon::new_from_k8s_version(
                KubernetesVersion::V1_30 {
                    prefix: None,
                    patch: None,
                    suffix: None,
                },
                profile,
            );

            // verify:
            assert_eq!(result.replica_count, expected_replicas, "Failed for profile: {:?}", profile);
            assert_eq!(
                result.resources.request_cpu,
                Some(KubernetesCpuResourceUnit::MilliCpu(expected_cpu)),
                "Failed for profile: {:?}",
                profile
            );
            assert_eq!(
                result.resources.limit_memory,
                Some(KubernetesMemoryResourceUnit::MebiByte(expected_mem_limit)),
                "Failed for profile: {:?}",
                profile
            );
            assert_eq!(
                result.resources.request_memory,
                Some(KubernetesMemoryResourceUnit::MebiByte(expected_mem_request)),
                "Failed for profile: {:?}",
                profile
            );
            assert_eq!(result.resources.limit_cpu, None, "Failed for profile: {:?}", profile);
        }
    }

    #[test]
    fn aws_addon_coredns_new_with_overridden_version_contains_correct_resources_and_replica_count() {
        // setup:
        let custom_version = "v1.99.99-eksbuild.999";

        // execute:
        let result = AwsCoreDnsAddon::new_with_overridden_version(custom_version, ClusterProfile::Large);

        // verify:
        assert_eq!(result.version, custom_version);
        assert_eq!(result.replica_count, 4u8);
        assert_eq!(result.resources.request_cpu, Some(KubernetesCpuResourceUnit::MilliCpu(300)));
        assert_eq!(result.resources.limit_memory, Some(KubernetesMemoryResourceUnit::MebiByte(384)));
        assert_eq!(
            result.resources.request_memory,
            Some(KubernetesMemoryResourceUnit::MebiByte(200))
        );
        assert_eq!(result.resources.limit_cpu, None);
    }
}
