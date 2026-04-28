use std::collections::HashMap;

use crate::infrastructure::models::load_balancer::InteractWithLoadBalancer;

mod models {
    use crate::infrastructure::models::cloud_provider::io::LoadBalancerIpAllocationId;
    use thiserror::Error;
    use uuid::Uuid;

    #[derive(Debug, Error, PartialEq, Eq)]
    pub enum ScwLoadBalancerIpAllocationError {
        #[error("Invalid Scaleway load balancer IP allocation ids: IDs must be non-empty")]
        EmptyId,
        #[error("Invalid Scaleway load balancer IP allocation ids: expected UUID format")]
        InvalidUuid,
    }

    #[derive(Clone, Debug)]
    pub struct ScwLoadBalancerIpAllocation {
        id: String,
    }

    impl ScwLoadBalancerIpAllocation {
        pub fn try_new(id: LoadBalancerIpAllocationId) -> Result<Self, ScwLoadBalancerIpAllocationError> {
            let id = id.as_str().trim().to_string();
            if id.is_empty() {
                return Err(ScwLoadBalancerIpAllocationError::EmptyId);
            }
            if Uuid::parse_str(&id).is_err() {
                return Err(ScwLoadBalancerIpAllocationError::InvalidUuid);
            }
            Ok(Self { id })
        }

        pub fn as_str(&self) -> &str {
            &self.id
        }
    }
}

pub use models::{ScwLoadBalancerIpAllocation, ScwLoadBalancerIpAllocationError};

/// Scaleway load balancer configuration.
pub struct ScalewayLoadBalancer {
    /// Load balancer size (e.g., "LB-S", "LB-M", "LB-L"). Defaults to "LB-S" if None.
    pub size: Option<String>,
    /// Provider-level static IP allocation IDs attached to the LB.
    pub load_balancer_ip_allocations: Option<Vec<ScwLoadBalancerIpAllocation>>,
}

impl InteractWithLoadBalancer for ScalewayLoadBalancer {
    fn annotations(&self) -> Option<HashMap<String, String>> {
        // Documentation: https://github.com/scaleway/scaleway-cloud-controller-manager/blob/master/docs/loadbalancer-annotations.md
        let mut annotations = HashMap::from([
            (
                "service.beta.kubernetes.io/scw-loadbalancer-type".to_string(),
                self.size.as_ref().map_or("LB-S", |v| v).to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-forward-port-algorithm".to_string(),
                "leastconn".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v2".to_string(),
                "true".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-health-check-type".to_string(),
                "80:tcp;443:tcp".to_string(),
            ),
            // HTTP URI not needed for TCP health checks, but kept for backward compatibility
            (
                "service.beta.kubernetes.io/scw-loadbalancer-health-check-http-uri".to_string(),
                "80:/healthz;443:/healthz".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-health-check-send-proxy".to_string(),
                "true".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-use-hostname".to_string(),
                "true".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-timeout-server".to_string(),
                "30s".to_string(),
            ),
            // Default settings, no need to declare those, but just in case
            // (
            //     "service.beta.kubernetes.io/scw-loadbalancer-protocol-http".to_string(),
            //     "false".to_string(),
            // ),
            // (
            //     "service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v1".to_string(),
            //     "false".to_string(),
            // ),
            // (
            //     "service.beta.kubernetes.io/scw-loadbalancer-health-check-delay".to_string(),
            //     "2s".to_string(),
            // ),
            // (
            //     "service.beta.kubernetes.io/scw-loadbalancer-health-check-timeout".to_string(),
            //     "2s".to_string(),
            // ),
            // (
            //     "service.beta.kubernetes.io/scw-loadbalancer-health-check-max-retries".to_string(),
            //     "2".to_string(),
            // ),
            // (
            //     "service.beta.kubernetes.io/scw-loadbalancer-redispatch-attempt-count".to_string(),
            //     "1".to_string(),
            // ),
        ]);

        if let Some(load_balancer_ip_allocations) = &self.load_balancer_ip_allocations {
            annotations.insert(
                "service.beta.kubernetes.io/scw-loadbalancer-ip-ids".to_string(),
                load_balancer_ip_allocations
                    .iter()
                    .map(ScwLoadBalancerIpAllocation::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }

        Some(annotations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::models::cloud_provider::io::LoadBalancerIpAllocationId;

    fn parse_ids(ids: Vec<LoadBalancerIpAllocationId>) -> Result<Option<Vec<ScwLoadBalancerIpAllocation>>, String> {
        if ids.is_empty() {
            return Ok(None);
        }
        if ids.len() > 2 {
            return Err(format!(
                "Invalid Scaleway load balancer IP allocation ids: got {}, but Scaleway supports at most 2 IDs",
                ids.len()
            ));
        }

        ids.into_iter()
            .map(ScwLoadBalancerIpAllocation::try_new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
            .map(Some)
    }

    #[test]
    fn test_scaleway_load_balancer_annotations_returns_some() {
        let lb = ScalewayLoadBalancer {
            size: None,
            load_balancer_ip_allocations: None,
        };
        let annotations = lb.annotations();
        assert!(annotations.is_some());
    }

    #[test]
    fn test_scaleway_load_balancer_annotations_default_size() {
        let lb = ScalewayLoadBalancer {
            size: None,
            load_balancer_ip_allocations: None,
        };
        let annotations = lb.annotations().unwrap();

        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-type"),
            Some(&"LB-S".to_string())
        );
    }

    #[test]
    fn test_scaleway_load_balancer_annotations_custom_size() {
        let lb = ScalewayLoadBalancer {
            size: Some("LB-L".to_string()),
            load_balancer_ip_allocations: None,
        };
        let annotations = lb.annotations().unwrap();

        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-type"),
            Some(&"LB-L".to_string())
        );
    }

    #[test]
    fn test_scaleway_load_balancer_annotations_keys_and_values() {
        let lb = ScalewayLoadBalancer {
            size: None,
            load_balancer_ip_allocations: None,
        };
        let annotations = lb.annotations().unwrap();

        // Verify count (reduced to 8 for Gateway API max limit)
        assert_eq!(annotations.len(), 8);

        // Verify basic configuration
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-forward-port-algorithm"),
            Some(&"leastconn".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-use-hostname"),
            Some(&"true".to_string())
        );

        // Verify proxy protocol settings (enabled for Envoy Gateway)
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v2"),
            Some(&"true".to_string())
        );

        // Verify health check configuration (TCP-based for Envoy Gateway compatibility)
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-type"),
            Some(&"80:tcp;443:tcp".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-http-uri"),
            Some(&"80:/healthz;443:/healthz".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-send-proxy"),
            Some(&"true".to_string())
        );

        // Verify timeout settings
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-timeout-server"),
            Some(&"30s".to_string())
        );
    }

    #[test]
    fn test_scaleway_load_balancer_respects_max_8_annotations() {
        let lb = ScalewayLoadBalancer {
            size: None,
            load_balancer_ip_allocations: None,
        };

        let annotations = lb.annotations().unwrap();
        assert!(
            annotations.len() <= 8,
            "Scaleway load balancer must have at most 8 annotations for Gateway API compatibility, got {}",
            annotations.len()
        );
    }

    #[test]
    fn test_scaleway_load_balancer_ip_ids_annotation_when_set() {
        let lb = ScalewayLoadBalancer {
            size: None,
            load_balancer_ip_allocations: parse_ids(vec!["11111111-2222-3333-4444-555555555555".into()]).unwrap(),
        };

        let annotations = lb.annotations().unwrap();
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-ip-ids"),
            Some(&"11111111-2222-3333-4444-555555555555".to_string())
        );
    }

    #[test]
    fn test_scaleway_load_balancer_ip_allocations_try_new_rejects_more_than_two_ids() {
        let result = parse_ids(vec!["id-1".into(), "id-2".into(), "id-3".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_scaleway_load_balancer_ip_allocations_try_new_rejects_empty_id() {
        let result = ScwLoadBalancerIpAllocation::try_new("   ".into());
        assert!(matches!(result, Err(ScwLoadBalancerIpAllocationError::EmptyId)));
    }

    #[test]
    fn test_scaleway_load_balancer_ip_allocations_try_new_rejects_non_uuid() {
        let result = ScwLoadBalancerIpAllocation::try_new("fr-par-1/11111111-2222-3333-4444-555555555555".into());
        assert!(matches!(result, Err(ScwLoadBalancerIpAllocationError::InvalidUuid)));
    }
}
