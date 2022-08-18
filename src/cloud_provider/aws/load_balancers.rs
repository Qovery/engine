use crate::cloud_provider::DeploymentTarget;
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::models::router::Router;
use crate::models::types::{ToTeraContext, AWS};
use crate::runtime::block_on;
use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_elasticloadbalancingv2::error::DescribeTagsError;
use aws_sdk_elasticloadbalancingv2::model::{LoadBalancer, TagDescription};
use aws_smithy_client::SdkError;
use tera::Context as TeraContext;

// temporary fix for NLB not properly removed https://discuss.qovery.com/t/why-provision-nlbs-for-container-databases/1114/10?u=pierre_mavro
pub fn _clean_up_deleted_k8s_nlb(event_details: EventDetails, target: &DeploymentTarget) -> Result<(), EngineError> {
    let conn = match target.cloud_provider.aws_sdk_client() {
        Some(x) => x,
        None => return Ok(()),
    };
    let load_balancers = block_on(conn.list_all_aws_load_balancers()).map_err(|e| {
        EngineError::new_cloud_provider_error_getting_load_balancers(
            event_details.clone(),
            CommandError::new_from_safe_message(e.to_string()),
        )
    })?;
    // get tags from the load balancers
    let load_balancers_tags = block_on(
        conn.get_aws_load_balancers_tags(load_balancers.load_balancers().unwrap_or(&[]).to_vec()),
    )
    .map_err(|e| {
        EngineError::new_cloud_provider_error_getting_load_balancer_tags(
            event_details,
            CommandError::new_from_safe_message(e.to_string()),
        )
    })?;
    // get only ones matching the current cluster
    let _load_balancers_matching_current_cluster = _filter_load_balancers_by_tag(
        Some(target.kubernetes.cluster_name().as_str()),
        Some("owned"),
        load_balancers_tags,
        true,
    );
    Ok(())
}

#[async_trait]
impl QoveryAwsSdkConfig for SdkConfig {
    /// Get/list all load balancers in the account.
    async fn list_all_aws_load_balancers(
        &self,
    ) -> Result<
        aws_sdk_elasticloadbalancingv2::output::DescribeLoadBalancersOutput,
        aws_smithy_client::SdkError<aws_sdk_elasticloadbalancingv2::error::DescribeLoadBalancersError>,
    > {
        let client = aws_sdk_elasticloadbalancingv2::Client::new(self);
        // AWS API maximum per page size is 400
        client.describe_load_balancers().page_size(400).send().await
    }

    /// Exracts the tags from the AWS API response
    async fn get_aws_load_balancers_tags(
        &self,
        load_balancers: Vec<LoadBalancer>,
    ) -> Result<Vec<TagDescription>, SdkError<DescribeTagsError>> {
        let total_lbs_output = load_balancers.len();
        if total_lbs_output == 0 {
            return Ok(Vec::with_capacity(0));
        }

        let client = aws_sdk_elasticloadbalancingv2::Client::new(self);
        let mut counter: usize = 0;
        let max_describe_tags_allowed_by_aws_api_call = 20;
        let mut lb_arns = Vec::with_capacity(max_describe_tags_allowed_by_aws_api_call);
        let mut load_balancers_tags = Vec::new();

        // store all lbs tags
        for load_balancer in load_balancers {
            counter = counter + 1;

            match load_balancer.load_balancer_arn() {
                Some(x) => lb_arns.push(x.to_string()),
                None => continue, // we must find an ARN, otherwise something went certainly wrong with the AWS API
            };

            // wait to have max_describe_tags_allowed_by_aws_by_call or the end of the list to make an AWS API call
            if lb_arns.len() == max_describe_tags_allowed_by_aws_api_call || counter == total_lbs_output {
                let current_lb_tags = client
                    .describe_tags()
                    .set_resource_arns(Some(lb_arns.clone()))
                    .send()
                    .await?;
                if let Some(x) = current_lb_tags.tag_descriptions {
                    load_balancers_tags.extend(x);
                };
                lb_arns.clear();
            }
        }

        Ok(load_balancers_tags)
    }
}

/// filter AWS load balancers based on by tags
/// load_balancers_tags: Tags returned from AWS API, also containing the ARN
pub fn _filter_load_balancers_by_tag(
    tag_key: Option<&str>,
    tag_value: Option<&str>,
    load_balancers_with_tags: Vec<TagDescription>,
    exact_match: bool,
) -> Result<Vec<TagDescription>, String> {
    let mut matched_load_balancers: Vec<TagDescription> = Vec::new();
    let check_match = |tag: &str, compare_to: &str| match exact_match {
        true => tag == compare_to,
        false => tag.to_string().contains(compare_to),
    };

    for load_balancer in load_balancers_with_tags {
        if let Some(tags_list) = load_balancer.tags() {
            for tag in tags_list {
                // check if key matches
                let key_match = match tag_key {
                    Some(x) => check_match(tag.key().unwrap_or(""), x),
                    None => false,
                };

                // check if value matches
                let value_match = match tag_value {
                    Some(x) => check_match(tag.value().unwrap_or(""), x),
                    None => false,
                };

                if tag_key.is_some() && tag_value.is_some() {
                    if key_match && value_match {
                        matched_load_balancers.push(load_balancer.clone());
                    }
                } else if tag_key.is_some() || tag_value.is_some() {
                    if key_match || value_match {
                        matched_load_balancers.push(load_balancer.clone());
                    }
                }
            }
        }
    }

    Ok(matched_load_balancers)
}

#[cfg(test)]
mod tests {
    use super::filter_load_balancers_by_tag;
    use aws_sdk_elasticloadbalancingv2::model::{Tag, TagDescription};
    use uuid::Uuid;

    #[test]
    fn test_load_balancers_filter() {
        let arn_id = Uuid::new_v4().to_string();
        let mut load_balancers_tags = TagDescription::builder()
            .set_resource_arn(Some(arn_id.clone()))
            .set_tags(None)
            .build();

        // match exact tag key
        load_balancers_tags.tags = Some(vec![Tag::builder()
            .set_key(Some("my_exact_key".to_string()))
            .set_value(None)
            .build()]);
        assert_eq!(
            filter_load_balancers_by_tag(Some("my_exact_key"), None, vec![load_balancers_tags.clone()], true).unwrap()
                [0]
            .resource_arn()
            .unwrap(),
            arn_id
        );
        // match tag key
        assert_eq!(
            filter_load_balancers_by_tag(Some("key"), None, vec![load_balancers_tags.clone()], false).unwrap()[0]
                .resource_arn()
                .unwrap(),
            arn_id
        );
        // do not match tag key
        assert_eq!(
            filter_load_balancers_by_tag(Some("do_not_match_key"), None, vec![load_balancers_tags.clone()], false)
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            filter_load_balancers_by_tag(Some("do_not_match_key"), None, vec![load_balancers_tags.clone()], true)
                .unwrap()
                .len(),
            0
        );

        // match tag values, same as keys tests above
        load_balancers_tags.tags = Some(vec![Tag::builder()
            .set_key(None)
            .set_value(Some("my_exact_key".to_string()))
            .build()]);
        assert_eq!(
            filter_load_balancers_by_tag(None, Some("my_exact_key"), vec![load_balancers_tags.clone()], true).unwrap()
                [0]
            .resource_arn()
            .unwrap(),
            arn_id
        );
        // match tag value
        assert_eq!(
            filter_load_balancers_by_tag(None, Some("key"), vec![load_balancers_tags.clone()], false).unwrap()[0]
                .resource_arn()
                .unwrap(),
            arn_id
        );
        // do not match tag value
        assert_eq!(
            filter_load_balancers_by_tag(None, Some("do_not_match_key"), vec![load_balancers_tags.clone()], false)
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            filter_load_balancers_by_tag(None, Some("do_not_match_key"), vec![load_balancers_tags.clone()], true)
                .unwrap()
                .len(),
            0
        );

        // math key and value
        load_balancers_tags.tags = Some(vec![Tag::builder()
            .set_key(Some("my_exact_key".to_string()))
            .set_value(Some("my_exact_value".to_string()))
            .build()]);
        assert_eq!(
            filter_load_balancers_by_tag(
                Some("my_exact_key"),
                Some("my_exact_value"),
                vec![load_balancers_tags.clone()],
                true
            )
            .unwrap()[0]
                .resource_arn()
                .unwrap(),
            arn_id
        );
        // do match key only
        assert_eq!(
            filter_load_balancers_by_tag(
                Some("my_exact_key"),
                Some("do_not_match_value"),
                vec![load_balancers_tags.clone()],
                false
            )
            .unwrap()
            .len(),
            0
        );
        // do match value only
        assert_eq!(
            filter_load_balancers_by_tag(
                Some("do_not_match_key"),
                Some("my_exact_value"),
                vec![load_balancers_tags.clone()],
                true
            )
            .unwrap()
            .len(),
            0
        );
    }
}
