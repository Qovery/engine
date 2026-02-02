use std::collections::HashMap;

use enum_dispatch::enum_dispatch;

pub mod aws_alb_load_balancer;
pub mod azure_load_balancer;
pub mod gcp_load_balancer;
pub mod scaleway_load_balancer;

/// Trait for interacting with cloud provider load balancers.
/// Provides a unified interface to get provider-specific Kubernetes service annotations.
#[enum_dispatch]
pub trait InteractWithLoadBalancer: Send + Sync {
    /// Returns Kubernetes service annotations for configuring the load balancer.
    /// Returns None if no specific annotations are needed for this provider.
    fn annotations(&self) -> Option<HashMap<String, String>>;
}

/// Cloud provider-specific load balancer configuration.
/// Uses enum_dispatch for efficient trait dispatch without dynamic dispatch overhead.
///
/// Each variant contains provider-specific configuration that translates to
/// Kubernetes service annotations.
#[enum_dispatch(InteractWithLoadBalancer)]
pub enum LoadBalancer {
    AwsAlb(aws_alb_load_balancer::AwsAlbLoadBalancer),
    Gcp(gcp_load_balancer::GcpLoadBalancer),
    Azure(azure_load_balancer::AzureLoadBalancer),
    Scaleway(scaleway_load_balancer::ScalewayLoadBalancer),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io_models::QoveryIdentifier;
    use uuid::Uuid;

    #[test]
    fn test_load_balancer_enum_dispatch_aws_alb() {
        let cluster_id = QoveryIdentifier::new(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap());
        let organization_id = QoveryIdentifier::new_random();

        let aws_lb = aws_alb_load_balancer::AwsAlbLoadBalancer {
            cluster_id,
            organization_id,
        };
        let lb = LoadBalancer::AwsAlb(aws_lb);

        let annotations = lb.annotations();
        assert!(annotations.is_some());
        let annotations = annotations.unwrap();
        assert!(!annotations.is_empty());
        assert!(annotations.contains_key("service.beta.kubernetes.io/aws-load-balancer-type"));
    }

    #[test]
    fn test_load_balancer_enum_dispatch_gcp() {
        let gcp_lb = gcp_load_balancer::GcpLoadBalancer {};
        let lb = LoadBalancer::Gcp(gcp_lb);

        let annotations = lb.annotations();
        assert!(annotations.is_none());
    }

    #[test]
    fn test_load_balancer_enum_dispatch_azure() {
        let azure_lb = azure_load_balancer::AzureLoadBalancer {};
        let lb = LoadBalancer::Azure(azure_lb);

        let annotations = lb.annotations();
        assert!(annotations.is_none());
    }

    #[test]
    fn test_load_balancer_enum_dispatch_scaleway() {
        let scaleway_lb = scaleway_load_balancer::ScalewayLoadBalancer {
            size: Some("LB-M".to_string()),
        };
        let lb = LoadBalancer::Scaleway(scaleway_lb);

        let annotations = lb.annotations();
        assert!(annotations.is_some());
        let annotations = annotations.unwrap();
        assert!(!annotations.is_empty());
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/scw-loadbalancer-type"),
            Some(&"LB-M".to_string())
        );
    }

    #[test]
    fn test_load_balancer_trait_object() {
        let cluster_id = QoveryIdentifier::new_random();
        let organization_id = QoveryIdentifier::new_random();

        let aws_lb = aws_alb_load_balancer::AwsAlbLoadBalancer {
            cluster_id,
            organization_id,
        };

        let trait_obj: &dyn InteractWithLoadBalancer = &aws_lb;
        let annotations = trait_obj.annotations();
        assert!(annotations.is_some());
    }

    #[test]
    fn test_load_balancer_enum_variants_exist() {
        let cluster_id = QoveryIdentifier::new_random();
        let organization_id = QoveryIdentifier::new_random();

        let _aws = LoadBalancer::AwsAlb(aws_alb_load_balancer::AwsAlbLoadBalancer {
            cluster_id,
            organization_id,
        });
        let _gcp = LoadBalancer::Gcp(gcp_load_balancer::GcpLoadBalancer {});
        let _azure = LoadBalancer::Azure(azure_load_balancer::AzureLoadBalancer {});
        let _scaleway = LoadBalancer::Scaleway(scaleway_load_balancer::ScalewayLoadBalancer { size: None });
    }
}
