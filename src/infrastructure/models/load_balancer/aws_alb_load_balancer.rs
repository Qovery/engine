use crate::{infrastructure::models::load_balancer::InteractWithLoadBalancer, io_models::QoveryIdentifier};
use std::collections::HashMap;

/// AWS Application Load Balancer (ALB) configuration.
pub struct AwsAlbLoadBalancer {
    pub cluster_id: QoveryIdentifier,
    pub organization_id: QoveryIdentifier,
}

impl InteractWithLoadBalancer for AwsAlbLoadBalancer {
    fn annotations(&self) -> Option<HashMap<String, String>> {
        Some(HashMap::from([
            (
                "service.beta.kubernetes.io/aws-load-balancer-type".to_string(),
                "external".to_string(),
            ),
            (
                "service.beta.kubernetes.io/aws-load-balancer-scheme".to_string(),
                "internet-facing".to_string(),
            ),
            (
                "service.beta.kubernetes.io/aws-load-balancer-name".to_string(),
                format!("qovery-{}-gateway", self.cluster_id.short()),
            ),
            (
                "service.beta.kubernetes.io/aws-load-balancer-nlb-target-type".to_string(),
                "ip".to_string(),
            ),
            (
                "service.beta.kubernetes.io/aws-load-balancer-cross-zone-load-balancing-enabled".to_string(),
                "true".to_string(),
            ),
            (
                "service.beta.kubernetes.io/aws-load-balancer-healthcheck-interval".to_string(),
                "10".to_string(),
            ),
            (
                "service.beta.kubernetes.io/aws-load-balancer-additional-resource-tags".to_string(),
                format!(
                    "OrganizationLongId={}\\,OrganizationId={}\\,ClusterLongId={}\\,ClusterId={}",
                    self.organization_id,
                    self.organization_id.short(),
                    self.cluster_id,
                    self.cluster_id.short(),
                ),
            ),
            (
                "service.beta.kubernetes.io/aws-load-balancer-proxy-protocol".to_string(),
                "*".to_string(),
            ),
            // Validation rule, must have max 8 annotations
            // spec.infrastructure.annotations: Too many: 10: must have at most 8 items
            // ("service.beta.kubernetes.io/aws-load-balancer-target-group-attributes".to_string(), "target_health_state.unhealthy.connection_termination.enabled=false,target_health_state.unhealthy.draining_interval_seconds=300".to_string()), // Can use AWS defaults or set via AWS Load Balancer Controller configuration
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_aws_alb_load_balancer_annotations_returns_some() {
        let cluster_id = QoveryIdentifier::new(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap());
        let organization_id = QoveryIdentifier::new(Uuid::parse_str("987fcdeb-51a2-43f7-b123-456789abcdef").unwrap());

        let lb = AwsAlbLoadBalancer {
            cluster_id: cluster_id.clone(),
            organization_id: organization_id.clone(),
        };

        let annotations = lb.annotations();
        assert!(annotations.is_some());
    }

    #[test]
    fn test_aws_alb_load_balancer_annotations_keys_and_values() {
        let cluster_id = QoveryIdentifier::new(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap());
        let organization_id = QoveryIdentifier::new(Uuid::parse_str("987fcdeb-51a2-43f7-b123-456789abcdef").unwrap());

        let lb = AwsAlbLoadBalancer {
            cluster_id: cluster_id.clone(),
            organization_id: organization_id.clone(),
        };

        let annotations = lb.annotations().unwrap();

        // Verify count
        assert_eq!(annotations.len(), 8);

        // Verify static annotation values
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/aws-load-balancer-type"),
            Some(&"external".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/aws-load-balancer-scheme"),
            Some(&"internet-facing".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/aws-load-balancer-nlb-target-type"),
            Some(&"ip".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/aws-load-balancer-cross-zone-load-balancing-enabled"),
            Some(&"true".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/aws-load-balancer-healthcheck-interval"),
            Some(&"10".to_string())
        );
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/aws-load-balancer-proxy-protocol"),
            Some(&"*".to_string())
        );

        // Verify load balancer name includes cluster ID
        let name = annotations
            .get("service.beta.kubernetes.io/aws-load-balancer-name")
            .unwrap();
        assert!(name.contains(cluster_id.short()));
        assert!(name.starts_with("qovery-"));
        assert!(name.ends_with("-gateway"));

        // Verify tags include organization and cluster IDs
        let tags = annotations
            .get("service.beta.kubernetes.io/aws-load-balancer-additional-resource-tags")
            .unwrap();
        assert!(tags.contains(&format!("OrganizationLongId={}", organization_id)));
        assert!(tags.contains(&format!("OrganizationId={}", organization_id.short())));
        assert!(tags.contains(&format!("ClusterLongId={}", cluster_id)));
        assert!(tags.contains(&format!("ClusterId={}", cluster_id.short())));
        assert!(tags.contains("\\,"));
    }

    #[test]
    fn test_aws_alb_load_balancer_respects_max_8_annotations() {
        let cluster_id = QoveryIdentifier::new_random();
        let organization_id = QoveryIdentifier::new_random();

        let lb = AwsAlbLoadBalancer {
            cluster_id,
            organization_id,
        };

        let annotations = lb.annotations().unwrap();
        assert!(
            annotations.len() <= 8,
            "AWS ALB load balancer must have at most 8 annotations, got {}",
            annotations.len()
        );
    }
}
