use std::collections::HashMap;

use crate::infrastructure::models::load_balancer::InteractWithLoadBalancer;

mod models {
    use crate::infrastructure::models::cloud_provider::io::LoadBalancerIpAllocationId;
    use thiserror::Error;

    #[derive(Debug, Error, PartialEq, Eq)]
    pub enum GcpLoadBalancerIpAllocationError {
        #[error("Invalid GCP load balancer IP allocation ids: resource names must be non-empty")]
        EmptyResourceName,
    }

    #[derive(Clone, Debug)]
    pub struct GcpLoadBalancerIpAllocation {
        id: String,
    }

    impl GcpLoadBalancerIpAllocation {
        pub fn try_new(id: LoadBalancerIpAllocationId) -> Result<Self, GcpLoadBalancerIpAllocationError> {
            let id = id.as_str().trim().to_string();
            if id.is_empty() {
                return Err(GcpLoadBalancerIpAllocationError::EmptyResourceName);
            }
            Ok(Self { id })
        }

        pub fn as_str(&self) -> &str {
            &self.id
        }
    }
}

pub use models::{GcpLoadBalancerIpAllocation, GcpLoadBalancerIpAllocationError};

/// GCP load balancer configuration.
pub struct GcpLoadBalancer {
    pub load_balancer_ip_allocations: Option<Vec<GcpLoadBalancerIpAllocation>>,
}

impl InteractWithLoadBalancer for GcpLoadBalancer {
    fn annotations(&self) -> Option<HashMap<String, String>> {
        let Some(load_balancer_ip_allocations) = &self.load_balancer_ip_allocations else {
            return None;
        };

        let value = load_balancer_ip_allocations
            .iter()
            .map(GcpLoadBalancerIpAllocation::as_str)
            .collect::<Vec<_>>()
            .join(",");

        Some(HashMap::from([(
            "networking.gke.io/load-balancer-ip-addresses".to_string(),
            value,
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gcp_load_balancer_annotations_returns_none() {
        let lb = GcpLoadBalancer {
            load_balancer_ip_allocations: None,
        };
        let annotations = lb.annotations();
        assert!(annotations.is_none());
    }

    #[test]
    fn test_gcp_load_balancer_can_be_created() {
        let _lb = GcpLoadBalancer {
            load_balancer_ip_allocations: None,
        };
    }

    #[test]
    fn test_gcp_load_balancer_annotations_with_ip_allocations() {
        let lb = GcpLoadBalancer {
            load_balancer_ip_allocations: Some(vec![
                GcpLoadBalancerIpAllocation::try_new("projects/foo/regions/europe-west1/addresses/my-ipv4".into())
                    .unwrap(),
                GcpLoadBalancerIpAllocation::try_new(
                    "projects/foo/regions/europe-west1/addresses/my-ipv6-range".into(),
                )
                .unwrap(),
            ]),
        };

        let annotations = lb.annotations().expect("expected annotations");
        assert_eq!(
            annotations.get("networking.gke.io/load-balancer-ip-addresses"),
            Some(
                &"projects/foo/regions/europe-west1/addresses/my-ipv4,projects/foo/regions/europe-west1/addresses/my-ipv6-range"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_gcp_load_balancer_respects_max_8_annotations() {
        let lb = GcpLoadBalancer {
            load_balancer_ip_allocations: None,
        };

        if let Some(annotations) = lb.annotations() {
            assert!(
                annotations.len() <= 8,
                "GCP load balancer must have at most 8 annotations for Gateway API compatibility, got {}",
                annotations.len()
            );
        }
    }

    #[test]
    fn test_gcp_load_balancer_ip_allocation_try_new_rejects_empty_id() {
        let result = GcpLoadBalancerIpAllocation::try_new("   ".into());
        assert!(matches!(result, Err(GcpLoadBalancerIpAllocationError::EmptyResourceName)));
    }
}
