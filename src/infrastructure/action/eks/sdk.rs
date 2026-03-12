use async_trait::async_trait;
use aws_sdk_ec2::operation::describe_subnets::{DescribeSubnetsError, DescribeSubnetsOutput};
use aws_sdk_eks::error::SdkError;
use aws_sdk_eks::operation::delete_nodegroup::{DeleteNodegroupError, DeleteNodegroupOutput};
use aws_sdk_eks::operation::describe_nodegroup::{DescribeNodegroupError, DescribeNodegroupOutput};
use aws_sdk_eks::operation::list_clusters::{ListClustersError, ListClustersOutput};
use aws_sdk_eks::operation::list_nodegroups::{ListNodegroupsError, ListNodegroupsOutput};
use aws_sdk_iam::operation::create_service_linked_role::{CreateServiceLinkedRoleError, CreateServiceLinkedRoleOutput};
use aws_sdk_iam::operation::get_role::{GetRoleError, GetRoleOutput};
use aws_types::SdkConfig;
use std::collections::HashMap;

#[async_trait]
pub trait QoveryAwsSdkConfigEks {
    async fn list_clusters(&self) -> Result<ListClustersOutput, SdkError<ListClustersError>>;
    async fn list_all_eks_nodegroups(
        &self,
        cluster_id: String,
    ) -> Result<ListNodegroupsOutput, SdkError<ListNodegroupsError>>;
    async fn describe_nodegroup(
        &self,
        cluster_id: String,
        nodegroup_id: String,
    ) -> Result<DescribeNodegroupOutput, SdkError<DescribeNodegroupError>>;
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

    async fn get_role(&self, name: &str) -> Result<GetRoleOutput, SdkError<GetRoleError>>;

    async fn create_service_linked_role(
        &self,
        name: &str,
    ) -> Result<CreateServiceLinkedRoleOutput, SdkError<CreateServiceLinkedRoleError>>;

    async fn describe_subnets_by_ids(
        &self,
        subnet_ids: Vec<String>,
    ) -> Result<DescribeSubnetsOutput, SdkError<DescribeSubnetsError>>;

    async fn describe_subnets_tags_by_ids(
        &self,
        subnet_ids: Vec<String>,
    ) -> Result<HashMap<String, HashMap<String, String>>, SdkError<DescribeSubnetsError>>;
}

#[async_trait]
impl QoveryAwsSdkConfigEks for SdkConfig {
    async fn list_clusters(&self) -> Result<ListClustersOutput, SdkError<ListClustersError>> {
        let client = aws_sdk_eks::Client::new(self);
        client.list_clusters().send().await
    }

    async fn list_all_eks_nodegroups(
        &self,
        cluster_name: String,
    ) -> Result<ListNodegroupsOutput, SdkError<ListNodegroupsError>> {
        let client = aws_sdk_eks::Client::new(self);
        client.list_nodegroups().cluster_name(cluster_name).send().await
    }

    async fn describe_nodegroup(
        &self,
        cluster_name: String,
        nodegroup_id: String,
    ) -> Result<DescribeNodegroupOutput, SdkError<DescribeNodegroupError>> {
        let client = aws_sdk_eks::Client::new(self);
        client
            .describe_nodegroup()
            .cluster_name(cluster_name)
            .nodegroup_name(nodegroup_id)
            .send()
            .await
    }

    async fn describe_nodegroups(
        &self,
        cluster_name: String,
        nodegroups: ListNodegroupsOutput,
    ) -> Result<Vec<DescribeNodegroupOutput>, SdkError<DescribeNodegroupError>> {
        let mut nodegroups_descriptions = Vec::new();

        for nodegroup in nodegroups.nodegroups.unwrap_or_default() {
            let nodegroup_description = self.describe_nodegroup(cluster_name.clone(), nodegroup).await;
            match nodegroup_description {
                Ok(x) => nodegroups_descriptions.push(x),
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(nodegroups_descriptions)
    }

    async fn delete_nodegroup(
        &self,
        cluster_name: String,
        nodegroup_name: String,
    ) -> Result<DeleteNodegroupOutput, SdkError<DeleteNodegroupError>> {
        let client = aws_sdk_eks::Client::new(self);
        client
            .delete_nodegroup()
            .cluster_name(cluster_name)
            .nodegroup_name(nodegroup_name)
            .send()
            .await
    }

    async fn get_role(&self, name: &str) -> Result<GetRoleOutput, SdkError<GetRoleError>> {
        let client = aws_sdk_iam::Client::new(self);
        client.get_role().role_name(name).send().await
    }

    async fn create_service_linked_role(
        &self,
        service_name: &str,
    ) -> Result<CreateServiceLinkedRoleOutput, SdkError<CreateServiceLinkedRoleError>> {
        let client = aws_sdk_iam::Client::new(self);
        client
            .create_service_linked_role()
            .aws_service_name(service_name)
            .send()
            .await
    }

    async fn describe_subnets_by_ids(
        &self,
        subnet_ids: Vec<String>,
    ) -> Result<DescribeSubnetsOutput, SdkError<DescribeSubnetsError>> {
        let client = aws_sdk_ec2::Client::new(self);
        client.describe_subnets().set_subnet_ids(Some(subnet_ids)).send().await
    }

    async fn describe_subnets_tags_by_ids(
        &self,
        subnet_ids: Vec<String>,
    ) -> Result<HashMap<String, HashMap<String, String>>, SdkError<DescribeSubnetsError>> {
        if subnet_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let subnets = self.describe_subnets_by_ids(subnet_ids).await?;
        let mut subnets_tags_by_id: HashMap<String, HashMap<String, String>> = HashMap::new();

        for subnet in subnets.subnets() {
            let Some(subnet_id) = subnet.subnet_id() else {
                continue;
            };

            let mut tags_by_key = HashMap::new();
            for tag in subnet.tags() {
                let Some(tag_key) = tag.key() else {
                    continue;
                };

                tags_by_key.insert(tag_key.to_string(), tag.value().unwrap_or("").to_string());
            }

            subnets_tags_by_id.insert(subnet_id.to_string(), tags_by_key);
        }

        Ok(subnets_tags_by_id)
    }
}
