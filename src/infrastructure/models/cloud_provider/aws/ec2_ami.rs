use crate::infrastructure::models::kubernetes::KubernetesVersion;
use crate::io_models::models::CpuArchitecture;
use std::fmt::{Display, Formatter};

#[derive(PartialEq, Eq)]
pub enum Ec2Ami {
    AmazonLinux2,
    AmazonLinux2023,
    Bottlerocket,
    Custom(String),
}

impl Ec2Ami {
    pub fn is_custom(&self) -> bool {
        matches!(self, Ec2Ami::Custom(_))
    }

    /// For Custom AMIs, parses an optional family prefix (e.g., "al2:ami-xxx", "bottlerocket:my-ami-*").
    /// Returns (family, ami_reference). Defaults to "AL2023" if no known prefix.
    fn parse_custom_value(value: &str) -> (&str, &str) {
        if let Some((prefix, reference)) = value.split_once(':') {
            match prefix.to_lowercase().as_str() {
                "al2" => ("AL2", reference),
                "al2023" => ("AL2023", reference),
                "bottlerocket" => ("Bottlerocket", reference),
                _ => ("AL2023", value), // Unknown prefix, treat whole string as reference
            }
        } else {
            ("AL2023", value)
        }
    }

    /// Returns the AMI reference (without the family prefix) for custom AMIs.
    pub fn custom_ami_reference(&self) -> Option<&str> {
        match self {
            Ec2Ami::Custom(v) => Some(Self::parse_custom_value(v).1),
            _ => None,
        }
    }

    /// Returns true if the custom AMI reference is an AMI ID (starts with "ami-").
    pub fn is_ami_id(&self) -> bool {
        matches!(self.custom_ami_reference(), Some(r) if r.starts_with("ami-"))
    }

    /// Returns true if this is a Bottlerocket-based AMI (standard or custom with bottlerocket: prefix).
    pub fn is_bottlerocket(&self) -> bool {
        match self {
            Ec2Ami::Bottlerocket => true,
            Ec2Ami::Custom(v) => Self::parse_custom_value(v).0 == "Bottlerocket",
            _ => false,
        }
    }

    /// Returns the Karpenter amiFamily value.
    /// For custom AMIs, parses the optional family prefix (defaults to AL2023).
    pub fn karpenter_ami_family(&self) -> &str {
        match self {
            Ec2Ami::AmazonLinux2 => "AL2",
            Ec2Ami::AmazonLinux2023 => "AL2023",
            Ec2Ami::Bottlerocket => "Bottlerocket",
            Ec2Ami::Custom(v) => Self::parse_custom_value(v).0,
        }
    }

    pub fn ami_type(&self, arch: CpuArchitecture) -> &str {
        match (self, arch) {
            (Ec2Ami::AmazonLinux2, CpuArchitecture::AMD64) => "AL2_x86_64",
            (Ec2Ami::AmazonLinux2, CpuArchitecture::ARM64) => "AL2_ARM_64",
            (Ec2Ami::AmazonLinux2023, CpuArchitecture::AMD64) => "AL2023_x86_64_STANDARD",
            (Ec2Ami::AmazonLinux2023, CpuArchitecture::ARM64) => "AL2023_ARM_64_STANDARD",
            (Ec2Ami::Bottlerocket, CpuArchitecture::ARM64) => "BOTTLEROCKET_ARM_64",
            (Ec2Ami::Bottlerocket, CpuArchitecture::AMD64) => "BOTTLEROCKET_x86_64",
            (Ec2Ami::Custom(_), _) => "CUSTOM_AMI",
        }
    }

    /// Returns the Karpenter alias for standard AMIs, or `None` for custom AMIs.
    pub fn ami_selector_terms_alias(&self) -> Option<&str> {
        match self {
            Ec2Ami::AmazonLinux2 => Some("al2@latest"),
            Ec2Ami::AmazonLinux2023 => Some("al2023@latest"),
            Ec2Ami::Bottlerocket => Some("bottlerocket@latest"),
            Ec2Ami::Custom(_) => None,
        }
    }

    pub fn ami_selector_terms_name(
        &self,
        kubernetes_versions: &KubernetesVersion,
        arch: Option<CpuArchitecture>,
        gpu: bool,
    ) -> String {
        let flavor = match gpu {
            true => "nvidia",
            false => "standard",
        };

        match (self, arch) {
            (Ec2Ami::AmazonLinux2, None) => format!("amazon-eks-node-*-{flavor}-{kubernetes_versions}*"),
            (Ec2Ami::AmazonLinux2, Some(CpuArchitecture::AMD64)) => {
                format!("amazon-eks-node-x86_64-{flavor}-{kubernetes_versions}*")
            }
            (Ec2Ami::AmazonLinux2, Some(CpuArchitecture::ARM64)) => {
                format!("amazon-eks-node-arm64-{flavor}-{kubernetes_versions}*")
            }
            (Ec2Ami::AmazonLinux2023, None) => format!("amazon-eks-node-al2023-*-{flavor}-{kubernetes_versions}*"),
            (Ec2Ami::AmazonLinux2023, Some(CpuArchitecture::AMD64)) => {
                format!("amazon-eks-node-al2023-x86_64-{flavor}-{kubernetes_versions}*")
            }
            (Ec2Ami::AmazonLinux2023, Some(CpuArchitecture::ARM64)) => {
                format!("amazon-eks-node-al2023-arm64-{flavor}-{kubernetes_versions}*")
            }
            (Ec2Ami::Bottlerocket, None) => format!("bottlerocket-aws-k8s-{kubernetes_versions}-{flavor}-*"),
            (Ec2Ami::Bottlerocket, Some(CpuArchitecture::AMD64)) => {
                format!("bottlerocket-aws-k8s-{kubernetes_versions}-{flavor}-x86_64-*")
            }
            (Ec2Ami::Bottlerocket, Some(CpuArchitecture::ARM64)) => {
                format!("bottlerocket-aws-k8s-{kubernetes_versions}-{flavor}-aarch64-*")
            }
            // Custom AMIs return the reference part (without family prefix)
            (Ec2Ami::Custom(v), _) => Self::parse_custom_value(v).1.to_string(),
        }
    }
}

impl Display for Ec2Ami {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let ami_str = match self {
            Ec2Ami::AmazonLinux2 => "AL2",
            Ec2Ami::AmazonLinux2023 => "AL2023",
            Ec2Ami::Bottlerocket => "Bottlerocket",
            Ec2Ami::Custom(_) => "Custom",
        };
        write!(f, "{ami_str}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ami_type() {
        let ami = Ec2Ami::AmazonLinux2;
        assert_eq!(ami.ami_type(CpuArchitecture::AMD64), "AL2_x86_64");
        assert_eq!(ami.ami_type(CpuArchitecture::ARM64), "AL2_ARM_64");

        let ami = Ec2Ami::AmazonLinux2023;
        assert_eq!(ami.ami_type(CpuArchitecture::AMD64), "AL2023_x86_64_STANDARD");
        assert_eq!(ami.ami_type(CpuArchitecture::ARM64), "AL2023_ARM_64_STANDARD");

        let ami = Ec2Ami::Bottlerocket;
        assert_eq!(ami.ami_type(CpuArchitecture::AMD64), "BOTTLEROCKET_x86_64");
        assert_eq!(ami.ami_type(CpuArchitecture::ARM64), "BOTTLEROCKET_ARM_64");

        let ami = Ec2Ami::Custom("ami-12345".to_string());
        assert_eq!(ami.ami_type(CpuArchitecture::AMD64), "CUSTOM_AMI");
        assert_eq!(ami.ami_type(CpuArchitecture::ARM64), "CUSTOM_AMI");
    }

    #[test]
    fn test_ami_selector_terms_alias() {
        assert_eq!(Ec2Ami::AmazonLinux2.ami_selector_terms_alias(), Some("al2@latest"));
        assert_eq!(Ec2Ami::AmazonLinux2023.ami_selector_terms_alias(), Some("al2023@latest"));
        assert_eq!(Ec2Ami::Bottlerocket.ami_selector_terms_alias(), Some("bottlerocket@latest"));
        assert_eq!(Ec2Ami::Custom("ami-12345".to_string()).ami_selector_terms_alias(), None);
    }

    #[test]
    fn test_to_string() {
        assert_eq!(Ec2Ami::AmazonLinux2.to_string(), "AL2");
        assert_eq!(Ec2Ami::AmazonLinux2023.to_string(), "AL2023");
        assert_eq!(Ec2Ami::Bottlerocket.to_string(), "Bottlerocket");
        assert_eq!(Ec2Ami::Custom("ami-12345".to_string()).to_string(), "Custom");
    }

    const K8S_TEST_VERSION: KubernetesVersion = KubernetesVersion::V1_33 {
        prefix: None,
        patch: None,
        suffix: None,
    };

    #[test]
    fn test_ami_selector_terms_name_amazon_linux2_none_arch_standard() {
        let ami = Ec2Ami::AmazonLinux2;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, None, false);
        assert_eq!(result, "amazon-eks-node-*-standard-1.33*");
    }

    #[test]
    fn test_ami_selector_terms_name_amazon_linux2_x86_64_nvidia() {
        let ami = Ec2Ami::AmazonLinux2;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, Some(CpuArchitecture::AMD64), true);
        assert_eq!(result, "amazon-eks-node-x86_64-nvidia-1.33*");
    }

    #[test]
    fn test_ami_selector_terms_name_amazon_linux2023_arm64_standard() {
        let ami = Ec2Ami::AmazonLinux2023;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, Some(CpuArchitecture::ARM64), false);
        assert_eq!(result, "amazon-eks-node-al2023-arm64-standard-1.33*");
    }

    #[test]
    fn test_ami_selector_terms_name_bottlerocket_none_arch_nvidia() {
        let ami = Ec2Ami::Bottlerocket;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, None, true);
        assert_eq!(result, "bottlerocket-aws-k8s-1.33-nvidia-*");
    }

    #[test]
    fn test_ami_selector_terms_name_bottlerocket_x86_64_standard() {
        let ami = Ec2Ami::Bottlerocket;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, Some(CpuArchitecture::AMD64), false);
        assert_eq!(result, "bottlerocket-aws-k8s-1.33-standard-x86_64-*");
    }

    #[test]
    fn test_ami_selector_terms_name_bottlerocket_arm64_nvidia() {
        let ami = Ec2Ami::Bottlerocket;
        let result = ami.ami_selector_terms_name(&K8S_TEST_VERSION, Some(CpuArchitecture::ARM64), true);
        assert_eq!(result, "bottlerocket-aws-k8s-1.33-nvidia-aarch64-*");
    }

    #[test]
    fn test_is_custom() {
        assert!(!Ec2Ami::AmazonLinux2.is_custom());
        assert!(!Ec2Ami::AmazonLinux2023.is_custom());
        assert!(!Ec2Ami::Bottlerocket.is_custom());
        assert!(Ec2Ami::Custom("ami-12345".to_string()).is_custom());
        assert!(Ec2Ami::Custom("al2:ami-12345".to_string()).is_custom());
    }

    #[test]
    fn test_is_ami_id() {
        assert!(!Ec2Ami::AmazonLinux2.is_ami_id());
        assert!(!Ec2Ami::AmazonLinux2023.is_ami_id());
        // No prefix — ami-xxx is an AMI ID
        assert!(Ec2Ami::Custom("ami-12345abc".to_string()).is_ami_id());
        // With prefix — reference part is ami-xxx
        assert!(Ec2Ami::Custom("al2:ami-12345abc".to_string()).is_ami_id());
        assert!(Ec2Ami::Custom("bottlerocket:ami-12345".to_string()).is_ami_id());
        // Name patterns are not AMI IDs
        assert!(!Ec2Ami::Custom("my-custom-ami-*".to_string()).is_ami_id());
        assert!(!Ec2Ami::Custom("al2:my-custom-ami-*".to_string()).is_ami_id());
    }

    #[test]
    fn test_is_bottlerocket() {
        assert!(!Ec2Ami::AmazonLinux2.is_bottlerocket());
        assert!(!Ec2Ami::AmazonLinux2023.is_bottlerocket());
        assert!(Ec2Ami::Bottlerocket.is_bottlerocket());
        // Custom with bottlerocket prefix
        assert!(Ec2Ami::Custom("bottlerocket:ami-12345".to_string()).is_bottlerocket());
        assert!(Ec2Ami::Custom("Bottlerocket:my-br-ami-*".to_string()).is_bottlerocket());
        // Custom without bottlerocket prefix
        assert!(!Ec2Ami::Custom("ami-12345".to_string()).is_bottlerocket());
        assert!(!Ec2Ami::Custom("al2:ami-12345".to_string()).is_bottlerocket());
    }

    #[test]
    fn test_karpenter_ami_family_with_prefix() {
        // Standard variants
        assert_eq!(Ec2Ami::AmazonLinux2.karpenter_ami_family(), "AL2");
        assert_eq!(Ec2Ami::AmazonLinux2023.karpenter_ami_family(), "AL2023");
        assert_eq!(Ec2Ami::Bottlerocket.karpenter_ami_family(), "Bottlerocket");
        // Custom without prefix → defaults to AL2023
        assert_eq!(Ec2Ami::Custom("ami-12345".to_string()).karpenter_ami_family(), "AL2023");
        assert_eq!(Ec2Ami::Custom("my-custom-ami-*".to_string()).karpenter_ami_family(), "AL2023");
        // Custom with prefix
        assert_eq!(Ec2Ami::Custom("al2:ami-12345".to_string()).karpenter_ami_family(), "AL2");
        assert_eq!(Ec2Ami::Custom("al2023:ami-12345".to_string()).karpenter_ami_family(), "AL2023");
        assert_eq!(
            Ec2Ami::Custom("bottlerocket:ami-12345".to_string()).karpenter_ami_family(),
            "Bottlerocket"
        );
        // Case insensitive prefix
        assert_eq!(Ec2Ami::Custom("AL2:ami-12345".to_string()).karpenter_ami_family(), "AL2");
        assert_eq!(
            Ec2Ami::Custom("BOTTLEROCKET:ami-12345".to_string()).karpenter_ami_family(),
            "Bottlerocket"
        );
    }

    #[test]
    fn test_custom_ami_reference() {
        // No prefix — whole string is the reference
        assert_eq!(
            Ec2Ami::Custom("ami-12345".to_string()).custom_ami_reference(),
            Some("ami-12345")
        );
        assert_eq!(Ec2Ami::Custom("my-ami-*".to_string()).custom_ami_reference(), Some("my-ami-*"));
        // With prefix — reference is after the colon
        assert_eq!(
            Ec2Ami::Custom("al2:ami-12345".to_string()).custom_ami_reference(),
            Some("ami-12345")
        );
        assert_eq!(
            Ec2Ami::Custom("bottlerocket:my-br-*".to_string()).custom_ami_reference(),
            Some("my-br-*")
        );
        // Unknown prefix — whole string is the reference
        assert_eq!(
            Ec2Ami::Custom("unknown:ami-12345".to_string()).custom_ami_reference(),
            Some("unknown:ami-12345")
        );
        // Standard variants return None
        assert_eq!(Ec2Ami::AmazonLinux2.custom_ami_reference(), None);
    }

    #[test]
    fn test_custom_ami_selector_terms_name_returns_reference() {
        // Without prefix
        let ami = Ec2Ami::Custom("my-custom-ami-*".to_string());
        assert_eq!(ami.ami_selector_terms_name(&K8S_TEST_VERSION, None, false), "my-custom-ami-*");
        // With prefix — returns reference only
        let ami = Ec2Ami::Custom("al2:my-custom-ami-*".to_string());
        assert_eq!(ami.ami_selector_terms_name(&K8S_TEST_VERSION, None, false), "my-custom-ami-*");
        let ami = Ec2Ami::Custom("bottlerocket:ami-12345".to_string());
        assert_eq!(ami.ami_selector_terms_name(&K8S_TEST_VERSION, None, false), "ami-12345");
    }
}
