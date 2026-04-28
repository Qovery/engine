use std::collections::HashMap;

use crate::infrastructure::models::load_balancer::InteractWithLoadBalancer;

mod models {
    use std::net::IpAddr;

    use crate::infrastructure::models::cloud_provider::io::LoadBalancerIpAllocationId;
    use thiserror::Error;

    #[derive(Debug, Error, PartialEq, Eq)]
    pub enum AzureLoadBalancerIpAllocationIdError {
        #[error("Invalid Azure load balancer IP allocation id `{0}`: expected an IPv4 or IPv6 address")]
        InvalidIpAddress(String),
    }

    #[derive(Clone, Debug)]
    pub enum AzureLoadBalancerIpAllocationId {
        Ipv4(String),
        Ipv6(String),
    }

    impl AzureLoadBalancerIpAllocationId {
        pub fn try_new(id: LoadBalancerIpAllocationId) -> Result<Self, AzureLoadBalancerIpAllocationIdError> {
            match id.as_str().parse::<IpAddr>() {
                Ok(IpAddr::V4(_)) => Ok(Self::Ipv4(id.as_str().to_string())),
                Ok(IpAddr::V6(_)) => Ok(Self::Ipv6(id.as_str().to_string())),
                Err(_) => Err(AzureLoadBalancerIpAllocationIdError::InvalidIpAddress(id.as_str().to_string())),
            }
        }
    }

    // Documentation:
    // https://learn.microsoft.com/azure/aks/load-balancer-standard#specify-the-load-balancer-ip-address
    // https://learn.microsoft.com/azure/aks/configure-load-balancer-standard#customizations-via-kubernetes-annotations
    pub fn select_ipv4_and_ipv6(ids: &[AzureLoadBalancerIpAllocationId]) -> (Option<&str>, Option<&str>) {
        let mut ipv4 = None;
        let mut ipv6 = None;
        for id in ids {
            match id {
                AzureLoadBalancerIpAllocationId::Ipv4(v) if ipv4.is_none() => ipv4 = Some(v.as_str()),
                AzureLoadBalancerIpAllocationId::Ipv6(v) if ipv6.is_none() => ipv6 = Some(v.as_str()),
                _ => {}
            }
        }
        (ipv4, ipv6)
    }
}

pub use models::{AzureLoadBalancerIpAllocationId, AzureLoadBalancerIpAllocationIdError, select_ipv4_and_ipv6};

/// Azure load balancer configuration.
pub struct AzureLoadBalancer {
    pub load_balancer_ip_allocations: Option<Vec<AzureLoadBalancerIpAllocationId>>,
}

impl InteractWithLoadBalancer for AzureLoadBalancer {
    fn annotations(&self) -> Option<HashMap<String, String>> {
        let mut annotations = HashMap::new();
        if let Some(load_balancer_ip_allocations) = &self.load_balancer_ip_allocations {
            let (ipv4, ipv6) = select_ipv4_and_ipv6(load_balancer_ip_allocations);
            if let Some(ipv4) = ipv4 {
                annotations.insert(
                    "service.beta.kubernetes.io/azure-load-balancer-ipv4".to_string(),
                    ipv4.to_string(),
                );
            }
            if let Some(ipv6) = ipv6 {
                annotations.insert(
                    "service.beta.kubernetes.io/azure-load-balancer-ipv6".to_string(),
                    ipv6.to_string(),
                );
            }
        }

        if annotations.is_empty() {
            None
        } else {
            Some(annotations)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_azure_load_balancer_annotations_returns_none() {
        let lb = AzureLoadBalancer {
            load_balancer_ip_allocations: None,
        };
        let annotations = lb.annotations();
        assert!(annotations.is_none());
    }

    #[test]
    fn test_azure_load_balancer_can_be_created() {
        let _lb = AzureLoadBalancer {
            load_balancer_ip_allocations: None,
        };
    }

    #[test]
    fn test_azure_load_balancer_annotations_with_ipv4_and_ipv6() {
        let lb = AzureLoadBalancer {
            load_balancer_ip_allocations: Some(vec![
                AzureLoadBalancerIpAllocationId::try_new("20.1.2.3".into()).unwrap(),
                AzureLoadBalancerIpAllocationId::try_new("2001:db8::1".into()).unwrap(),
            ]),
        };

        let annotations = lb.annotations().expect("expected annotations");
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/azure-load-balancer-ipv4"),
            Some(&"20.1.2.3".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/azure-load-balancer-ipv6"),
            Some(&"2001:db8::1".to_string())
        );
    }

    #[test]
    fn test_azure_load_balancer_annotations_dispatch_by_ip_family_regardless_of_order() {
        let lb = AzureLoadBalancer {
            load_balancer_ip_allocations: Some(vec![
                AzureLoadBalancerIpAllocationId::try_new("2001:db8::1".into()).unwrap(),
                AzureLoadBalancerIpAllocationId::try_new("20.1.2.3".into()).unwrap(),
            ]),
        };

        let annotations = lb.annotations().expect("expected annotations");
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/azure-load-balancer-ipv4"),
            Some(&"20.1.2.3".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/azure-load-balancer-ipv6"),
            Some(&"2001:db8::1".to_string())
        );
    }

    #[test]
    fn test_azure_load_balancer_respects_max_8_annotations() {
        let lb = AzureLoadBalancer {
            load_balancer_ip_allocations: None,
        };

        if let Some(annotations) = lb.annotations() {
            assert!(
                annotations.len() <= 8,
                "Azure load balancer must have at most 8 annotations for Gateway API compatibility, got {}",
                annotations.len()
            );
        }
    }

    #[test]
    fn test_azure_load_balancer_ip_allocation_try_new_rejects_invalid_ip() {
        let result = AzureLoadBalancerIpAllocationId::try_new("not-an-ip".into());
        assert!(matches!(result, Err(AzureLoadBalancerIpAllocationIdError::InvalidIpAddress(_))));
    }
}
