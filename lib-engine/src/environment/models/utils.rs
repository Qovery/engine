use crate::infrastructure::models::kubernetes::{Kind, Kubernetes};
use crate::io_models::models::CpuArchitecture;
use std::collections::BTreeMap;

pub fn add_arch_to_deployment_affinity_node(
    deployment_affinity_node_required: &BTreeMap<String, String>,
    cpu_architectures: &[CpuArchitecture],
) -> BTreeMap<String, String> {
    let mut deployment_affinity_node_required = deployment_affinity_node_required.clone();

    // For the moment deployment_affinity_node_required support only one value
    if let Some(arch) = cpu_architectures.first() {
        let arch = match arch {
            CpuArchitecture::AMD64 => "amd64",
            CpuArchitecture::ARM64 => "arm64",
        };
        deployment_affinity_node_required
            .entry("kubernetes.io/arch".to_string())
            .or_insert_with(|| arch.to_string());
    }

    deployment_affinity_node_required
}

pub fn need_target_stable_node_pool(
    kubernetes: &dyn Kubernetes,
    min_instances: u32,
    is_stateful_set: bool,
    service_explicitely_targets_stable: bool,
) -> bool {
    kubernetes.kind() == Kind::Eks
        && kubernetes.is_karpenter_enabled()
        && (service_explicitely_targets_stable || min_instances == 1 || is_stateful_set)
}

pub fn target_stable_node_pool(
    deployment_affinity_node_required: &mut BTreeMap<String, String>,
    tolerations: &mut BTreeMap<String, String>,
    is_stateful_set: bool,
) {
    deployment_affinity_node_required
        .entry("karpenter.sh/nodepool".to_string())
        .or_insert_with(|| "stable".to_string());

    if is_stateful_set {
        deployment_affinity_node_required
            .entry("karpenter.sh/capacity-type".to_string())
            .or_insert_with(|| "on-demand".to_string());
    }

    tolerations
        .entry("nodepool/stable".to_string())
        .or_insert_with(|| "NoSchedule".to_string());
}

#[cfg(test)]
mod tests {
    use crate::environment::models::utils::{add_arch_to_deployment_affinity_node, need_target_stable_node_pool};
    use crate::infrastructure::models::kubernetes::{Kind, Kubernetes};
    use crate::io_models::models::CpuArchitecture;
    use std::collections::BTreeMap;

    #[test]
    fn test_add_arch_to_deployment_affinity_node_with_empty_arch() {
        let deployment_affinity_node_required = BTreeMap::<String, String>::new();
        let cpu_architectures = vec![];

        let result = add_arch_to_deployment_affinity_node(&deployment_affinity_node_required, &cpu_architectures);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_add_arch_to_deployment_affinity_node_with_empty_arch_and_existing_key_value() {
        let mut deployment_affinity_node_required = BTreeMap::<String, String>::new();
        deployment_affinity_node_required.insert("key".to_string(), "value".to_string());
        let cpu_architectures = vec![];

        let result = add_arch_to_deployment_affinity_node(&deployment_affinity_node_required, &cpu_architectures);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_add_arch_to_deployment_affinity_node() {
        let deployment_affinity_node_required = BTreeMap::<String, String>::new();
        let cpu_architectures = vec![CpuArchitecture::AMD64];

        let result = add_arch_to_deployment_affinity_node(&deployment_affinity_node_required, &cpu_architectures);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("kubernetes.io/arch"), Some(&"amd64".to_string()));
    }

    #[test]
    fn test_add_arch_to_deployment_affinity_node_with_existing_key_value() {
        let mut deployment_affinity_node_required = BTreeMap::<String, String>::new();
        deployment_affinity_node_required.insert("key".to_string(), "value".to_string());
        let cpu_architectures = vec![CpuArchitecture::ARM64];

        let result = add_arch_to_deployment_affinity_node(&deployment_affinity_node_required, &cpu_architectures);
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("key"), Some(&"value".to_string()));
        assert_eq!(result.get("kubernetes.io/arch"), Some(&"arm64".to_string()));
    }

    #[test]
    fn test_add_arch_to_deployment_affinity_node_with_existing_arch_value() {
        let mut deployment_affinity_node_required = BTreeMap::<String, String>::new();
        deployment_affinity_node_required.insert("kubernetes.io/arch".to_string(), "value".to_string());
        let cpu_architectures = vec![CpuArchitecture::ARM64];

        let result = add_arch_to_deployment_affinity_node(&deployment_affinity_node_required, &cpu_architectures);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("kubernetes.io/arch"), Some(&"value".to_string()));
    }

    struct MockKubernetes {
        kind: Kind,
        karpenter_enabled: bool,
    }

    impl Kubernetes for MockKubernetes {
        fn kind(&self) -> Kind {
            self.kind
        }

        fn is_karpenter_enabled(&self) -> bool {
            self.karpenter_enabled
        }

        fn context(&self) -> &crate::io_models::context::Context {
            todo!()
        }

        fn short_id(&self) -> &str {
            todo!()
        }

        fn long_id(&self) -> &uuid::Uuid {
            todo!()
        }

        fn name(&self) -> &str {
            todo!()
        }

        fn version(&self) -> crate::infrastructure::models::kubernetes::KubernetesVersion {
            todo!()
        }

        fn region(&self) -> &str {
            todo!()
        }

        fn zones(&self) -> Option<Vec<&str>> {
            todo!()
        }

        fn logger(&self) -> &dyn crate::logger::Logger {
            todo!()
        }

        fn is_network_managed_by_user(&self) -> bool {
            todo!()
        }

        fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
            todo!()
        }

        fn temp_dir(&self) -> &std::path::Path {
            todo!()
        }

        fn advanced_settings(&self) -> &crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings {
            todo!()
        }

        fn loadbalancer_l4_annotations(&self, _cloud_provider_lb_name: Option<&str>) -> Vec<(String, String)> {
            todo!()
        }

        fn as_infra_actions(&self) -> &dyn crate::infrastructure::action::InfrastructureAction {
            todo!()
        }
    }

    struct TestCase {
        kind: Kind,
        karpenter_enabled: bool,
        min_instances: u32,
        is_stateful_set: bool,
        explicit_target_stable_node_pool: bool,
        expected: bool,
        name: &'static str,
    }

    #[test]
    fn test_need_target_stable_node_pool_cases() {
        let cases = [
            TestCase {
                kind: Kind::Eks,
                karpenter_enabled: true,
                min_instances: 2,
                is_stateful_set: false,
                explicit_target_stable_node_pool: true,
                expected: true,
                name: "eks + karpenter + explicit_target",
            },
            TestCase {
                kind: Kind::Eks,
                karpenter_enabled: true,
                min_instances: 1,
                is_stateful_set: false,
                explicit_target_stable_node_pool: false,
                expected: true,
                name: "eks + karpenter + min_instances=1",
            },
            TestCase {
                kind: Kind::Eks,
                karpenter_enabled: true,
                min_instances: 2,
                is_stateful_set: true,
                explicit_target_stable_node_pool: false,
                expected: true,
                name: "eks + karpenter + stateful_set",
            },
            TestCase {
                kind: Kind::Eks,
                karpenter_enabled: true,
                min_instances: 2,
                is_stateful_set: false,
                explicit_target_stable_node_pool: false,
                expected: false,
                name: "eks + karpenter + no conditions",
            },
            TestCase {
                kind: Kind::Gke,
                karpenter_enabled: true,
                min_instances: 1,
                is_stateful_set: true,
                explicit_target_stable_node_pool: true,
                expected: false,
                name: "not eks",
            },
            TestCase {
                kind: Kind::Eks,
                karpenter_enabled: false,
                min_instances: 1,
                is_stateful_set: true,
                explicit_target_stable_node_pool: true,
                expected: false,
                name: "eks + no karpenter",
            },
        ];

        for case in cases.iter() {
            let kube = MockKubernetes {
                kind: case.kind,
                karpenter_enabled: case.karpenter_enabled,
            };
            let result = need_target_stable_node_pool(
                &kube,
                case.min_instances,
                case.is_stateful_set,
                case.explicit_target_stable_node_pool,
            );
            assert_eq!(case.expected, result, "failed test: {}", case.name);
        }
    }
}
