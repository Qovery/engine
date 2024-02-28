use crate::cloud_provider::models::CpuArchitecture;
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

#[cfg(test)]
mod tests {
    use crate::cloud_provider::models::CpuArchitecture;
    use crate::models::utils::add_arch_to_deployment_affinity_node;
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
