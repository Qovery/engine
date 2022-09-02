use async_trait::async_trait;
use aws_sdk_elasticloadbalancingv2::{
    error::{DeleteLoadBalancerError, DescribeTagsError},
    model::{LoadBalancer, TagDescription},
};
use aws_smithy_client::SdkError;

#[async_trait]
pub trait QoveryAwsSdkConfig {
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
