use crate::cloud_provider::models::IngressLoadBalancerType;
use crate::errors::CommandError;
use async_trait::async_trait;
use aws_sdk_eks::{
    error::{DeleteNodegroupError, DescribeClusterError, DescribeNodegroupError, ListClustersError},
    output::{
        DeleteNodegroupOutput, DescribeClusterOutput, DescribeNodegroupOutput, ListClustersOutput, ListNodegroupsOutput,
    },
};
use aws_sdk_elasticloadbalancingv2::{
    error::{DeleteLoadBalancerError, DescribeTagsError},
    model::{LoadBalancer, TagDescription},
};

use aws_smithy_client::SdkError;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(Clone, Debug, EnumIter, Eq, PartialEq)]
pub enum AwsLoadBalancerType {
    Nlb,
}

impl Display for AwsLoadBalancerType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AwsLoadBalancerType::Nlb => "nlb",
        })
    }
}

impl FromStr for AwsLoadBalancerType {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "nlb" => Ok(AwsLoadBalancerType::Nlb),
            _ => Err(CommandError::new_from_safe_message(format!(
                "`{}` load balancer type is not supported",
                s
            ))),
        }
    }
}

impl IngressLoadBalancerType for AwsLoadBalancerType {
    fn annotation_key(&self) -> String {
        "service.beta.kubernetes.io/aws-load-balancer-type".to_string()
    }

    fn annotation_value(&self) -> String {
        self.to_string()
    }
}

#[async_trait]
pub trait QoveryAwsSdkConfigLoadBalancer {
    async fn list_all_aws_load_balancers(
        &self,
    ) -> Result<
        aws_sdk_elasticloadbalancingv2::output::DescribeLoadBalancersOutput,
        aws_smithy_client::SdkError<aws_sdk_elasticloadbalancingv2::error::DescribeLoadBalancersError>,
    >;
    async fn get_aws_load_balancers_tags(
        &self,
        load_balancers: Vec<LoadBalancer>,
    ) -> Result<Vec<TagDescription>, SdkError<DescribeTagsError>>;
    async fn delete_aws_load_balancer(
        &self,
        load_balancer_arn: String,
    ) -> Result<(), SdkError<DeleteLoadBalancerError>>;
}

#[async_trait]
pub trait QoveryAwsSdkConfigEks {
    async fn list_clusters(&self) -> Result<ListClustersOutput, SdkError<ListClustersError>>;
    async fn describe_cluster(
        &self,
        cluster_id: String,
    ) -> Result<DescribeClusterOutput, SdkError<DescribeClusterError>>;
    async fn list_all_eks_nodegroups(
        &self,
        cluster_id: String,
    ) -> Result<ListNodegroupsOutput, SdkError<aws_sdk_eks::error::ListNodegroupsError>>;
    async fn describe_nodegroup(
        &self,
        cluster_id: String,
        nodegroup_id: String,
    ) -> Result<DescribeNodegroupOutput, SdkError<aws_sdk_eks::error::DescribeNodegroupError>>;
    async fn describe_nodegroups(
        &self,
        cluster_id: String,
        nodegroups: ListNodegroupsOutput,
    ) -> Result<Vec<DescribeNodegroupOutput>, SdkError<DescribeNodegroupError>>;
    async fn delete_nodegroup(
        &self,
        cluster_id: String,
        nodegroup_id: String,
    ) -> Result<DeleteNodegroupOutput, SdkError<DeleteNodegroupError>>;
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::models::AwsLoadBalancerType;
    use crate::cloud_provider::models::IngressLoadBalancerType;
    use crate::errors::CommandError;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_aws_load_balancer_type_to_string() {
        for lb_type in AwsLoadBalancerType::iter() {
            // execute:
            let res = lb_type.to_string();

            // verify:
            assert_eq!(
                match lb_type {
                    AwsLoadBalancerType::Nlb => "nlb",
                },
                res
            );
        }
    }

    #[test]
    fn test_aws_load_balancer_type_from_str() {
        // setup:
        struct TestCase<'a> {
            input: &'a str,
            expected: Result<AwsLoadBalancerType, CommandError>,
        }

        let test_cases = vec![
            TestCase {
                input: "wrong",
                expected: Err(CommandError::new_from_safe_message(
                    "`wrong` load balancer type is not supported".to_string(),
                )),
            },
            TestCase {
                input: "nlb",
                expected: Ok(AwsLoadBalancerType::Nlb),
            },
            TestCase {
                input: "Nlb",
                expected: Ok(AwsLoadBalancerType::Nlb),
            },
            TestCase {
                input: "NLB",
                expected: Ok(AwsLoadBalancerType::Nlb),
            },
        ];

        for tc in test_cases {
            // execute:
            let res = AwsLoadBalancerType::from_str(tc.input);

            // verify:
            assert_eq!(tc.expected, res,);
        }
    }

    #[test]
    fn test_aws_load_balancer_type_annotation_key() {
        for lb_type in AwsLoadBalancerType::iter() {
            // execute:
            let res = lb_type.annotation_key();

            // verify:
            assert_eq!("service.beta.kubernetes.io/aws-load-balancer-type".to_string(), res);
        }
    }

    #[test]
    fn test_aws_load_balancer_type_annotation_value() {
        for lb_type in AwsLoadBalancerType::iter() {
            // execute:
            let res = lb_type.annotation_value();

            // verify:
            assert_eq!(lb_type.to_string(), res);
        }
    }
}
