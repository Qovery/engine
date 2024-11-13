use crate::cloud_provider::aws::kubernetes::eks::EKS;
use crate::cloud_provider::aws::regions::AwsRegion;
use crate::cloud_provider::helm::deploy_charts_levels;
use crate::cloud_provider::kubeconfig_helper::update_kubeconfig_file;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes};
use crate::cloud_provider::models::KubernetesClusterAction;
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsAws};
use crate::cmd::kubectl_utils::kubectl_are_qovery_infra_pods_executed;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError, Tag};
use crate::events::{EventDetails, EventMessage, InfrastructureStep, Stage};
use crate::infrastructure_action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure_action::eks::custom_vpc::patch_kube_proxy_for_aws_user_network;
use crate::infrastructure_action::eks::helm_charts::{eks_helm_charts, EksChartsConfigPrerequisites};
use crate::infrastructure_action::eks::karpenter::node_groups_when_karpenter_is_enabled;
use crate::infrastructure_action::eks::karpenter::Karpenter;
use crate::infrastructure_action::eks::nodegroup::{
    delete_eks_nodegroups, node_group_is_running, should_update_desired_nodes, NodeGroupsDeletionType,
};
use crate::infrastructure_action::eks::sdk::QoveryAwsSdkConfigEks;
use crate::infrastructure_action::eks::tera_context::eks_tera_context;
use crate::infrastructure_action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure_action::eks::{AwsEksQoveryTerraformOutput, AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION};
use crate::infrastructure_action::{InfraLogger, InfrastructureAction};
use crate::io_models::context::Features;
use crate::models::domain::ToHelmString;
use crate::models::kubernetes::K8sObject;
use crate::models::third_parties::LetsEncryptConfig;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use crate::string::terraform_list_format;
use itertools::Itertools;
use retry::delay::Fixed;
use retry::{Error, OperationResult};
use rusoto_eks::EksClient;
use std::str::FromStr;

pub fn create_eks_cluster(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
    let cloud_provider = infra_ctx.cloud_provider();
    let dns_provider = infra_ctx.dns_provider();

    logger.info(format!("Preparing {} cluster deployment.", kubernetes.kind()));
    let cluster_secrets = ClusterSecrets::new_aws_eks(ClusterSecretsAws::new(
        cloud_provider.access_key_id(),
        kubernetes.region().to_string(),
        cloud_provider.secret_access_key(),
        None,
        None,
        kubernetes.kind(),
        kubernetes.cluster_name(),
        kubernetes.long_id().to_string(),
        kubernetes.options.grafana_admin_user.clone(),
        kubernetes.options.grafana_admin_password.clone(),
        cloud_provider.organization_long_id().to_string(),
        kubernetes.context().is_test_cluster(),
    ));
    let temp_dir = kubernetes.temp_dir();

    // old method with rusoto
    let aws_eks_client = get_rusoto_eks_client(event_details.clone(), kubernetes, cloud_provider).ok();

    // aws connection
    let aws_conn = cloud_provider
        .aws_sdk_client()
        .ok_or_else(|| Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details.clone())))?;
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
            temp_dir.join("terraform"),
            event_details.clone(),
            infra_ctx.context().is_dry_run_deploy(),
        );

        let tf_apply_result = retry::retry(Fixed::from_millis(3000).take(1), || {
            let qovery_terraform_output: Result<AwsEksQoveryTerraformOutput, Box<EngineError>> = tf_action.create(
                infra_ctx
                    .cloud_provider()
                    .credentials_environment_variables()
                    .as_slice(),
                &logger,
            );

            match qovery_terraform_output {
                Ok(output) => OperationResult::Ok(output),
                Err(e) => {
                    // on EKS, clean possible nodegroup deployment failures because of quota issues
                    // do not exit on this error to avoid masking the real Terraform issue
                    logger.info("Ensuring no failed nodegroups are present in the cluster, or delete them if at least one active nodegroup is present");
                    match block_on(delete_eks_nodegroups(
                        aws_conn.clone(),
                        kubernetes.cluster_name(),
                        kubernetes.context().is_first_cluster_deployment(),
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
        kubernetes.context().is_first_cluster_deployment(),
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

    let mut kubernetes_version_upgrade_requested = false;
    if let Some(version_target) = kubernetes.is_upgrade_required(infra_ctx) {
        kubernetes_version_upgrade_requested = true;
        kubernetes.upgrade_cluster(infra_ctx, version_target)?;
    }

    // apply to generate tf_qovery_config.json
    let (eks_tf_output, tera_context) = terraform_apply()?;
    if infra_ctx.context().is_dry_run_deploy() {
        logger.warn("Exiting. Dry run is not supported after the terraform action for now");
        return Ok(());
    }
    update_kubeconfig_file(kubernetes, &eks_tf_output.kubeconfig)?;
    let kubeconfig_path = kubernetes.kubeconfig_local_file_path();

    // send cluster info with kubeconfig
    // create vault connection (Vault connectivity should not be on the critical deployment path,
    // if it temporarily fails, just ignore it, data will be pushed on the next sync)
    let _ = kubernetes.update_vault_config(event_details.clone(), cluster_secrets, Some(&kubeconfig_path));

    logger.info("Preparing chart configuration to be deployed");
    // kubernetes helm deployments on the cluster

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(&kubernetes.template_directory, temp_dir, &tera_context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details.clone(),
            kubernetes.template_directory.to_string_lossy().to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    let dirs_to_be_copied_to = vec![
        // copy lib/common/bootstrap/charts directory (and subdirectory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        (
            format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir()),
            format!("{}/common/charts", temp_dir.to_string_lossy()),
        ),
        // copy lib/common/bootstrap/chart_values directory (and subdirectory) into the lib/aws/bootstrap/common/chart_values directory.
        (
            format!("{}/common/bootstrap/chart_values", kubernetes.context().lib_root_dir()),
            format!("{}/common/chart_values", temp_dir.to_string_lossy()),
        ),
    ];
    for (source_dir, target_dir) in dirs_to_be_copied_to {
        if let Err(e) = crate::template::copy_non_template_files(&source_dir, target_dir.as_str()) {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details.clone(),
                source_dir,
                target_dir,
                e,
            )));
        }
    }

    let credentials_environment_variables: Vec<(String, String)> = cloud_provider
        .credentials_environment_variables()
        .into_iter()
        .map(|x| (x.0.to_string(), x.1.to_string()))
        .collect();

    if kubernetes.is_karpenter_enabled() {
        let kubernetes = kubernetes.as_eks().expect("expected EKS cluster here");
        if let Some(karpenter_parameters) = &kubernetes.get_karpenter_parameters() {
            if karpenter_parameters.spot_enabled {
                block_on(Karpenter::create_aws_service_role_for_ec2_spot(&aws_conn, &event_details))?;
            }
        }

        if Karpenter::is_paused(&infra_ctx.mk_kube_client()?, &event_details)? {
            let kube_client = infra_ctx.mk_kube_client()?;
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

    if let Err(e) = kubectl_are_qovery_infra_pods_executed(&kubeconfig_path, &credentials_environment_variables) {
        logger.warn(EventMessage::new(
            "Didn't manage to restart all paused pods".to_string(),
            Some(e.to_string()),
        ));
    }

    // When the user control the network/vpc configuration, we may hit a bug of the in tree aws load balancer controller
    // were if there is a custom dns server set for the VPC, kube-proxy nodes are not correctly configured and load balancer healthcheck are failing
    // The correct fix would be to stop using the k8s in tree lb controller, and use instead the external aws lb controller.
    // But as we don't want to do the migration for all users, we will just patch the kube-proxy configuration on the fly
    // https://aws.amazon.com/premiumsupport/knowledge-center/eks-troubleshoot-unhealthy-targets-nlb/
    // https://github.com/kubernetes/kubernetes/issues/80579
    // https://github.com/kubernetes/cloud-provider-aws/issues/87
    if kubernetes.is_network_managed_by_user()
        && kubernetes.kind() == Kind::Eks
        && !kubernetes.advanced_settings().aws_eks_enable_alb_controller
    {
        info!("patching kube-proxy configuration to fix k8s in tree load balancer controller bug");
        block_on(patch_kube_proxy_for_aws_user_network(
            infra_ctx.mk_kube_client()?.client().clone(),
        ))
        .map_err(|e| {
            EngineError::new_k8s_node_not_ready(
                event_details.clone(),
                CommandError::new_from_safe_message(format!(
                    "Cannot patch kube proxy for user configured network: {e}"
                )),
            )
        })?;
    }

    let qube_client = infra_ctx.mk_kube_client()?;

    // check if alb controller is already enabled to decide if webhooks should be enabled or not
    let found_alb_mutating_configs = block_on(
        qube_client
            .get_mutating_webhook_configurations(event_details.clone(), SelectK8sResourceBy::Name("xxx".to_string())),
    )?;
    let alb_already_deployed = !found_alb_mutating_configs.is_empty();

    // retrieve cluster CPU architectures
    let charts_prerequisites = EksChartsConfigPrerequisites {
        organization_id: cloud_provider.organization_id().to_string(),
        organization_long_id: cloud_provider.organization_long_id(),
        infra_options: kubernetes.options.clone(),
        cluster_id: kubernetes.short_id().to_string(),
        cluster_long_id: kubernetes.long_id,
        region: AwsRegion::from_str(kubernetes.region()).map_err(|_e| {
            EngineError::new_unsupported_region(event_details.clone(), kubernetes.region().to_string(), None)
        })?,
        cluster_name: kubernetes.cluster_name(),
        cpu_architectures: kubernetes.cpu_architectures(),
        cloud_provider: "aws".to_string(),
        qovery_engine_location: kubernetes.options.qovery_engine_location.clone(),
        ff_log_history_enabled: kubernetes.context().is_feature_enabled(&Features::LogsHistory),
        ff_metrics_history_enabled: kubernetes.context().is_feature_enabled(&Features::MetricsHistory),
        ff_grafana_enabled: kubernetes.context().is_feature_enabled(&Features::Grafana),
        managed_dns_helm_format: dns_provider.domain().to_helm_format_string(),
        managed_dns_resolvers_terraform_format: terraform_list_format(
            dns_provider.resolvers().iter().map(|x| x.clone().to_string()).collect(),
        ),
        managed_dns_root_domain_helm_format: dns_provider.domain().root_domain().to_helm_format_string(),
        lets_encrypt_config: LetsEncryptConfig::new(
            kubernetes.options.tls_email_report.to_string(),
            kubernetes.context().is_test_cluster(),
        ),
        dns_provider_config: dns_provider.provider_configuration(),
        cluster_advanced_settings: kubernetes.advanced_settings().clone(),
        is_karpenter_enabled: kubernetes.is_karpenter_enabled(),
        karpenter_parameters: kubernetes.get_karpenter_parameters(),
        aws_account_id: eks_tf_output.aws_account_id.clone(),
        aws_iam_eks_user_mapper_role_arn: eks_tf_output.aws_iam_eks_user_mapper_role_arn.clone(),
        aws_iam_cluster_autoscaler_role_arn: eks_tf_output.aws_iam_cluster_autoscaler_role_arn.clone(),
        aws_iam_cloudwatch_role_arn: eks_tf_output.aws_iam_cloudwatch_role_arn.clone(),
        aws_iam_loki_role_arn: eks_tf_output.aws_iam_loki_role_arn.clone(),
        aws_s3_loki_bucket_name: eks_tf_output.aws_s3_loki_bucket_name.clone(),
        loki_storage_config_aws_s3: eks_tf_output.loki_storage_config_aws_s3.clone(),
        karpenter_controller_aws_role_arn: eks_tf_output.karpenter_controller_aws_role_arn.clone(),
        cluster_security_group_id: eks_tf_output.cluster_security_group_id.clone(),
        alb_controller_already_deployed: alb_already_deployed,
        kubernetes_version_upgrade_requested,
        aws_iam_alb_controller_arn: eks_tf_output.aws_iam_alb_controller_arn.clone(),
    };
    let helm_charts_to_deploy = eks_helm_charts(
        &charts_prerequisites,
        Some(temp_dir.to_string_lossy().as_ref()),
        &kubeconfig_path,
        &*kubernetes.context().qovery_api,
        kubernetes.customer_helm_charts_override(),
        dns_provider.domain(),
    )
    .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?;

    // before deploying Helm charts, we need to check if Nginx ingress controller needs to move NLB to ALB controller
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

    deploy_charts_levels(
        qube_client.client(),
        &kubeconfig_path,
        credentials_environment_variables
            .iter()
            .map(|(l, r)| (l.as_str(), r.as_str()))
            .collect_vec()
            .as_slice(),
        helm_charts_to_deploy,
        kubernetes.context().is_dry_run_deploy(),
        Some(&infra_ctx.kubernetes().helm_charts_diffs_directory()),
    )
    .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))?;

    clean_karpenter_installation(kubernetes, infra_ctx, &logger, event_details.clone(), aws_eks_client)?;

    Ok(())
}

// after Karpenter is deployed, we can remove the node groups
// after Karpenter is deployed, we can remove fargate profile for add-ons
// TODO: remove this function once every cluster has Karpenter enabled.
// It is only needed for the transition between nodegroup to karpente
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

    let has_node_group_running = kubernetes.nodes_groups.iter().any(|ng| {
        matches!(
            node_group_is_running(kubernetes, &event_details, ng, aws_eks_client.clone()),
            Ok(Some(_v))
        )
    });

    if !(has_node_group_running || kubernetes.context().is_first_cluster_deployment()) {
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
        kubernetes.temp_dir().join("terrafor_karpenter_cleanup"),
        event_details.clone(),
        infra_ctx.context().is_dry_run_deploy(),
    );

    let _: AwsEksQoveryTerraformOutput = tf_action.create(
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
        logger,
    )?;

    Ok(())
}
