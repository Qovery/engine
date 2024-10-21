use async_trait::async_trait;
use aws_sdk_docdb::operation::describe_db_clusters::{DescribeDBClustersError, DescribeDbClustersOutput};
use aws_sdk_elasticache::operation::describe_cache_clusters::{
    DescribeCacheClustersError, DescribeCacheClustersOutput,
};
use aws_sdk_elasticloadbalancingv2::operation::delete_load_balancer::DeleteLoadBalancerError;
use aws_sdk_elasticloadbalancingv2::operation::describe_load_balancers::{
    DescribeLoadBalancersError, DescribeLoadBalancersOutput,
};
use aws_sdk_elasticloadbalancingv2::operation::describe_tags::DescribeTagsError;
use aws_sdk_elasticloadbalancingv2::types::{LoadBalancer, TagDescription};
use aws_sdk_rds::operation::describe_db_instances::{DescribeDBInstancesError, DescribeDbInstancesOutput};

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
