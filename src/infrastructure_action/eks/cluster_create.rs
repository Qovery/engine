use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZone};
use crate::cloud_provider::helm::deploy_charts_levels;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubeconfig_helper::put_kubeconfig_file_to_object_storage;
use crate::cloud_provider::kubernetes::{is_kubernetes_upgrade_required, Kind, Kubernetes};
use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups};
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsAws};
use crate::cloud_provider::CloudProvider;
use crate::cmd::kubectl_utils::kubectl_are_qovery_infra_pods_executed;
use crate::cmd::terraform::{
    force_terraform_ec2_instance_type_switch, terraform_import, terraform_init_validate_plan_apply, terraform_output,
    TerraformError,
};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::dns_provider::DnsProvider;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity, Tag};
use crate::events::{EngineEvent, EventMessage, InfrastructureStep, Stage};
use crate::infrastructure_action::ec2_k3s;
use crate::infrastructure_action::ec2_k3s::helm_charts::{ec2_k3s_helm_charts, Ec2ChartsConfigPrerequisites};
use crate::infrastructure_action::ec2_k3s::AwsEc2QoveryTerraformOutput;
use crate::infrastructure_action::eks::custom_vpc::patch_kube_proxy_for_aws_user_network;
use crate::infrastructure_action::eks::helm_charts::{eks_helm_charts, EksChartsConfigPrerequisites};
use crate::infrastructure_action::eks::karpenter::Karpenter;
use crate::infrastructure_action::eks::karpenter::{
    bootstrap_on_fargate_when_karpenter_is_enabled, node_groups_when_karpenter_is_enabled,
};
use crate::infrastructure_action::eks::nodegroup::{
    delete_eks_nodegroups, node_group_is_running, should_update_desired_nodes, NodeGroupsDeletionType,
};
use crate::infrastructure_action::eks::sdk::QoveryAwsSdkConfigEks;
use crate::infrastructure_action::eks::tera_context::eks_tera_context;
use crate::infrastructure_action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure_action::eks::{AwsEksQoveryTerraformOutput, AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION};
use crate::io_models::context::Features;
use crate::models::domain::ToHelmString;
use crate::models::kubernetes::K8sObject;
use crate::models::third_parties::LetsEncryptConfig;
use crate::object_storage::ObjectStorage;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use crate::string::terraform_list_format;
use itertools::Itertools;
use retry::delay::Fixed;
use retry::{Error, OperationResult};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

pub fn create_eks_cluster(
    infra_ctx: &InfrastructureContext,
    kubernetes: &dyn Kubernetes,
    cloud_provider: &dyn CloudProvider,
    dns_provider: &dyn DnsProvider,
    object_store: &dyn ObjectStorage,
    kubernetes_long_id: uuid::Uuid,
    template_directory: &str,
    aws_zones: &[AwsZone],
    node_groups: &[NodeGroups],
    options: &Options,
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Preparing {} cluster deployment.", kubernetes.kind())),
    ));

    let cluster_secrets = ClusterSecrets::new_aws_eks(ClusterSecretsAws::new(
        cloud_provider.access_key_id(),
        kubernetes.region().to_string(),
        cloud_provider.secret_access_key(),
        None,
        None,
        kubernetes.kind(),
        kubernetes.cluster_name(),
        kubernetes_long_id.to_string(),
        options.grafana_admin_user.clone(),
        options.grafana_admin_password.clone(),
        cloud_provider.organization_long_id().to_string(),
        kubernetes.context().is_test_cluster(),
    ));
    let temp_dir = kubernetes.temp_dir();

    // old method with rusoto
    let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), kubernetes, cloud_provider) {
        Ok(value) => Some(value),
        Err(_) => None,
    };

    // aws connection
    let aws_conn = match cloud_provider.aws_sdk_client() {
        Some(x) => x,
        None => return Err(Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details))),
    };

    let terraform_apply = |kubernetes_action: KubernetesClusterAction| {
        // don't create node groups if karpenter is enabled
        let applied_node_groups = if kubernetes.is_karpenter_enabled() {
            node_groups_when_karpenter_is_enabled(
                kubernetes,
                infra_ctx,
                node_groups,
                &event_details,
                kubernetes_action,
            )?
        } else {
            node_groups
        };

        let bootstrap_on_fargate = if kubernetes.is_karpenter_enabled() {
            bootstrap_on_fargate_when_karpenter_is_enabled(kubernetes, kubernetes_action)
        } else {
            false
        };

        let node_groups_with_desired_states = should_update_desired_nodes(
            event_details.clone(),
            kubernetes,
            kubernetes_action,
            applied_node_groups,
            aws_eks_client.clone(),
        )?;

        // in case error, this should no be a blocking error
        let mut cluster_upgrade_timeout_in_min = AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
        if let Ok(kube_client) = infra_ctx.mk_kube_client() {
            let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
                .unwrap_or(Vec::with_capacity(0));

            let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Upgrade(None));
            cluster_upgrade_timeout_in_min = timeout;

            if let Some(x) = message {
                kubernetes
                    .logger()
                    .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
            }
        };

        // generate terraform files and copy them into temp dir
        let context = eks_tera_context(
            kubernetes,
            cloud_provider,
            dns_provider,
            aws_zones,
            &node_groups_with_desired_states,
            options,
            cluster_upgrade_timeout_in_min,
            bootstrap_on_fargate,
            advanced_settings,
            qovery_allowed_public_access_cidrs,
        )?;

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir, context) {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details.clone(),
                template_directory.to_string(),
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

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Deploying {} cluster.", kubernetes.kind())),
        ));

        let tf_apply_result = retry::retry(Fixed::from_millis(3000).take(1), || {
            match terraform_init_validate_plan_apply(
                temp_dir.to_string_lossy().as_ref(),
                kubernetes.context().is_dry_run_deploy(),
                cloud_provider.credentials_environment_variables().as_slice(),
                &TerraformValidators::Default,
            ) {
                Ok(_) => OperationResult::Ok(()),
                Err(e) => {
                    match &e {
                        TerraformError::S3BucketAlreadyOwnedByYou {
                            bucket_name,
                            terraform_resource_name,
                            ..
                        } => {
                            // Try to import S3 bucket and relaunch Terraform apply
                            kubernetes.logger().log(EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new(
                                    format!("There was an issue trying to create the S3 bucket `{bucket_name}`, trying to import it."),
                                    Some(e.to_string()),
                                ),
                            ));
                            match terraform_import(
                                temp_dir.to_string_lossy().as_ref(),
                                format!("aws_s3_bucket.{terraform_resource_name}").as_str(),
                                bucket_name,
                                cloud_provider.credentials_environment_variables().as_slice(),
                                &TerraformValidators::Default,
                            ) {
                                Ok(_) => {
                                    kubernetes.logger().log(EngineEvent::Info(
                                        event_details.clone(),
                                        EventMessage::new_from_safe(format!(
                                            "S3 bucket `{bucket_name}` has been imported properly."
                                        )),
                                    ));

                                    // triggering retry (applying Terraform apply)
                                    OperationResult::Retry(Box::new(EngineError::new_terraform_error(
                                        event_details.clone(),
                                        e.clone(),
                                    )))
                                }
                                Err(e) => OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                    event_details.clone(),
                                    e,
                                ))),
                            }
                        }
                        _ => match kubernetes.kind() {
                            Kind::Eks => {
                                // on EKS, clean possible nodegroup deployment failures because of quota issues
                                // do not exit on this error to avoid masking the real Terraform issue
                                kubernetes.logger().log(EngineEvent::Info(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(
                                        "Ensuring no failed nodegroups are present in the cluster, or delete them if at least one active nodegroup is present".to_string()
                                    ),
                                ));
                                if let Err(e) = block_on(delete_eks_nodegroups(
                                    aws_conn.clone(),
                                    kubernetes.cluster_name(),
                                    kubernetes.context().is_first_cluster_deployment(),
                                    NodeGroupsDeletionType::FailedOnly,
                                    event_details.clone(),
                                )) {
                                    // only return failures if the cluster is not absent, because it can be a VPC quota issue
                                    if e.tag() != &Tag::CannotGetCluster {
                                        return OperationResult::Err(e);
                                    }
                                }

                                OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                    event_details.clone(),
                                    e.clone(),
                                )))
                            }
                            Kind::Ec2 => {
                                if let Err(err) = force_terraform_ec2_instance_type_switch(
                                    temp_dir.to_string_lossy().as_ref(),
                                    e.clone(),
                                    kubernetes.logger(),
                                    &event_details,
                                    kubernetes.context().is_dry_run_deploy(),
                                    cloud_provider.credentials_environment_variables().as_slice(),
                                    &TerraformValidators::None, // no validator for now, it's likely to introduce a destructive change
                                ) {
                                    return OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                        event_details.clone(),
                                        err,
                                    )));
                                }

                                OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                    event_details.clone(),
                                    e.clone(),
                                )))
                            }
                            _ => OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                event_details.clone(),
                                e,
                            ))),
                        },
                    }
                }
            }
        });

        match tf_apply_result {
            Ok(_) => Ok(()),
            Err(Error { error, .. }) => Err(error),
        }
    };

    let mut kubernetes_version_upgrade_requested = false;

    // upgrade cluster instead if required
    if kubernetes.context().is_first_cluster_deployment() {
        // terraform deployment dedicated to cloud resources
        terraform_apply(KubernetesClusterAction::Bootstrap)?;
    } else {
        // on EKS, we need to check if there is no already deployed failed nodegroups to avoid future quota issues
        if kubernetes.kind() == Kind::Eks {
            kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(
                    "Ensuring no failed nodegroups are present in the cluster, or delete them if at least one active nodegroup is present".to_string(),
                )));

            if let Err(e) = block_on(delete_eks_nodegroups(
                aws_conn.clone(),
                kubernetes.cluster_name(),
                kubernetes.context().is_first_cluster_deployment(),
                NodeGroupsDeletionType::FailedOnly,
                event_details.clone(),
            )) {
                let is_the_only_nodegroup_available =
                    match block_on(aws_conn.list_all_eks_nodegroups(kubernetes.cluster_name())) {
                        Ok(x) => matches!(x.nodegroups(), n if n.len() == 1),
                        Err(_) => false,
                    };
                // only return failures if the cluster is not absent, because it can be a VPC quota issue
                if e.tag() != &Tag::CannotGetCluster && !is_the_only_nodegroup_available {
                    return Err(e);
                }
            }
        };
        match is_kubernetes_upgrade_required(
            kubernetes.kubeconfig_local_file_path(),
            kubernetes.version(),
            cloud_provider.credentials_environment_variables(),
            event_details.clone(),
            kubernetes.logger(),
            match kubernetes.is_karpenter_enabled() {
                true => Some("eks.amazonaws.com/compute-type!=fargate"), // Exclude fargate nodes from the test in case of karpenter, those will be recreated after helm deploy
                false => None,
            },
        ) {
            Ok(x) => {
                if x.required_upgrade_on.is_some() {
                    kubernetes_version_upgrade_requested = true;

                    // useful for debug purpose: we update here Vault with the name of the instance only because k3s is not ready yet (after upgrade)
                    let res = kubernetes.upgrade_with_status(infra_ctx, x);
                    // push endpoint to Vault for EC2
                    if kubernetes.kind() == Kind::Ec2 {
                        // TODO: FIX for EC2
                        //let qovery_terraform_config =
                        //    get_aws_ec2_qovery_terraform_config(qovery_terraform_config_file.as_str())
                        //        .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;
                        //cluster_secrets.set_k8s_cluster_endpoint(qovery_terraform_config.aws_ec2_public_hostname);
                        //let _ = kubernetes.update_vault_config(
                        //    event_details.clone(),
                        //    qovery_terraform_config_file.clone(),
                        //    cluster_secrets.clone(),
                        //    None,
                        //);
                    };
                    // return error on upgrade failure
                    res?;
                } else {
                    kubernetes.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                    ));
                }
            }
            Err(e) => {
                // Log a warning, this error is not blocking
                kubernetes.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(
                        "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                        Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }
        };
    }

    // apply to generate tf_qovery_config.json
    terraform_apply(KubernetesClusterAction::Update(None))?;
    let (eks_tf_output, ec2_tf_output) = match kubernetes.kind() {
        Kind::Eks => {
            let eks_tf_output = terraform_output(
                temp_dir.to_string_lossy().as_ref(),
                cloud_provider.credentials_environment_variables().as_slice(),
            )
            .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;
            (eks_tf_output, AwsEc2QoveryTerraformOutput::default())
        }
        Kind::Ec2 => {
            let ec2_tf_output = terraform_output(
                temp_dir.to_string_lossy().as_ref(),
                cloud_provider.credentials_environment_variables().as_slice(),
            )
            .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;
            (AwsEksQoveryTerraformOutput::default(), ec2_tf_output)
        }
        _ => {
            return Err(Box::new(EngineError::new_unsupported_cluster_kind(
                event_details,
                &kubernetes.kind().to_string(),
                CommandError::new_from_safe_message(format!(
                    "expected AWS provider here, while {} was found",
                    kubernetes.kind()
                )),
            )));
        }
    };

    let kubeconfig_path = match kubernetes.kind() {
        Kind::Eks => {
            put_kubeconfig_file_to_object_storage(kubernetes, object_store)?;
            kubernetes.kubeconfig_local_file_path()
        }
        Kind::Ec2 => {
            // wait for EC2 k3S kubeconfig to be ready and valid
            // no need to push it to object storage, it's already done by the EC2 instance itself
            // TODO: fix for ec2
            //let qovery_terraform_config = get_aws_ec2_qovery_terraform_config(qovery_terraform_config_file.as_str())
            //    .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;
            //cluster_secrets.set_k8s_cluster_endpoint(qovery_terraform_config.aws_ec2_public_hostname.clone());
            ec2_k3s::utils::get_and_check_if_kubeconfig_is_valid(
                kubernetes,
                object_store,
                event_details.clone(),
                ec2_tf_output.clone(),
            )?
        }
        _ => {
            return Err(Box::new(EngineError::new_unsupported_cluster_kind(
                event_details,
                &kubernetes.kind().to_string(),
                CommandError::new_from_safe_message(format!(
                    "expected AWS provider here, while {} was found",
                    kubernetes.kind()
                )),
            )));
        }
    };

    // send cluster info with kubeconfig
    // create vault connection (Vault connectivity should not be on the critical deployment path,
    // if it temporarily fails, just ignore it, data will be pushed on the next sync)
    let _ = kubernetes.update_vault_config(event_details.clone(), cluster_secrets, Some(&kubeconfig_path));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
    ));

    // kubernetes helm deployments on the cluster
    let kubeconfig_path = Path::new(&kubeconfig_path);

    let credentials_environment_variables: Vec<(String, String)> = cloud_provider
        .credentials_environment_variables()
        .into_iter()
        .map(|x| (x.0.to_string(), x.1.to_string()))
        .collect();

    if kubernetes.is_karpenter_enabled() {
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
                kubernetes_long_id,
                options,
            ))?;
        }
    }

    if let Err(e) = kubectl_are_qovery_infra_pods_executed(kubeconfig_path, &credentials_environment_variables) {
        kubernetes.logger().log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new("Didn't manage to restart all paused pods".to_string(), Some(e.to_string())),
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
    let cpu_architectures = kubernetes.cpu_architectures();
    let helm_charts_to_deploy = match kubernetes.kind() {
        Kind::Eks => {
            let charts_prerequisites = EksChartsConfigPrerequisites {
                organization_id: cloud_provider.organization_id().to_string(),
                organization_long_id: cloud_provider.organization_long_id(),
                infra_options: options.clone(),
                cluster_id: kubernetes.id().to_string(),
                cluster_long_id: kubernetes_long_id,
                region: AwsRegion::from_str(kubernetes.region()).map_err(|_e| {
                    EngineError::new_unsupported_region(event_details.clone(), kubernetes.region().to_string(), None)
                })?,
                cluster_name: kubernetes.cluster_name(),
                cpu_architectures,
                cloud_provider: "aws".to_string(),
                qovery_engine_location: options.qovery_engine_location.clone(),
                ff_log_history_enabled: kubernetes.context().is_feature_enabled(&Features::LogsHistory),
                ff_metrics_history_enabled: kubernetes.context().is_feature_enabled(&Features::MetricsHistory),
                ff_grafana_enabled: kubernetes.context().is_feature_enabled(&Features::Grafana),
                managed_dns_helm_format: dns_provider.domain().to_helm_format_string(),
                managed_dns_resolvers_terraform_format: terraform_list_format(
                    dns_provider.resolvers().iter().map(|x| x.clone().to_string()).collect(),
                ),
                managed_dns_root_domain_helm_format: dns_provider.domain().root_domain().to_helm_format_string(),
                lets_encrypt_config: LetsEncryptConfig::new(
                    options.tls_email_report.to_string(),
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
            eks_helm_charts(
                &charts_prerequisites,
                Some(temp_dir.to_string_lossy().as_ref()),
                kubeconfig_path,
                &*kubernetes.context().qovery_api,
                kubernetes.customer_helm_charts_override(),
                dns_provider.domain(),
            )
            .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?
        }
        Kind::Ec2 => {
            let charts_prerequisites = Ec2ChartsConfigPrerequisites {
                organization_id: cloud_provider.organization_id().to_string(),
                organization_long_id: cloud_provider.organization_long_id(),
                infra_options: options.clone(),
                cluster_id: kubernetes.id().to_string(),
                cluster_long_id: kubernetes_long_id,
                region: kubernetes.region().to_string(),
                cpu_architectures: cpu_architectures[0],
                cloud_provider: "aws".to_string(),
                aws_access_key_id: cloud_provider.access_key_id(),
                aws_secret_access_key: cloud_provider.secret_access_key(),
                qovery_engine_location: options.qovery_engine_location.clone(),
                ff_log_history_enabled: kubernetes.context().is_feature_enabled(&Features::LogsHistory),
                managed_domain: dns_provider.domain().clone(),
                managed_dns_name_wildcarded: dns_provider.domain().wildcarded().to_string(),
                managed_dns_helm_format: dns_provider.domain().to_helm_format_string(),
                managed_dns_resolvers_terraform_format: terraform_list_format(
                    dns_provider.resolvers().iter().map(|x| x.clone().to_string()).collect(),
                ),
                managed_dns_root_domain_helm_format: dns_provider.domain().root_domain().to_helm_format_string(),
                lets_encrypt_config: LetsEncryptConfig::new(
                    options.tls_email_report.to_string(),
                    kubernetes.context().is_test_cluster(),
                ),
                dns_provider_config: dns_provider.provider_configuration(),
                cluster_advanced_settings: kubernetes.advanced_settings().clone(),
                aws_account_id: ec2_tf_output.aws_aws_account_id.clone(),
                aws_ec2_public_hostname: ec2_tf_output.aws_ec2_public_hostname.clone(),
            };
            ec2_k3s_helm_charts(
                &charts_prerequisites,
                Some(temp_dir.to_string_lossy().as_ref()),
                kubeconfig_path,
                &credentials_environment_variables,
                &*kubernetes.context().qovery_api,
                kubernetes.customer_helm_charts_override(),
            )
            .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?
        }
        _ => {
            let safe_message = format!("unsupported requested cluster type: {}", kubernetes.kind());
            return Err(Box::new(EngineError::new_unsupported_cluster_kind(
                event_details,
                &safe_message,
                CommandError::new(safe_message.to_string(), None, None),
            )));
        }
    };

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

    if kubernetes.kind() == Kind::Ec2 {
        let result = retry::retry(Fixed::from(Duration::from_secs(60)).take(5), || {
            match deploy_charts_levels(
                qube_client.client(),
                kubeconfig_path,
                credentials_environment_variables
                    .iter()
                    .map(|(l, r)| (l.as_str(), r.as_str()))
                    .collect_vec()
                    .as_slice(),
                helm_charts_to_deploy.clone(),
                kubernetes.context().is_dry_run_deploy(),
                Some(&infra_ctx.kubernetes().helm_charts_diffs_directory()),
            ) {
                Ok(_) => OperationResult::Ok(()),
                Err(e) => {
                    kubernetes.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(
                            "Didn't manage to update Helm charts. Retrying...".to_string(),
                            Some(e.to_string()),
                        ),
                    ));
                    OperationResult::Retry(e)
                }
            }
        });
        match result {
            Ok(_) => Ok(()),
            Err(Error { error, .. }) => Err(error),
        }
        .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))
    } else {
        deploy_charts_levels(
            qube_client.client(),
            kubeconfig_path,
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

        if kubernetes.is_karpenter_enabled() {
            let has_node_group_running = node_groups.iter().any(|ng| {
                matches!(
                    node_group_is_running(kubernetes, &event_details, ng, aws_eks_client.clone()),
                    Ok(Some(_v))
                )
            });

            // after Karpenter is deployed, we can remove the node groups
            // after Karpenter is deployed, we can remove fargate profile for add-ons
            if has_node_group_running || kubernetes.context().is_first_cluster_deployment() {
                terraform_apply(KubernetesClusterAction::CleanKarpenterMigration)?;
            }
        }

        Ok(())
    }
}
