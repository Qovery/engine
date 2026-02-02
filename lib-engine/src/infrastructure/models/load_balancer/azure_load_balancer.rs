use std::collections::HashMap;

use crate::infrastructure::models::load_balancer::InteractWithLoadBalancer;

/// Azure load balancer configuration.
pub struct AzureLoadBalancer {}

impl InteractWithLoadBalancer for AzureLoadBalancer {
    fn annotations(&self) -> Option<HashMap<String, String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_azure_load_balancer_annotations_returns_none() {
        let lb = AzureLoadBalancer {};
        let annotations = lb.annotations();
        assert!(annotations.is_none());
    }

    #[test]
    fn test_azure_load_balancer_can_be_created() {
        let _lb = AzureLoadBalancer {};
    }
}
