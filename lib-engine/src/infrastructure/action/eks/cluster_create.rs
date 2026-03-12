use crate::environment::models::kubernetes::K8sObject;
use crate::errors::{CommandError, EngineError, Tag};
use crate::events::{EventDetails, InfrastructureStep, Stage};
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::cluster_outputs_helper::update_cluster_outputs;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::eks::custom_vpc::patch_kube_proxy_for_aws_user_network;
use crate::infrastructure::action::eks::helm_charts::EksHelmsDeployment;
use crate::infrastructure::action::eks::karpenter::Karpenter;
use crate::infrastructure::action::eks::karpenter::node_groups_when_karpenter_is_enabled;
use crate::infrastructure::action::eks::karpenter_migration::{
    deploy_karpenter_nodegroup, should_deploy_karpenter_nodegroup,
};
use crate::infrastructure::action::eks::nodegroup::{
    NodeGroupsDeletionType, delete_eks_nodegroups, should_update_desired_nodes,
};
use crate::infrastructure::action::eks::sdk::QoveryAwsSdkConfigEks;
use crate::infrastructure::action::eks::tera_context::eks_tera_context;
use crate::infrastructure::action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure::action::eks::{
    AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION, AWS_EKS_TERRAFORM_APPLY_HARD_TIMEOUT, AwsEksQoveryTerraformOutput,
};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::io_models::models::KubernetesClusterAction;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use crate::utilities::envs_to_string;
use retry::delay::Fixed;
use retry::{Error, OperationResult};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use url::Url;

pub fn create_eks_cluster(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    has_been_upgraded: bool,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
    let cloud_provider = infra_ctx.cloud_provider();
    let dns_provider = infra_ctx.dns_provider();

    logger.info(format!("Preparing {} cluster deployment.", kubernetes.kind()));

    // old method with rusoto
    let aws_eks_client = get_rusoto_eks_client(event_details.clone(), kubernetes, cloud_provider).ok();

    // aws connection
    let aws_conn = cloud_provider
        .downcast_ref()
        .as_aws()
        .ok_or_else(|| Box::new(EngineError::new_bad_cast(event_details.clone(), "cloud provider is not aws")))?
        .aws_sdk_client();

    let _ = restore_access_to_eks(kubernetes, infra_ctx, &event_details, &logger);

    let terraform_apply = || {
        // don't create node groups if karpenter is enabled
        let nodes_groups = node_groups_when_karpenter_is_enabled(
            kubernetes,
            infra_ctx,
            &kubernetes.nodes_groups,
            &event_details,
            KubernetesClusterAction::Update(None),
        )?;

        let node_groups_with_desired_states = should_update_desired_nodes(
            event_details.clone(),
            kubernetes,
            if infra_ctx.context().is_first_cluster_deployment() {
                KubernetesClusterAction::Bootstrap
            } else {
                KubernetesClusterAction::Update(None)
            },
            nodes_groups,
            aws_eks_client.clone(),
        )?;

        // in case error, this should no be a blocking error
        let cluster_upgrade_timeout_in_min = if let Ok(kube_client) = infra_ctx.mk_kube_client() {
            let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
                .unwrap_or(Vec::with_capacity(0));

            let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Upgrade(None));
            if let Some(x) = message {
                logger.info(x);
            }
            timeout
        } else {
            AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION
        };

        // generate terraform files and copy them into temp dir
        let tera_context = eks_tera_context(
            kubernetes,
            cloud_provider,
            dns_provider,
            kubernetes.zones.as_slice(),
            &node_groups_with_desired_states,
            &kubernetes.options,
            cluster_upgrade_timeout_in_min,
            &kubernetes.advanced_settings,
            kubernetes.qovery_allowed_public_access_cidrs.as_ref(),
        )?;

        logger.info(format!("Deploying {} cluster.", kubernetes.kind()));
        let tf_action = TerraformInfraResources::new(
            tera_context.clone(),
            kubernetes.template_directory.join("terraform"),
            kubernetes.temp_dir.join("terraform"),
            event_details.clone(),
            envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
            infra_ctx.context().is_dry_run_deploy(),
        );

        let mut retry_warning_sent = false;
        let tf_apply_result = retry::retry(Fixed::from_millis(3000).take(1), || {
            let qovery_terraform_output: Result<AwsEksQoveryTerraformOutput, Box<EngineError>> =
                tf_action.create_with_custom_tf_apply_options(&logger, 0, Some(AWS_EKS_TERRAFORM_APPLY_HARD_TIMEOUT));

            match qovery_terraform_output {
                Ok(output) => OperationResult::Ok(output),
                Err(e) => {
                    if !retry_warning_sent {
                        logger.warn("Terraform apply failed. Retrying once before failing infrastructure deployment.");
                        retry_warning_sent = true;
                    }
                    // on EKS, clean possible nodegroup deployment failures because of quota issues
                    // do not exit on this error to avoid masking the real Terraform issue
                    logger.info("Ensuring no failed nodegroups are present in the cluster, or delete them if at least one active nodegroup is present");
                    match block_on(delete_eks_nodegroups(
                        aws_conn.clone(),
                        kubernetes.cluster_name(),
                        NodeGroupsDeletionType::FailedOnly,
                        event_details.clone(),
                    )) {
                        Ok(_) => OperationResult::Retry(e),
                        Err(_) => OperationResult::Retry(e),
                    }
                }
            }
        });

        match tf_apply_result {
            Ok(output) => Ok((output, tera_context)),
            Err(Error { error, .. }) => Err(error),
        }
    };

    // on EKS, we need to check if there is no already deployed failed nodegroups to avoid future quota issues
    logger.info("Ensuring no failed nodegroups are present in the cluster, or delete them if at least one active nodegroup is present");
    if let Err(e) = block_on(delete_eks_nodegroups(
        aws_conn.clone(),
        kubernetes.cluster_name(),
        NodeGroupsDeletionType::FailedOnly,
        event_details.clone(),
    )) {
        // only return failures if the cluster is not absent, because it can be a VPC quota issue
        let nodgroups_len = block_on(aws_conn.list_all_eks_nodegroups(kubernetes.cluster_name()))
            .map(|n| n.nodegroups().len())
            .unwrap_or(1);
        if e.tag() != &Tag::CannotGetCluster && nodgroups_len != 1 {
            return Err(e);
        }
    }

    precheck_custom_vpc_alb_subnet_tags(
        kubernetes,
        &aws_conn,
        event_details.clone(),
        &logger,
        infra_ctx.context().is_first_cluster_deployment(),
    )?;

    // Deploy Karpenter nodegroup and install Karpenter charts if migration is enabled
    logger.info("Checking if Karpenter nodegroup should be deployed...");
    if should_deploy_karpenter_nodegroup(kubernetes, infra_ctx, &logger) {
        logger.info("🚀 Deploying Karpenter controller nodegroup");
        let _karpenter_tf_output = deploy_karpenter_nodegroup(kubernetes, infra_ctx, &logger)?;
    } else {
        logger.info("⏭️  Skipping Karpenter nodegroup deployment (not required)");
    }

    // apply to generate tf_qovery_config.json
    let (eks_tf_output, tera_context) = terraform_apply()?;
    update_cluster_outputs(kubernetes, &eks_tf_output)?;

    let kube_client = infra_ctx.mk_kube_client()?;

    let credentials_env_vars = envs_to_string(cloud_provider.credentials_environment_variables());
    if let Some(spot_enabled) = &kubernetes.get_karpenter_parameters().map(|x| x.spot_enabled) {
        if *spot_enabled {
            block_on(Karpenter::create_aws_service_role_for_ec2_spot(&aws_conn, &event_details))?;
        }

        if Karpenter::is_paused(&infra_ctx.mk_kube_client()?, &event_details)? {
            logger.info("Karpenter is paused, restarting...");
            block_on(Karpenter::restart(
                kubernetes,
                cloud_provider,
                &eks_tf_output,
                &kube_client,
                kubernetes.long_id,
                &kubernetes.options,
            ))?;
        }
    }

    patch_kube_proxy_for_custom_vpc(kubernetes, infra_ctx, event_details.clone(), &logger)?;
    let alb_already_deployed = is_nginx_migrated_to_alb(kubernetes, infra_ctx, event_details.clone())?;
    let helms_deployments = EksHelmsDeployment::new(
        HelmInfraContext::new(
            tera_context,
            PathBuf::from(infra_ctx.context().lib_root_dir()),
            kubernetes.template_directory.clone(),
            kubernetes.temp_dir().join("helms"),
            event_details.clone(),
            credentials_env_vars,
            kubernetes.context().is_dry_run_deploy(),
        ),
        eks_tf_output,
        kubernetes,
        alb_already_deployed,
        has_been_upgraded,
    );

    helms_deployments.deploy_charts(infra_ctx, &logger)?;

    Ok(())
}

fn should_precheck_custom_vpc_alb_subnet_tags(network_managed_by_user: bool, alb_controller_enabled: bool) -> bool {
    network_managed_by_user && alb_controller_enabled
}

fn precheck_custom_vpc_alb_subnet_tags(
    kubernetes: &EKS,
    aws_conn: &aws_types::SdkConfig,
    event_details: EventDetails,
    logger: &impl InfraLogger,
    requested_strict_mode: bool,
) -> Result<(), Box<EngineError>> {
    if !should_precheck_custom_vpc_alb_subnet_tags(
        kubernetes.is_network_managed_by_user(),
        kubernetes.advanced_settings().aws_eks_enable_alb_controller,
    ) {
        return Ok(());
    }

    let Some(user_network_config) = kubernetes.options.user_provided_network.as_ref() else {
        return Ok(());
    };

    let public_subnet_ids = collect_unique_subnet_ids([
        user_network_config.eks_subnets_zone_a_ids.as_slice(),
        user_network_config.eks_subnets_zone_b_ids.as_slice(),
        user_network_config.eks_subnets_zone_c_ids.as_slice(),
    ]);
    let private_subnet_ids = collect_unique_subnet_ids([
        user_network_config.eks_private_subnets_zone_a_ids.as_slice(),
        user_network_config.eks_private_subnets_zone_b_ids.as_slice(),
        user_network_config.eks_private_subnets_zone_c_ids.as_slice(),
    ]);
    let all_subnet_ids = collect_unique_subnet_ids([public_subnet_ids.as_slice(), private_subnet_ids.as_slice()]);

    if all_subnet_ids.is_empty() {
        return Ok(());
    }

    let subnet_tags_by_id = match block_on(aws_conn.describe_subnets_tags_by_ids(all_subnet_ids.clone())) {
        Ok(tags) => tags,
        Err(error) => {
            if is_update_relaxed_mode(requested_strict_mode) {
                logger.warn(format!(
                    "Cannot validate custom VPC subnet tags required by AWS ALB controller during cluster update. Continuing because this is not a cluster creation. Error: {error}"
                ));
                return Ok(());
            }

            return Err(Box::new(EngineError::new_error_do_not_respect_cloud_provider_best_practices(
                event_details.clone(),
                CommandError::new_from_safe_message(format!(
                    "Cannot validate custom VPC subnet tags required by AWS ALB controller: {error}"
                )),
                Url::parse("https://docs.aws.amazon.com/eks/latest/userguide/alb-ingress.html").ok(),
            )));
        }
    };

    let cluster_tag_key = format!("kubernetes.io/cluster/{}", kubernetes.cluster_name());
    let validation_errors = validate_alb_controller_subnet_tags(
        public_subnet_ids.as_slice(),
        private_subnet_ids.as_slice(),
        all_subnet_ids.as_slice(),
        &subnet_tags_by_id,
        cluster_tag_key.as_str(),
    );

    if validation_errors.is_empty() {
        return Ok(());
    }

    if is_update_relaxed_mode(requested_strict_mode) {
        // Transitional policy:
        // for cluster updates, do not fail yet on subnet-tag non-compliance.
        // We first warn and contact impacted customers before making this mandatory.
        logger.warn(format!(
            "ALB custom VPC subnet tags are non-compliant. Continuing because this is not a cluster creation. Please fix these tags (will become mandatory later): {}",
            validation_errors.join(" ; ")
        ));
        return Ok(());
    }

    let remediation_message = format!(
        "ALB controller requires subnet tags on custom VPC before Terraform during cluster creation. Fix the following and retry: {}",
        validation_errors.join(" ; ")
    );
    Err(Box::new(EngineError::new_error_do_not_respect_cloud_provider_best_practices(
        event_details,
        CommandError::new_from_safe_message(remediation_message),
        Url::parse("https://docs.aws.amazon.com/eks/latest/userguide/alb-ingress.html").ok(),
    )))
}

fn is_update_relaxed_mode(requested_strict_mode: bool) -> bool {
    !requested_strict_mode
}

fn collect_unique_subnet_ids<'a, I>(subnet_groups: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a [String]>,
{
    let mut unique_subnet_ids = BTreeSet::new();
    for subnet_group in subnet_groups {
        for subnet_id in subnet_group {
            unique_subnet_ids.insert(subnet_id.clone());
        }
    }

    unique_subnet_ids.into_iter().collect()
}

fn validate_alb_controller_subnet_tags(
    public_subnet_ids: &[String],
    private_subnet_ids: &[String],
    all_subnet_ids: &[String],
    subnet_tags_by_id: &HashMap<String, HashMap<String, String>>,
    cluster_tag_key: &str,
) -> Vec<String> {
    let mut validation_errors = BTreeSet::new();
    let expected_cluster_tag_value = "shared";
    let public_subnet_ids_set: BTreeSet<&str> = public_subnet_ids.iter().map(String::as_str).collect();
    let private_subnet_ids_set: BTreeSet<&str> = private_subnet_ids.iter().map(String::as_str).collect();

    // Validate public/internal role tags. If a subnet is present in both lists, accept either role tag.
    for subnet_id in all_subnet_ids {
        let Some(subnet_tags) = subnet_tags_by_id.get(subnet_id) else {
            validation_errors.insert(format!("Subnet `{subnet_id}` was not returned by AWS DescribeSubnets API."));
            continue;
        };

        let is_public = public_subnet_ids_set.contains(subnet_id.as_str());
        let is_private = private_subnet_ids_set.contains(subnet_id.as_str());

        match (is_public, is_private) {
            (true, true) => validate_subnet_any_role_tag(subnet_id, subnet_tags, &mut validation_errors),
            (true, false) => {
                validate_subnet_tag(subnet_id, subnet_tags, "kubernetes.io/role/elb", "1", &mut validation_errors)
            }
            (false, true) => validate_subnet_tag(
                subnet_id,
                subnet_tags,
                "kubernetes.io/role/internal-elb",
                "1",
                &mut validation_errors,
            ),
            (false, false) => {}
        }
    }

    // Validate cluster tag once per subnet to avoid duplicated errors.
    for subnet_id in all_subnet_ids {
        let Some(subnet_tags) = subnet_tags_by_id.get(subnet_id) else {
            continue;
        };
        validate_subnet_tag(
            subnet_id,
            subnet_tags,
            cluster_tag_key,
            expected_cluster_tag_value,
            &mut validation_errors,
        );
    }

    validation_errors.into_iter().collect()
}

fn validate_subnet_tag(
    subnet_id: &str,
    subnet_tags: &HashMap<String, String>,
    tag_key: &str,
    expected_tag_value: &str,
    validation_errors: &mut BTreeSet<String>,
) {
    match subnet_tags.get(tag_key) {
        None => {
            validation_errors.insert(format!(
                "Subnet `{subnet_id}` is missing tag `{tag_key}` with value `{expected_tag_value}`."
            ));
        }
        Some(actual_value) if actual_value != expected_tag_value => {
            validation_errors.insert(format!(
                "Subnet `{subnet_id}` has tag `{tag_key}={actual_value}` but expected `{expected_tag_value}`."
            ));
        }
        Some(_) => {}
    }
}

fn validate_subnet_any_role_tag(
    subnet_id: &str,
    subnet_tags: &HashMap<String, String>,
    validation_errors: &mut BTreeSet<String>,
) {
    let has_public_role_tag = subnet_tags
        .get("kubernetes.io/role/elb")
        .is_some_and(|value| value == "1");
    let has_private_role_tag = subnet_tags
        .get("kubernetes.io/role/internal-elb")
        .is_some_and(|value| value == "1");

    if !has_public_role_tag && !has_private_role_tag {
        validation_errors.insert(format!(
            "Subnet `{subnet_id}` is in both public/private lists and must have at least one role tag with value `1`: `kubernetes.io/role/elb` or `kubernetes.io/role/internal-elb`."
        ));
    }
}

fn is_nginx_migrated_to_alb(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    event_details: EventDetails,
) -> Result<bool, Box<EngineError>> {
    // before deploying Helm charts, we need to check if Nginx ingress controller needs to move NLB to ALB controller
    let qube_client = infra_ctx.mk_kube_client()?;
    let nginx_namespace = "nginx-ingress";
    let services = block_on(qube_client.get_services(
        event_details.clone(),
        Some(nginx_namespace),
        SelectK8sResourceBy::LabelsSelector("app.kubernetes.io/name=ingress-nginx".to_string()),
    ))?;
    // annotations corresponding to service to delete if found (to be later replaced)
    let service_nlb_annotation_to_delete = match kubernetes.advanced_settings().aws_eks_enable_alb_controller {
        true => "nlb".to_string(),       // without ALB controller
        false => "external".to_string(), // with ALB controller
    };
    // search for nlb annotation
    for service in &services {
        if service.get_annotation_value("service.beta.kubernetes.io/aws-load-balancer-type")
            == Some(&service_nlb_annotation_to_delete)
        {
            block_on(qube_client.delete_service_from_name(
                event_details.clone(),
                nginx_namespace,
                service.metadata.name.as_str(),
            ))?;
            break;
        }
    }

    // check if alb controller is already enabled to decide if webhooks should be enabled or not
    let found_alb_mutating_configs = block_on(
        qube_client
            .get_mutating_webhook_configurations(event_details.clone(), SelectK8sResourceBy::Name("xxx".to_string())),
    )?;

    Ok(!found_alb_mutating_configs.is_empty())
}

fn patch_kube_proxy_for_custom_vpc(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    event_details: EventDetails,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    if !kubernetes.is_network_managed_by_user() || kubernetes.advanced_settings().aws_eks_enable_alb_controller {
        return Ok(());
    }

    if kubernetes.context.is_dry_run_deploy() {
        logger.warn("👻 Dry run mode enabled. Skipping kube proxy patching for user configured network");
        return Ok(());
    }

    // When the user control the network/vpc configuration, we may hit a bug of the in tree aws load balancer controller
    // were if there is a custom dns server set for the VPC, kube-proxy nodes are not correctly configured and load balancer healthcheck are failing
    // The correct fix would be to stop using the k8s in tree lb controller, and use instead the external aws lb controller.
    // But as we don't want to do the migration for all users, we will just patch the kube-proxy configuration on the fly
    // https://aws.amazon.com/premiumsupport/knowledge-center/eks-troubleshoot-unhealthy-targets-nlb/
    // https://github.com/kubernetes/kubernetes/issues/80579
    // https://github.com/kubernetes/cloud-provider-aws/issues/87
    info!("patching kube-proxy configuration to fix k8s in tree load balancer controller bug");
    block_on(patch_kube_proxy_for_aws_user_network(infra_ctx.mk_kube_client()?.client())).map_err(|e| {
        EngineError::new_k8s_node_not_ready(
            event_details.clone(),
            CommandError::new_from_safe_message(format!("Cannot patch kube proxy for user configured network: {e}")),
        )
    })?;

    Ok(())
}

fn restore_access_to_eks(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    event_details: &EventDetails,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    if kubernetes.context.is_first_cluster_deployment() {
        return Ok(());
    }

    // We should be able to connect, if not try to restore access
    match infra_ctx.mk_kube_client() {
        Err(e) if e.tag() == &Tag::CannotConnectK8sCluster => (),
        _ => return Ok(()),
    };

    logger.info("⚗️ Restoring access to the EKS cluster");
    let tera_context = eks_tera_context(
        kubernetes,
        infra_ctx.cloud_provider(),
        infra_ctx.dns_provider(),
        kubernetes.zones.as_slice(),
        &[],
        &kubernetes.options,
        AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION,
        &kubernetes.advanced_settings,
        kubernetes.qovery_allowed_public_access_cidrs.as_ref(),
    )?;

    let tf_action = TerraformInfraResources::new(
        tera_context,
        kubernetes.template_directory.join("terraform"),
        kubernetes.temp_dir.join("terraform_eks_restore_access"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        infra_ctx.context().is_dry_run_deploy(),
    );

    let _ = tf_action
        .apply_specific_resources(
            &[
                "aws_eks_access_entry.qovery_eks_access",
                "aws_eks_access_policy_association.qovery_eks_access",
            ],
            logger,
        )
        .map_err(|err| logger.warn(*err));

    if infra_ctx.context().is_dry_run_deploy() {
        return Ok(());
    }

    // This should never happen in real life, but just in case we re-create the cluster outside Qovery
    // and that the kubeconfig changed in the meantime
    let _ = tf_action
        .output::<AwsEksQoveryTerraformOutput>()
        .map(|eks_tf_output| update_cluster_outputs(kubernetes, &eks_tf_output));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        collect_unique_subnet_ids, is_update_relaxed_mode, should_precheck_custom_vpc_alb_subnet_tags,
        validate_alb_controller_subnet_tags,
    };
    use std::collections::HashMap;

    #[test]
    fn should_precheck_only_when_network_is_user_managed_and_alb_enabled() {
        assert!(should_precheck_custom_vpc_alb_subnet_tags(true, true));
        assert!(!should_precheck_custom_vpc_alb_subnet_tags(true, false));
        assert!(!should_precheck_custom_vpc_alb_subnet_tags(false, true));
        assert!(!should_precheck_custom_vpc_alb_subnet_tags(false, false));
    }

    #[test]
    fn should_use_relaxed_mode_only_when_not_strict() {
        assert!(is_update_relaxed_mode(false));
        assert!(!is_update_relaxed_mode(true));
    }

    #[test]
    fn collect_unique_subnet_ids_should_deduplicate_and_sort() {
        let subnets = collect_unique_subnet_ids([
            &["subnet-b".to_string(), "subnet-a".to_string()][..],
            &["subnet-a".to_string()][..],
        ]);
        assert_eq!(subnets, vec!["subnet-a".to_string(), "subnet-b".to_string()]);
    }

    #[test]
    fn validate_alb_controller_subnet_tags_should_succeed_when_all_tags_are_correct() {
        let public_subnet_ids = vec!["subnet-public-a".to_string()];
        let private_subnet_ids = vec!["subnet-private-a".to_string()];
        let all_subnet_ids = collect_unique_subnet_ids([public_subnet_ids.as_slice(), private_subnet_ids.as_slice()]);
        let cluster_tag_key = "kubernetes.io/cluster/qovery-abcd1234";

        let mut subnet_tags_by_id: HashMap<String, HashMap<String, String>> = HashMap::new();
        subnet_tags_by_id.insert(
            "subnet-public-a".to_string(),
            HashMap::from([
                ("kubernetes.io/role/elb".to_string(), "1".to_string()),
                (cluster_tag_key.to_string(), "shared".to_string()),
            ]),
        );
        subnet_tags_by_id.insert(
            "subnet-private-a".to_string(),
            HashMap::from([
                ("kubernetes.io/role/internal-elb".to_string(), "1".to_string()),
                (cluster_tag_key.to_string(), "shared".to_string()),
            ]),
        );

        let errors = validate_alb_controller_subnet_tags(
            public_subnet_ids.as_slice(),
            private_subnet_ids.as_slice(),
            all_subnet_ids.as_slice(),
            &subnet_tags_by_id,
            cluster_tag_key,
        );

        assert!(errors.is_empty());
    }

    #[test]
    fn validate_alb_controller_subnet_tags_should_fail_when_public_subnet_tag_is_missing() {
        let public_subnet_ids = vec!["subnet-public-a".to_string()];
        let private_subnet_ids = vec![];
        let all_subnet_ids = collect_unique_subnet_ids([public_subnet_ids.as_slice(), private_subnet_ids.as_slice()]);
        let cluster_tag_key = "kubernetes.io/cluster/qovery-abcd1234";
        let subnet_tags_by_id = HashMap::from([(
            "subnet-public-a".to_string(),
            HashMap::from([(cluster_tag_key.to_string(), "shared".to_string())]),
        )]);

        let errors = validate_alb_controller_subnet_tags(
            public_subnet_ids.as_slice(),
            private_subnet_ids.as_slice(),
            all_subnet_ids.as_slice(),
            &subnet_tags_by_id,
            cluster_tag_key,
        );

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("kubernetes.io/role/elb"));
    }

    #[test]
    fn validate_alb_controller_subnet_tags_should_fail_when_private_subnet_tag_value_is_invalid() {
        let public_subnet_ids = vec![];
        let private_subnet_ids = vec!["subnet-private-a".to_string()];
        let all_subnet_ids = collect_unique_subnet_ids([public_subnet_ids.as_slice(), private_subnet_ids.as_slice()]);
        let cluster_tag_key = "kubernetes.io/cluster/qovery-abcd1234";
        let subnet_tags_by_id = HashMap::from([(
            "subnet-private-a".to_string(),
            HashMap::from([
                ("kubernetes.io/role/internal-elb".to_string(), "true".to_string()),
                (cluster_tag_key.to_string(), "shared".to_string()),
            ]),
        )]);

        let errors = validate_alb_controller_subnet_tags(
            public_subnet_ids.as_slice(),
            private_subnet_ids.as_slice(),
            all_subnet_ids.as_slice(),
            &subnet_tags_by_id,
            cluster_tag_key,
        );

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("kubernetes.io/role/internal-elb=true"));
    }

    #[test]
    fn validate_alb_controller_subnet_tags_should_fail_when_cluster_tag_value_is_invalid() {
        let public_subnet_ids = vec!["subnet-public-a".to_string()];
        let private_subnet_ids = vec![];
        let all_subnet_ids = collect_unique_subnet_ids([public_subnet_ids.as_slice(), private_subnet_ids.as_slice()]);
        let cluster_tag_key = "kubernetes.io/cluster/qovery-abcd1234";
        let subnet_tags_by_id = HashMap::from([(
            "subnet-public-a".to_string(),
            HashMap::from([
                ("kubernetes.io/role/elb".to_string(), "1".to_string()),
                (cluster_tag_key.to_string(), "owned".to_string()),
            ]),
        )]);

        let errors = validate_alb_controller_subnet_tags(
            public_subnet_ids.as_slice(),
            private_subnet_ids.as_slice(),
            all_subnet_ids.as_slice(),
            &subnet_tags_by_id,
            cluster_tag_key,
        );

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected `shared`"));
    }

    #[test]
    fn validate_alb_controller_subnet_tags_should_fail_when_subnet_not_returned_by_aws() {
        let public_subnet_ids = vec!["subnet-public-a".to_string()];
        let private_subnet_ids = vec![];
        let all_subnet_ids = collect_unique_subnet_ids([public_subnet_ids.as_slice(), private_subnet_ids.as_slice()]);
        let cluster_tag_key = "kubernetes.io/cluster/qovery-abcd1234";
        let subnet_tags_by_id = HashMap::new();

        let errors = validate_alb_controller_subnet_tags(
            public_subnet_ids.as_slice(),
            private_subnet_ids.as_slice(),
            all_subnet_ids.as_slice(),
            &subnet_tags_by_id,
            cluster_tag_key,
        );

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("was not returned by AWS DescribeSubnets API"));
    }

    #[test]
    fn validate_alb_controller_subnet_tags_should_allow_overlap_when_one_role_tag_is_present() {
        let public_subnet_ids = vec!["subnet-a".to_string()];
        let private_subnet_ids = vec!["subnet-a".to_string()];
        let all_subnet_ids = collect_unique_subnet_ids([public_subnet_ids.as_slice(), private_subnet_ids.as_slice()]);
        let cluster_tag_key = "kubernetes.io/cluster/qovery-abcd1234";
        let subnet_tags_by_id = HashMap::from([(
            "subnet-a".to_string(),
            HashMap::from([
                ("kubernetes.io/role/elb".to_string(), "1".to_string()),
                (cluster_tag_key.to_string(), "shared".to_string()),
            ]),
        )]);

        let errors = validate_alb_controller_subnet_tags(
            public_subnet_ids.as_slice(),
            private_subnet_ids.as_slice(),
            all_subnet_ids.as_slice(),
            &subnet_tags_by_id,
            cluster_tag_key,
        );

        assert!(errors.is_empty());
    }

    #[test]
    fn validate_alb_controller_subnet_tags_should_deduplicate_cluster_tag_errors_for_overlap() {
        let public_subnet_ids = vec!["subnet-a".to_string()];
        let private_subnet_ids = vec!["subnet-a".to_string()];
        let all_subnet_ids = collect_unique_subnet_ids([public_subnet_ids.as_slice(), private_subnet_ids.as_slice()]);
        let cluster_tag_key = "kubernetes.io/cluster/qovery-abcd1234";
        let subnet_tags_by_id = HashMap::from([(
            "subnet-a".to_string(),
            HashMap::from([("kubernetes.io/role/elb".to_string(), "1".to_string())]),
        )]);

        let errors = validate_alb_controller_subnet_tags(
            public_subnet_ids.as_slice(),
            private_subnet_ids.as_slice(),
            all_subnet_ids.as_slice(),
            &subnet_tags_by_id,
            cluster_tag_key,
        );

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains(cluster_tag_key));
    }
}
