#[async_trait]
trait QoveryAwsSdkConfig {
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
}
