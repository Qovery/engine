use std::collections::HashMap;

use crate::infrastructure::models::load_balancer::InteractWithLoadBalancer;

/// Scaleway load balancer configuration.
pub struct ScalewayLoadBalancer {
    /// Load balancer size (e.g., "LB-S", "LB-M", "LB-L"). Defaults to "LB-S" if None.
    pub size: Option<String>,
}

impl InteractWithLoadBalancer for ScalewayLoadBalancer {
    fn annotations(&self) -> Option<HashMap<String, String>> {
        Some(std::collections::HashMap::from([
            (
                "service.beta.kubernetes.io/scw-loadbalancer-type".to_string(),
                self.size.as_ref().map_or("LB-S", |v| v).to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-forward-port-algorithm".to_string(),
                "leastconn".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-protocol-http".to_string(),
                "false".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v1".to_string(),
                "false".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v2".to_string(),
                "true".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-health-check-type".to_string(),
                "80:http;443:https".to_string(),
            ),
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
                "service.beta.kubernetes.io/scw-loadbalancer-health-check-delay".to_string(),
                "2s".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-health-check-timeout".to_string(),
                "2s".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-health-check-max-retries".to_string(),
                "2".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-redispatch-attempt-count".to_string(),
                "1".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-timeout-server".to_string(),
                "30s".to_string(),
            ),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaleway_load_balancer_annotations_returns_some() {
        let lb = ScalewayLoadBalancer { size: None };
        let annotations = lb.annotations();
        assert!(annotations.is_some());
    }

    #[test]
    fn test_scaleway_load_balancer_annotations_default_size() {
        let lb = ScalewayLoadBalancer { size: None };
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
        };
        let annotations = lb.annotations().unwrap();

        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-type"),
            Some(&"LB-L".to_string())
        );
    }

    #[test]
    fn test_scaleway_load_balancer_annotations_keys_and_values() {
        let lb = ScalewayLoadBalancer { size: None };
        let annotations = lb.annotations().unwrap();

        // Verify count
        assert_eq!(annotations.len(), 14);

        // Verify basic configuration
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-forward-port-algorithm"),
            Some(&"leastconn".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-protocol-http"),
            Some(&"false".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-use-hostname"),
            Some(&"true".to_string())
        );

        // Verify proxy protocol settings
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v1"),
            Some(&"false".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v2"),
            Some(&"true".to_string())
        );

        // Verify health check configuration
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-type"),
            Some(&"80:http;443:https".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-http-uri"),
            Some(&"80:/healthz;443:/healthz".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-send-proxy"),
            Some(&"true".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-delay"),
            Some(&"2s".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-timeout"),
            Some(&"2s".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-health-check-max-retries"),
            Some(&"2".to_string())
        );

        // Verify timeout and redispatch settings
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-redispatch-attempt-count"),
            Some(&"1".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-timeout-server"),
            Some(&"30s".to_string())
        );
    }
}
