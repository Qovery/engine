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
