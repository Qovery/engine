use std::collections::HashMap;

use crate::infrastructure::models::load_balancer::InteractWithLoadBalancer;

/// GCP load balancer configuration.
pub struct GcpLoadBalancer {}

impl InteractWithLoadBalancer for GcpLoadBalancer {
    fn annotations(&self) -> Option<HashMap<String, String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gcp_load_balancer_annotations_returns_none() {
        let lb = GcpLoadBalancer {};
        let annotations = lb.annotations();
        assert!(annotations.is_none());
    }

    #[test]
    fn test_gcp_load_balancer_can_be_created() {
        let _lb = GcpLoadBalancer {};
    }
}
