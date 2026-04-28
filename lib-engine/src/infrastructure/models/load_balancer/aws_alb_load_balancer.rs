use crate::{
    infrastructure::models::{cloud_provider::io::AwsAlbLoadBalancerScheme, load_balancer::InteractWithLoadBalancer},
    io_models::QoveryIdentifier,
};
use ipnet::IpNet;
use itertools::Itertools;
use std::collections::HashMap;

mod models {
    use std::fmt::{Display, Formatter};

    use crate::infrastructure::models::cloud_provider::io::LoadBalancerIpAllocationId;
    use thiserror::Error;

    #[derive(Debug, Error, PartialEq, Eq)]
    pub enum AwsEipAllocationIdError {
        #[error("invalid AWS EIP allocation id `{0}`: expected prefix `eipalloc-`")]
        MissingPrefix(String),
        #[error("invalid AWS EIP allocation id `{0}`: expected `eipalloc-` followed by 8 or 17 hex characters")]
        InvalidSuffix(String),
    }

    #[derive(Clone, Debug)]
    pub struct AwsEipAllocationId {
        id: String,
    }

    impl AwsEipAllocationId {
        pub fn try_new(id: LoadBalancerIpAllocationId) -> Result<Self, AwsEipAllocationIdError> {
            let raw_id = id.as_str();
            let suffix = raw_id
                .strip_prefix("eipalloc-")
                .ok_or_else(|| AwsEipAllocationIdError::MissingPrefix(raw_id.to_string()))?;

            // AWS IDs are hex-encoded. Historically short IDs (8 chars) and long IDs (17 chars) both exist.
            // Documentation:
            // https://docs.aws.amazon.com/elasticloadbalancing/latest/network/network-load-balancers.html
            // https://kubernetes-sigs.github.io/aws-load-balancer-controller/latest/guide/service/annotations/
            let has_valid_len = suffix.len() == 8 || suffix.len() == 17;
            let is_hex = suffix.chars().all(|c| c.is_ascii_hexdigit());
            if !has_valid_len || !is_hex {
                return Err(AwsEipAllocationIdError::InvalidSuffix(raw_id.to_string()));
            }

            Ok(Self { id: raw_id.to_string() })
        }
    }

    impl Display for AwsEipAllocationId {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.id)
        }
    }

    impl AwsEipAllocationId {
        pub fn as_str(&self) -> &str {
            &self.id
        }
    }
}

pub use models::{AwsEipAllocationId, AwsEipAllocationIdError};

/// AWS Application Load Balancer (ALB) configuration.
pub struct AwsAlbLoadBalancer {
    pub cluster_id: QoveryIdentifier,
    pub organization_id: QoveryIdentifier,
    pub load_balancer_source_ranges: Vec<IpNet>,
    pub load_balancer_eip_allocation_ids: Option<Vec<AwsEipAllocationId>>,
    pub load_balancer_scheme: AwsAlbLoadBalancerScheme,
}

impl InteractWithLoadBalancer for AwsAlbLoadBalancer {
    fn annotations(&self) -> Option<HashMap<String, String>> {
        let mut annotations = HashMap::from([
            (
                "service.beta.kubernetes.io/aws-load-balancer-type".to_string(),
                "external".to_string(),
            ),
            (
                "service.beta.kubernetes.io/aws-load-balancer-scheme".to_string(),
                self.load_balancer_scheme.to_string(),
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
            // Default settings, no need to declare those, but just in case
            // (
            //     "service.beta.kubernetes.io/aws-load-balancer-target-group-attributes".to_string(),
            //     "target_health_state.unhealthy.connection_termination.enabled=false,target_health_state.unhealthy.draining_interval_seconds=300".to_string()
            // ),
        ]);

        if !self.load_balancer_source_ranges.is_empty() {
            annotations.insert(
                "service.beta.kubernetes.io/load-balancer-source-ranges".to_string(),
                self.load_balancer_source_ranges
                    .iter()
                    .map(|ip| ip.to_string())
                    .collect_vec()
                    .join("\\,"),
            );
        }

        if let Some(load_balancer_eip_allocation_ids) = &self.load_balancer_eip_allocation_ids {
            annotations.insert(
                "service.beta.kubernetes.io/aws-load-balancer-eip-allocations".to_string(),
                load_balancer_eip_allocation_ids
                    .iter()
                    .map(AwsEipAllocationId::as_str)
                    .join("\\,"),
            );
        }

        Some(annotations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::models::load_balancer::aws_alb_load_balancer::models::AwsEipAllocationId;
    use uuid::Uuid;

    fn parse_ids(ids: Vec<&str>) -> Result<Option<Vec<AwsEipAllocationId>>, AwsEipAllocationIdError> {
        if ids.is_empty() {
            return Ok(None);
        }
        ids.into_iter()
            .map(|id| AwsEipAllocationId::try_new(id.into()))
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    #[test]
    fn test_aws_eip_allocation_id_try_new_accepts_valid_formats() {
        assert!(AwsEipAllocationId::try_new("eipalloc-deadbeef".into()).is_ok());
        assert!(AwsEipAllocationId::try_new("eipalloc-0123456789abcdef0".into()).is_ok());
    }

    #[test]
    fn test_aws_eip_allocation_id_try_new_rejects_invalid_formats() {
        assert!(matches!(
            AwsEipAllocationId::try_new("eip-deadbeef".into()),
            Err(AwsEipAllocationIdError::MissingPrefix(_))
        ));
        assert!(matches!(
            AwsEipAllocationId::try_new("eipalloc-xyz".into()),
            Err(AwsEipAllocationIdError::InvalidSuffix(_))
        ));
        assert!(matches!(
            AwsEipAllocationId::try_new("eipalloc-0123456789abcdef".into()),
            Err(AwsEipAllocationIdError::InvalidSuffix(_))
        )); // 16 chars
        assert!(matches!(
            AwsEipAllocationId::try_new("eipalloc-deadbee".into()),
            Err(AwsEipAllocationIdError::InvalidSuffix(_))
        )); // 7 chars
        assert!(matches!(
            AwsEipAllocationId::try_new("eipalloc-deadbeef0".into()),
            Err(AwsEipAllocationIdError::InvalidSuffix(_))
        )); // 9 chars
        assert!(matches!(
            AwsEipAllocationId::try_new("eipalloc-0123456789abcdef01".into()),
            Err(AwsEipAllocationIdError::InvalidSuffix(_))
        )); // 18 chars
        assert!(matches!(
            AwsEipAllocationId::try_new("EIPALLOC-deadbeef".into()),
            Err(AwsEipAllocationIdError::MissingPrefix(_))
        )); // uppercase prefix not allowed
    }

    #[test]
    fn test_aws_eip_allocation_id_try_new_accepts_uppercase_hex_suffix() {
        assert!(AwsEipAllocationId::try_new("eipalloc-DEADBEEF".into()).is_ok());
        assert!(AwsEipAllocationId::try_new("eipalloc-0123456789ABCDEF0".into()).is_ok());
    }

    #[test]
    fn test_aws_alb_load_balancer_annotations_returns_some() {
        let cluster_id = QoveryIdentifier::new(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap());
        let organization_id = QoveryIdentifier::new(Uuid::parse_str("987fcdeb-51a2-43f7-b123-456789abcdef").unwrap());

        let lb = AwsAlbLoadBalancer {
            cluster_id: cluster_id.clone(),
            organization_id: organization_id.clone(),
            load_balancer_source_ranges: vec![],
            load_balancer_eip_allocation_ids: None,
            load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
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
            load_balancer_source_ranges: vec![],
            load_balancer_eip_allocation_ids: None,
            load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
        };

        let annotations = lb.annotations().unwrap();

        // Verify count
        assert_eq!(annotations.len(), 7);

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

        let lb = AwsAlbLoadBalancer {
            cluster_id: cluster_id.clone(),
            organization_id: organization_id.clone(),
            load_balancer_source_ranges: vec![],
            load_balancer_eip_allocation_ids: None,
            load_balancer_scheme: AwsAlbLoadBalancerScheme::Internal,
        };

        let annotations = lb.annotations().unwrap();
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/aws-load-balancer-scheme"),
            Some(&"internal".to_string())
        );
    }

    #[test]
    fn test_aws_alb_load_balancer_respects_max_8_annotations() {
        let cluster_id = QoveryIdentifier::new_random();
        let organization_id = QoveryIdentifier::new_random();

        let lb = AwsAlbLoadBalancer {
            cluster_id,
            organization_id,
            load_balancer_source_ranges: vec![],
            load_balancer_eip_allocation_ids: None,
            load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
        };

        let annotations = lb.annotations().unwrap();
        assert!(
            annotations.len() <= 8,
            "AWS ALB load balancer must have at most 8 annotations, got {}",
            annotations.len()
        );
    }

    #[test]
    fn test_aws_alb_load_balancer_load_balancer_source_ranges_annotation() {
        let cluster_id = QoveryIdentifier::new_random();
        let organization_id = QoveryIdentifier::new_random();

        let lb = AwsAlbLoadBalancer {
            cluster_id,
            organization_id,
            load_balancer_source_ranges: vec![
                "10.0.0.0/8".parse().unwrap(),
                "192.168.1.0/24".parse().unwrap(),
                "fd01::/64".parse().unwrap(),
            ],
            load_balancer_eip_allocation_ids: None,
            load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
        };

        let annotations = lb.annotations().unwrap();
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/load-balancer-source-ranges"),
            Some(&"10.0.0.0/8\\,192.168.1.0/24\\,fd01::/64".to_string())
        );
    }

    #[test]
    fn test_aws_alb_load_balancer_no_source_ranges_annotation_when_empty() {
        let cluster_id = QoveryIdentifier::new_random();
        let organization_id = QoveryIdentifier::new_random();

        let lb = AwsAlbLoadBalancer {
            cluster_id,
            organization_id,
            load_balancer_source_ranges: vec![],
            load_balancer_eip_allocation_ids: None,
            load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
        };

        let annotations = lb.annotations().unwrap();
        assert!(!annotations.contains_key("service.beta.kubernetes.io/load-balancer-source-ranges"));
    }

    #[test]
    fn test_aws_alb_load_balancer_ip_allocation_ids_annotation_when_set() {
        let cluster_id = QoveryIdentifier::new_random();
        let organization_id = QoveryIdentifier::new_random();

        let lb = AwsAlbLoadBalancer {
            cluster_id,
            organization_id,
            load_balancer_source_ranges: vec![],
            load_balancer_eip_allocation_ids: parse_ids(vec![
                "eipalloc-0123456789abcdef0",
                "eipalloc-abcdef01234567890",
            ])
            .unwrap(),
            load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
        };

        let annotations = lb.annotations().unwrap();
        assert_eq!(
            annotations.get("service.beta.kubernetes.io/aws-load-balancer-eip-allocations"),
            Some(&"eipalloc-0123456789abcdef0\\,eipalloc-abcdef01234567890".to_string())
        );
    }
}
