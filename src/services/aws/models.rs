use async_trait::async_trait;
use aws_sdk_docdb::operation::describe_db_clusters::{DescribeDBClustersError, DescribeDbClustersOutput};
use aws_sdk_ec2::operation::describe_instances::{DescribeInstancesError, DescribeInstancesOutput};
use aws_sdk_ec2::operation::describe_volumes::{DescribeVolumesError, DescribeVolumesOutput};
use aws_sdk_ec2::operation::detach_volume::{DetachVolumeError, DetachVolumeOutput};
use aws_sdk_eks::operation::delete_nodegroup::{DeleteNodegroupError, DeleteNodegroupOutput};
use aws_sdk_eks::operation::describe_cluster::{DescribeClusterError, DescribeClusterOutput};
use aws_sdk_eks::operation::describe_nodegroup::{DescribeNodegroupError, DescribeNodegroupOutput};
use aws_sdk_eks::operation::list_clusters::{ListClustersError, ListClustersOutput};
use aws_sdk_eks::operation::list_nodegroups::{ListNodegroupsError, ListNodegroupsOutput};
use aws_sdk_elasticache::operation::describe_cache_clusters::{
    DescribeCacheClustersError, DescribeCacheClustersOutput,
};
use aws_sdk_elasticloadbalancingv2::operation::delete_load_balancer::DeleteLoadBalancerError;
use aws_sdk_elasticloadbalancingv2::operation::describe_load_balancers::{
    DescribeLoadBalancersError, DescribeLoadBalancersOutput,
};
use aws_sdk_elasticloadbalancingv2::operation::describe_tags::DescribeTagsError;
use aws_sdk_elasticloadbalancingv2::types::{LoadBalancer, TagDescription};
use aws_sdk_iam::operation::create_service_linked_role::{CreateServiceLinkedRoleError, CreateServiceLinkedRoleOutput};
use aws_sdk_iam::operation::get_role::{GetRoleError, GetRoleOutput};
use aws_sdk_rds::operation::describe_db_instances::{DescribeDBInstancesError, DescribeDbInstancesOutput};

use crate::errors::EngineError;
use crate::events::EventDetails;

#[async_trait]
pub trait QoveryAwsSdkConfigLoadBalancer {
    async fn list_all_aws_load_balancers(
        &self,
    ) -> Result<DescribeLoadBalancersOutput, aws_sdk_elasticloadbalancingv2::error::SdkError<DescribeLoadBalancersError>>;
    async fn get_aws_load_balancers_tags(
        &self,
        load_balancers: Vec<LoadBalancer>,
    ) -> Result<Vec<TagDescription>, aws_sdk_elasticloadbalancingv2::error::SdkError<DescribeTagsError>>;
    async fn delete_aws_load_balancer(
        &self,
        load_balancer_arn: String,
    ) -> Result<(), aws_sdk_elasticloadbalancingv2::error::SdkError<DeleteLoadBalancerError>>;
}

#[async_trait]
pub trait QoveryAwsSdkConfigEks {
    async fn list_clusters(&self) -> Result<ListClustersOutput, aws_sdk_eks::error::SdkError<ListClustersError>>;
    async fn describe_cluster(
        &self,
        cluster_id: String,
    ) -> Result<DescribeClusterOutput, aws_sdk_eks::error::SdkError<DescribeClusterError>>;
    async fn list_all_eks_nodegroups(
        &self,
        cluster_id: String,
    ) -> Result<ListNodegroupsOutput, aws_sdk_eks::error::SdkError<ListNodegroupsError>>;
    async fn describe_nodegroup(
        &self,
        cluster_id: String,
        nodegroup_id: String,
    ) -> Result<DescribeNodegroupOutput, aws_sdk_eks::error::SdkError<DescribeNodegroupError>>;
    async fn describe_nodegroups(
        &self,
        cluster_id: String,
        nodegroups: ListNodegroupsOutput,
    ) -> Result<Vec<DescribeNodegroupOutput>, aws_sdk_eks::error::SdkError<DescribeNodegroupError>>;
    async fn delete_nodegroup(
        &self,
        cluster_id: String,
        nodegroup_id: String,
    ) -> Result<DeleteNodegroupOutput, aws_sdk_eks::error::SdkError<DeleteNodegroupError>>;

    async fn get_role(&self, name: &str) -> Result<GetRoleOutput, aws_sdk_eks::error::SdkError<GetRoleError>>;

    async fn create_service_linked_role(
        &self,
        name: &str,
    ) -> Result<CreateServiceLinkedRoleOutput, aws_sdk_eks::error::SdkError<CreateServiceLinkedRoleError>>;
}

#[async_trait]
pub trait QoveryAwsSdkConfigManagedDatabase {
    async fn find_managed_rds_database(
        &self,
        db_id: &str,
    ) -> Result<DescribeDbInstancesOutput, aws_sdk_rds::error::SdkError<DescribeDBInstancesError>>;
    async fn find_managed_elasticache_database(
        &self,
        db_id: &str,
    ) -> Result<DescribeCacheClustersOutput, aws_sdk_elasticache::error::SdkError<DescribeCacheClustersError>>;
    async fn find_managed_doc_db_database(
        &self,
        db_id: &str,
    ) -> Result<DescribeDbClustersOutput, aws_sdk_docdb::error::SdkError<DescribeDBClustersError>>;
}

#[async_trait]
pub trait QoveryAwsSdkConfigEc2 {
    async fn get_volume_by_instance_id(
        &self,
        instance_id: String,
    ) -> Result<DescribeVolumesOutput, aws_sdk_ec2::error::SdkError<DescribeVolumesError>>;
    async fn detach_instance_volume(
        &self,
        volume_id: String,
    ) -> Result<DetachVolumeOutput, aws_sdk_ec2::error::SdkError<DetachVolumeError>>;
    async fn _get_instance_by_id(
        &self,
        instance_id: String,
    ) -> Result<DescribeInstancesOutput, aws_sdk_ec2::error::SdkError<DescribeInstancesError>>;
    async fn detach_ec2_volumes(&self, instance_id: &str, event_details: &EventDetails)
        -> Result<(), Box<EngineError>>;
}
