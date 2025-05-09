use crate::environment::models::kubernetes::K8sObject;
use crate::errors::{CommandError, EngineError, Tag};
use crate::events::{EventDetails, InfrastructureStep, Stage};
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::eks::custom_vpc::patch_kube_proxy_for_aws_user_network;
use crate::infrastructure::action::eks::helm_charts::EksHelmsDeployment;
use crate::infrastructure::action::eks::karpenter::Karpenter;
use crate::infrastructure::action::eks::karpenter::node_groups_when_karpenter_is_enabled;
use crate::infrastructure::action::eks::nodegroup::{
    NodeGroupsDeletionType, delete_eks_nodegroups, node_group_is_running, should_update_desired_nodes,
};
use crate::infrastructure::action::eks::sdk::QoveryAwsSdkConfigEks;
use crate::infrastructure::action::eks::tera_context::eks_tera_context;
use crate::infrastructure::action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure::action::eks::{AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION, AwsEksQoveryTerraformOutput};
use crate::infrastructure::action::kubeconfig_helper::update_kubeconfig_file;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::io_models::models::KubernetesClusterAction;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use crate::utilities::envs_to_string;
use retry::delay::Fixed;
use retry::{Error, OperationResult};
use rusoto_eks::EksClient;
use std::path::PathBuf;

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
            // if it is the first install we must keep fargate profile for add-ons/user-mapper, until we have karpenter installed (during helm deployments)
            // After karpenter is installed, we can remove the fargate profile for add-ons/user-mapper.
            infra_ctx.kubernetes().is_karpenter_enabled() && infra_ctx.context().is_first_cluster_deployment(),
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

        let tf_apply_result = retry::retry(Fixed::from_millis(3000).take(1), || {
            let qovery_terraform_output: Result<AwsEksQoveryTerraformOutput, Box<EngineError>> =
                tf_action.create(&logger);

            match qovery_terraform_output {
                Ok(output) => OperationResult::Ok(output),
                Err(e) => {
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

    // apply to generate tf_qovery_config.json
    let (eks_tf_output, tera_context) = terraform_apply()?;
    update_kubeconfig_file(kubernetes, &eks_tf_output.kubeconfig)?;

    let kube_client = infra_ctx.mk_kube_client()?;

    let credentials_env_vars = envs_to_string(cloud_provider.credentials_environment_variables());
    if let Some(spot_enabled) = &kubernetes.get_karpenter_parameters().map(|x| x.spot_enabled) {
        if *spot_enabled {
            block_on(Karpenter::create_aws_service_role_for_ec2_spot(&aws_conn, &event_details))?;
        }

        if Karpenter::is_paused(&infra_ctx.mk_kube_client()?, &event_details)? {
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
    clean_karpenter_installation(kubernetes, infra_ctx, &logger, event_details.clone(), aws_eks_client)?;

    Ok(())
}

// after Karpenter is deployed, we can remove the node groups
// after Karpenter is deployed, we can remove fargate profile for add-ons
// TODO: remove the node groups part of the function once every cluster has Karpenter enabled.
fn clean_karpenter_installation(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    logger: &impl InfraLogger,
    event_details: EventDetails,
    aws_eks_client: Option<EksClient>,
) -> Result<(), Box<EngineError>> {
    if !kubernetes.is_karpenter_enabled() {
        return Ok(());
    }

    if kubernetes.context.is_dry_run_deploy() {
        logger.warn("👻 Dry run mode enabled. Skipping Karpenter cleanup");
        return Ok(());
    }

    let has_node_group_running = kubernetes.nodes_groups.iter().any(|ng| {
        matches!(
            node_group_is_running(kubernetes, &event_details, ng, aws_eks_client.clone()),
            Ok(Some(_v))
        )
    });

    if !has_node_group_running && !kubernetes.context().is_first_cluster_deployment() {
        return Ok(());
    }

    // generate terraform files and copy them into temp dir
    let tera_context = eks_tera_context(
        kubernetes,
        infra_ctx.cloud_provider(),
        infra_ctx.dns_provider(),
        kubernetes.zones.as_slice(),
        &[],
        &kubernetes.options,
        AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION,
        false,
        &kubernetes.advanced_settings,
        kubernetes.qovery_allowed_public_access_cidrs.as_ref(),
    )?;

    logger.info(format!("Deploying {} cluster.", kubernetes.kind()));
    let tf_action = TerraformInfraResources::new(
        tera_context.clone(),
        kubernetes.template_directory.join("terraform"),
        kubernetes.temp_dir().join("terraform_karpenter_cleanup"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        infra_ctx.context().is_dry_run_deploy(),
    );

    let _: AwsEksQoveryTerraformOutput = tf_action.create(logger)?;

    Ok(())
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
        false,
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
        .map(|eks_tf_output| update_kubeconfig_file(kubernetes, &eks_tf_output.kubeconfig));

    Ok(())
}
