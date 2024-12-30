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

pub fn need_target_stable_node_pool(kubernetes: &dyn Kubernetes, min_instances: u32, is_stateful_set: bool) -> bool {
    kubernetes.kind() == Kind::Eks && kubernetes.is_karpenter_enabled() && (min_instances == 1 || is_stateful_set)
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
    use crate::environment::models::utils::add_arch_to_deployment_affinity_node;
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
}
