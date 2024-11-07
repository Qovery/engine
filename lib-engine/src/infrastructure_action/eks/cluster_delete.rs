use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::regions::AwsZone;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubeconfig_helper::{delete_kubeconfig_from_object_storage, fetch_kubeconfig};
use crate::cloud_provider::kubernetes::{Kind, Kubernetes};
use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState};
use crate::cloud_provider::utilities::{wait_until_port_is_open, TcpCheckSource};
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsAws};
use crate::cloud_provider::CloudProvider;
use crate::cmd::terraform::TerraformError;
use crate::cmd::terraform_validators::TerraformValidators;
use crate::dns_provider::DnsProvider;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::{EventMessage, InfrastructureStep, Stage};
use crate::infrastructure_action::delete_kube_apps::delete_kube_apps;
use crate::infrastructure_action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure_action::ec2_k3s::sdk::QoveryAwsSdkConfigEc2;
use crate::infrastructure_action::ec2_k3s::AwsEc2QoveryTerraformOutput;
use crate::infrastructure_action::eks::karpenter::node_groups_when_karpenter_is_enabled;
use crate::infrastructure_action::eks::karpenter::Karpenter;
use crate::infrastructure_action::eks::nodegroup::{
    delete_eks_nodegroups, should_update_desired_nodes, NodeGroupsDeletionType,
};
use crate::infrastructure_action::eks::tera_context::eks_tera_context;
use crate::infrastructure_action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure_action::eks::{AwsEksQoveryTerraformOutput, AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION};
use crate::infrastructure_action::InfraLogger;
use crate::object_storage::ObjectStorage;
use crate::runtime::block_on;
use crate::secret_manager::vault::QVaultClient;
use crate::services::kube_client::SelectK8sResourceBy;
use crate::{cmd, secret_manager};
use retry::delay::Fixed;
use retry::{Error, OperationResult};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

pub fn delete_eks_cluster(
    infra_ctx: &InfrastructureContext,
    kubernetes: &dyn Kubernetes,
    cloud_provider: &dyn CloudProvider,
    dns_provider: &dyn DnsProvider,
    object_store: &dyn ObjectStorage,
    aws_zones: &[AwsZone],
    node_groups: &[NodeGroups],
    options: &Options,
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));

    logger.info("Preparing cluster deletion.");
    let aws_conn = cloud_provider
        .aws_sdk_client()
        .ok_or_else(|| Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details.clone())))?;
    let template_directory = match kubernetes.as_eks() {
        Some(eks) => PathBuf::from(&eks.template_directory),
        None => PathBuf::from(&kubernetes.as_ec2().unwrap().template_directory),
    };

    let temp_dir = kubernetes.temp_dir();
    let node_groups = match kubernetes.as_eks() {
        Some(_) => {
            let node_groups = node_groups_when_karpenter_is_enabled(
                kubernetes,
                infra_ctx,
                node_groups,
                &event_details,
                KubernetesClusterAction::Delete,
            )?;

            should_update_desired_nodes(
                event_details.clone(),
                kubernetes,
                KubernetesClusterAction::Delete,
                node_groups,
                get_rusoto_eks_client(event_details.clone(), kubernetes, cloud_provider).ok(),
            )?
        }
        // It is EC2
        None => {
            vec![NodeGroupsWithDesiredState::new_from_node_groups(
                &node_groups[0],
                1,
                false,
            )]
        }
    };

    // generate terraform files and copy them into temp dir
    // in case error, this should no be a blocking error
    let mut cluster_upgrade_timeout_in_min = AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
    if let Some(kube_client) = infra_ctx.mk_kube_client() {
        let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
            .unwrap_or(Vec::with_capacity(0));

        let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Delete);
        cluster_upgrade_timeout_in_min = timeout;
        if let Some(x) = message {
            logger.info(x);
        }
    }

    let mut tera_context = eks_tera_context(
        kubernetes,
        cloud_provider,
        dns_provider,
        aws_zones,
        &node_groups,
        options,
        cluster_upgrade_timeout_in_min,
        false,
        advanced_settings,
        qovery_allowed_public_access_cidrs,
    )?;
    tera_context.insert("is_deletion_step", &true);

    let tf_resources = TerraformInfraResources::new(
        tera_context.clone(),
        template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        infra_ctx.context().is_dry_run_deploy(),
    );

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    let message = format!(
        "Ensuring everything is up to date before deleting cluster {}/{}",
        kubernetes.name(),
        kubernetes.short_id()
    );

    logger.info(message);
    logger.info("Running Terraform apply before running a delete.");

    match kubernetes.as_ec2() {
        // EKS
        None => {
            let _: Result<AwsEksQoveryTerraformOutput, Box<EngineError>> = tf_resources
                .create(cloud_provider.credentials_environment_variables().as_slice(), &logger)
                .inspect_err(|e| {
                    logger.warn(EventMessage::new(
                        "Terraform apply before delete failed. It may occur but may not be blocking.".to_string(),
                        Some(e.to_string()),
                    ));
                });
            kubernetes.kubeconfig_local_file_path()
        }

        Some(ec2) => {
            let qovery_terraform_output: AwsEc2QoveryTerraformOutput = tf_resources
                .create(cloud_provider.credentials_environment_variables().as_slice(), &logger)
                .inspect_err(|e| {
                    logger.warn(EventMessage::new(
                        "Terraform apply before delete failed. It may occur but may not be blocking.".to_string(),
                        Some(e.to_string()),
                    ));
                })?;

            // delete kubeconfig on s3 to avoid obsolete kubeconfig (not for EC2 because S3 kubeconfig upload is not done the same way)
            let _ = delete_kubeconfig_from_object_storage(ec2, object_store);

            // send cluster info to vault if info mismatch
            // create vault connection (Vault connectivity should not be on the critical deployment path,
            // if it temporarily fails, just ignore it, data will be pushed on the next sync)
            let cluster_secrets = ClusterSecrets::new_aws_eks(ClusterSecretsAws::new(
                cloud_provider.access_key_id(),
                kubernetes.region().to_string(),
                cloud_provider.secret_access_key(),
                None,
                Some(qovery_terraform_output.aws_ec2_public_hostname.clone()),
                kubernetes.kind(),
                kubernetes.cluster_name(),
                kubernetes.long_id().to_string(),
                options.grafana_admin_user.clone(),
                options.grafana_admin_password.clone(),
                cloud_provider.organization_id().to_string(),
                kubernetes.context().is_test_cluster(),
            ));
            if let Ok(vault) = QVaultClient::new(event_details.clone()) {
                let _ = cluster_secrets.create_or_update_secret(&vault, true, event_details.clone());
            };

            let port = qovery_terraform_output.kubernetes_port_to_u16().map_err(|e| {
                Box::new(EngineError::new_terraform_error(
                    event_details.clone(),
                    TerraformError::ConfigFileInvalidContent {
                        path: "ec2 terraform output".to_string(),
                        raw_message: e,
                    },
                ))
            })?;

            // wait for k3s port to be open
            // retry for 10 min, a reboot will occur after 5 min if nothing happens (see EC2 Terraform user config)
            wait_until_port_is_open(
                &TcpCheckSource::DnsName(qovery_terraform_output.aws_ec2_public_hostname.as_str()),
                port,
                600,
                kubernetes.logger(),
                event_details.clone(),
            )
            .map_err(|_| EngineError::new_k8s_cannot_reach_api(event_details.clone()))?;

            // during an instance replacement, the EC2 host dns will change and will require the kubeconfig to be updated
            // we need to ensure the kubeconfig is the correct one by checking the current instance dns in the kubeconfig
            let result = retry::retry(Fixed::from_millis(5 * 1000).take(120), || {
                match fetch_kubeconfig(kubernetes, object_store) {
                    Ok(_) => (),
                    Err(e) => return OperationResult::Retry(e),
                };

                let current_kubeconfig_path = kubernetes.kubeconfig_local_file_path();
                let mut kubeconfig_file = File::open(&current_kubeconfig_path).expect("Cannot open kubeconfig file");

                // ensure the kubeconfig content address match with the current instance dns
                let mut buffer = String::new();
                let _ = kubeconfig_file.read_to_string(&mut buffer);
                match buffer.contains(&qovery_terraform_output.aws_ec2_public_hostname) {
                    true => {
                        logger.info(format!(
                            "kubeconfig stored on s3 do correspond with the actual host {}",
                            &qovery_terraform_output.aws_ec2_public_hostname
                        ));
                        OperationResult::Ok(current_kubeconfig_path)
                    }
                    false => {
                        logger.warn(
                            EventMessage::new_from_safe(format!(
                                "kubeconfig stored on s3 do not yet correspond with the actual host {}, retrying in 5 sec...",
                                &qovery_terraform_output.aws_ec2_public_hostname
                            )),
                        );
                        OperationResult::Retry(Box::new(
                            EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(event_details.clone()),
                        ))
                    }
                }
            });

            match result {
                Ok(x) => x,
                Err(Error { error, .. }) => return Err(error),
            }
        }
    };

    delete_kube_apps(kubernetes, infra_ctx, event_details.clone(), &logger)?;

    logger.info(format!(
        "Deleting Kubernetes cluster {}/{}",
        kubernetes.name(),
        kubernetes.short_id()
    ));
    if let Some(kubernetes) = kubernetes.as_eks() {
        if kubernetes.is_karpenter_enabled() {
            let kube_client = infra_ctx.mk_kube_client()?;
            block_on(Karpenter::delete(kubernetes, cloud_provider, &kube_client))?;
        } else {
            // remove all node groups to avoid issues because of nodegroups manually added by user, making terraform unable to delete the EKS cluster
            block_on(delete_eks_nodegroups(
                aws_conn,
                kubernetes.cluster_name(),
                kubernetes.context().is_first_cluster_deployment(),
                NodeGroupsDeletionType::All,
                event_details.clone(),
            ))?;
        }

        // remove S3 buckets from tf state
        // TODO: Why do we forgot them ?
        logger.info("Removing S3 buckets from tf state");
        let resources_to_be_removed_from_tf_state: Vec<(&str, &str)> = vec![
            ("aws_s3_bucket.loki_bucket", "S3 logs bucket"),
            ("aws_s3_bucket_lifecycle_configuration.loki_lifecycle", "S3 logs lifecycle"),
            ("aws_s3_bucket.vpc_flow_logs", "S3 flow logs bucket"),
            (
                "aws_s3_bucket_lifecycle_configuration.vpc_flow_logs_lifecycle",
                "S3 vpc log flow lifecycle",
            ),
        ];

        for resource_to_be_removed_from_tf_state in resources_to_be_removed_from_tf_state {
            match cmd::terraform::terraform_remove_resource_from_tf_state(
                temp_dir.join("terraform").to_string_lossy().as_ref(),
                resource_to_be_removed_from_tf_state.0,
                &TerraformValidators::None,
            ) {
                Ok(_) => {
                    logger.info(format!(
                        "{} successfully removed from tf state.",
                        resource_to_be_removed_from_tf_state.1
                    ));
                }
                Err(err) => {
                    // We weren't able to remove S3 bucket from tf state, maybe it's not there?
                    // Anyways, this is not blocking
                    logger.warn(EventMessage::new_from_engine_error(EngineError::new_terraform_error(
                        event_details.clone(),
                        err,
                    )));
                }
            }
        }
    }

    logger.info("Running Terraform destroy");
    if kubernetes.kind() == Kind::Ec2 {
        match cloud_provider.aws_sdk_client() {
            None => return Err(Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details))),
            Some(client) => block_on(client.detach_ec2_volumes(kubernetes.short_id(), &event_details))?,
        };
    }

    tf_resources.delete(cloud_provider.credentials_environment_variables().as_slice(), &logger)?;

    logger.info("Kubernetes cluster successfully deleted");

    // delete info on vault
    if let Ok(vault_conn) = QVaultClient::new(event_details) {
        let mount = secret_manager::vault::get_vault_mount_name(kubernetes.context().is_test_cluster());
        // ignore on failure
        let _ = vault_conn.delete_secret(mount.as_str(), kubernetes.short_id());
    };

    Ok(())
}
