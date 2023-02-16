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

use crate::errors::EngineError;
use crate::events::EventDetails;
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

#[async_trait]
pub trait QoveryAwsSdkConfigManagedDatabase {
    async fn find_managed_rds_database(
        &self,
        db_id: &str,
    ) -> Result<
        aws_sdk_rds::output::DescribeDbInstancesOutput,
        aws_smithy_client::SdkError<aws_sdk_rds::error::DescribeDBInstancesError>,
    >;
    async fn find_managed_elasticache_database(
        &self,
        db_id: &str,
    ) -> Result<
        aws_sdk_elasticache::output::DescribeCacheClustersOutput,
        aws_smithy_client::SdkError<aws_sdk_elasticache::error::DescribeCacheClustersError>,
    >;
    async fn find_managed_doc_db_database(
        &self,
        db_id: &str,
    ) -> Result<
        aws_sdk_docdb::output::DescribeDbClustersOutput,
        aws_smithy_client::SdkError<aws_sdk_docdb::error::DescribeDBClustersError>,
    >;
}

#[async_trait]
pub trait QoveryAwsSdkConfigEc2 {
    async fn get_volume_by_instance_id(
        &self,
        instance_id: String,
    ) -> Result<aws_sdk_ec2::output::DescribeVolumesOutput, SdkError<aws_sdk_ec2::error::DescribeVolumesError>>;
    async fn detach_instance_volume(
        &self,
        volume_id: String,
    ) -> Result<aws_sdk_ec2::output::DetachVolumeOutput, SdkError<aws_sdk_ec2::error::DetachVolumeError>>;
    async fn detach_ec2_volumes(&self, instance_id: &str, event_details: &EventDetails)
        -> Result<(), Box<EngineError>>;
}
