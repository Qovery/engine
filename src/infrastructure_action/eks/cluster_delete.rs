use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::regions::AwsZone;
use crate::cloud_provider::helm::ChartInfo;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubeconfig_helper::{delete_kubeconfig_from_object_storage, fetch_kubeconfig};
use crate::cloud_provider::kubernetes::{uninstall_cert_manager, Kind, Kubernetes};
use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState};
use crate::cloud_provider::utilities::{wait_until_port_is_open, TcpCheckSource};
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsAws};
use crate::cloud_provider::CloudProvider;
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::cmd::kubectl::kubectl_exec_get_all_namespaces;
use crate::cmd::terraform::{terraform_init_validate_plan_apply, terraform_output, TerraformError};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EventMessage, InfrastructureStep, Stage};
use crate::infrastructure_action::ec2_k3s::sdk::QoveryAwsSdkConfigEc2;
use crate::infrastructure_action::ec2_k3s::AwsEc2QoveryTerraformOutput;
use crate::infrastructure_action::eks::helm_charts::karpenter::KarpenterChart;
use crate::infrastructure_action::eks::helm_charts::karpenter_configuration::KarpenterConfigurationChart;
use crate::infrastructure_action::eks::helm_charts::karpenter_crd::KarpenterCrdChart;
use crate::infrastructure_action::eks::karpenter::node_groups_when_karpenter_is_enabled;
use crate::infrastructure_action::eks::karpenter::Karpenter;
use crate::infrastructure_action::eks::nodegroup::{
    delete_eks_nodegroups, should_update_desired_nodes, NodeGroupsDeletionType,
};
use crate::infrastructure_action::eks::tera_context::eks_tera_context;
use crate::infrastructure_action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure_action::eks::AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
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
    template_directory: &str,
    aws_zones: &[AwsZone],
    node_groups: &[NodeGroups],
    options: &Options,
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
    let mut skip_kubernetes_step = false;

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Preparing to delete {} cluster.", kubernetes.kind())),
    ));

    let aws_conn = match cloud_provider.aws_sdk_client() {
        Some(x) => x,
        None => return Err(Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details))),
    };

    let temp_dir = kubernetes.temp_dir();
    let node_groups_with_desired_states = match kubernetes.kind() {
        Kind::Eks => {
            let applied_node_groups = if kubernetes.is_karpenter_enabled() {
                node_groups_when_karpenter_is_enabled(
                    kubernetes,
                    infra_ctx,
                    node_groups,
                    &event_details,
                    KubernetesClusterAction::Delete,
                )?
            } else {
                node_groups
            };

            let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), kubernetes, cloud_provider) {
                Ok(value) => Some(value),
                Err(_) => None,
            };

            should_update_desired_nodes(
                event_details.clone(),
                kubernetes,
                KubernetesClusterAction::Delete,
                applied_node_groups,
                aws_eks_client,
            )?
        }
        Kind::Ec2 => {
            vec![NodeGroupsWithDesiredState::new_from_node_groups(
                &node_groups[0],
                1,
                false,
            )]
        }
        _ => {
            return Err(Box::new(EngineError::new_unsupported_cluster_kind(
                event_details,
                "only AWS clusters are supported for this delete method",
                CommandError::new_from_safe_message(
                    "please contact Qovery, deletion can't happen on something else than AWS clsuter type".to_string(),
                ),
            )));
        }
    };

    // generate terraform files and copy them into temp dir
    // in case error, this should no be a blocking error
    let mut cluster_upgrade_timeout_in_min = AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
    if let Ok(kube_client) = infra_ctx.mk_kube_client() {
        let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
            .unwrap_or(Vec::with_capacity(0));

        let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Delete);
        cluster_upgrade_timeout_in_min = timeout;

        if let Some(x) = message {
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
        }
    };
    let mut context = eks_tera_context(
        kubernetes,
        cloud_provider,
        dns_provider,
        aws_zones,
        &node_groups_with_desired_states,
        options,
        cluster_upgrade_timeout_in_min,
        false,
        advanced_settings,
        qovery_allowed_public_access_cidrs,
    )?;
    context.insert("is_deletion_step", &true);

    if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir, context) {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        )));
    }

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    let message = format!(
        "Ensuring everything is up to date before deleting cluster {}/{}",
        kubernetes.name(),
        kubernetes.short_id()
    );

    kubernetes
        .logger()
        .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform apply before running a delete.".to_string()),
    ));

    if let Err(e) = terraform_init_validate_plan_apply(
        temp_dir.to_string_lossy().as_ref(),
        false,
        cloud_provider.credentials_environment_variables().as_slice(),
        &TerraformValidators::None,
    ) {
        // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
        kubernetes.logger().log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new(
                "Terraform apply before delete failed. It may occur but may not be blocking.".to_string(),
                Some(e.to_string()),
            ),
        ));
    };

    // delete kubeconfig on s3 to avoid obsolete kubeconfig (not for EC2 because S3 kubeconfig upload is not done the same way)
    if kubernetes.kind() != Kind::Ec2 {
        let _ = delete_kubeconfig_from_object_storage(kubernetes, object_store);
    };

    let kubernetes_config_file_path = match kubernetes.kind() {
        Kind::Eks => kubernetes.kubeconfig_local_file_path(),
        Kind::Ec2 => {
            let qovery_terraform_output: AwsEc2QoveryTerraformOutput = terraform_output(
                temp_dir.to_string_lossy().as_ref(),
                infra_ctx
                    .cloud_provider()
                    .credentials_environment_variables()
                    .as_slice(),
            )
            .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;
            // send cluster info to vault if info mismatch
            // create vault connection (Vault connectivity should not be on the critical deployment path,
            // if it temporarily fails, just ignore it, data will be pushed on the next sync)
            let vault_conn = match QVaultClient::new(event_details.clone()) {
                Ok(x) => Some(x),
                Err(_) => None,
            };
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
            if let Some(vault) = vault_conn {
                // update info without taking care of the kubeconfig because we don't have it yet
                let _ = cluster_secrets.create_or_update_secret(&vault, true, event_details.clone());
            };

            let port = match qovery_terraform_output.kubernetes_port_to_u16() {
                Ok(p) => p,
                Err(e) => {
                    return Err(Box::new(EngineError::new_terraform_error(
                        event_details,
                        TerraformError::ConfigFileInvalidContent {
                            path: "ec2 terraform output".to_string(),
                            raw_message: e,
                        },
                    )));
                }
            };

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
                        kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!(
                                "kubeconfig stored on s3 do correspond with the actual host {}",
                                &qovery_terraform_output.aws_ec2_public_hostname
                            )),
                        ));
                        OperationResult::Ok(current_kubeconfig_path)
                    }
                    false => {
                        kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!(
                                "kubeconfig stored on s3 do not yet correspond with the actual host {}, retrying in 5 sec...",
                                &qovery_terraform_output.aws_ec2_public_hostname
                            )),
                        ));
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
        _ => {
            let safe_message = "Skipping Kubernetes uninstall because it can't be reached.";
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new_from_safe(safe_message.to_string()),
            ));
            skip_kubernetes_step = true;
            PathBuf::from("")
        }
    };

    if !skip_kubernetes_step {
        // should make the diff between all namespaces and qovery managed namespaces
        let message = format!(
            "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
            kubernetes.name(),
            kubernetes.short_id()
        );

        kubernetes
            .logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        let all_namespaces = kubectl_exec_get_all_namespaces(
            &kubernetes_config_file_path,
            cloud_provider.credentials_environment_variables(),
        );

        match all_namespaces {
            Ok(namespace_vec) => {
                let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                kubernetes.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Deleting non Qovery namespaces".to_string()),
                ));

                for namespace_to_delete in namespaces_to_delete.iter() {
                    match cmd::kubectl::kubectl_exec_delete_namespace(
                        &kubernetes_config_file_path,
                        namespace_to_delete,
                        cloud_provider.credentials_environment_variables(),
                    ) {
                        Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!(
                                "Namespace `{namespace_to_delete}` deleted successfully."
                            )),
                        )),
                        Err(e) => {
                            if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                                kubernetes.logger().log(EngineEvent::Warning(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(format!(
                                        "Can't delete the namespace `{namespace_to_delete}`"
                                    )),
                                ));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let message_safe = format!(
                    "Error while getting all namespaces for Kubernetes cluster {}",
                    kubernetes.name_with_id(),
                );
                kubernetes.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(message_safe, Some(e.message(ErrorMessageVerbosity::FullDetails))),
                ));
            }
        }

        let message = format!(
            "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
            kubernetes.name(),
            kubernetes.short_id()
        );

        kubernetes
            .logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        // delete custom metrics api to avoid stale namespaces on deletion
        let helm = Helm::new(
            Some(&kubernetes_config_file_path),
            &cloud_provider.credentials_environment_variables(),
        )
        .map_err(|e| to_engine_error(&event_details, e))?;
        let chart = ChartInfo::new_from_release_name("metrics-server", "kube-system");
        if let Err(e) = helm.uninstall(&chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
            // this error is not blocking
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new_from_engine_error(to_engine_error(&event_details, e)),
            ));
        }

        // required to avoid namespace stuck on deletion
        if let Err(e) = uninstall_cert_manager(
            &kubernetes_config_file_path,
            cloud_provider.credentials_environment_variables(),
            event_details.clone(),
            kubernetes.logger(),
        ) {
            // this error is not blocking, logging a warning and move on
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(
                    "An error occurred while trying to uninstall cert-manager. This is not blocking.".to_string(),
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                ),
            ));
        }

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deleting Qovery managed helm charts".to_string()),
        ));

        let qovery_namespaces = get_qovery_managed_namespaces();
        for qovery_namespace in qovery_namespaces.iter() {
            let charts_to_delete = helm
                .list_release(Some(qovery_namespace), &[])
                .map_err(|e| to_engine_error(&event_details, e))?;

            for chart in charts_to_delete {
                let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                    Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                    )),
                    Err(e) => kubernetes.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(format!("Can't delete chart `{}`", &chart.name), Some(e.to_string())),
                    )),
                }
            }
        }

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deleting Qovery managed namespaces".to_string()),
        ));

        for qovery_namespace in qovery_namespaces.iter() {
            let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                &kubernetes_config_file_path,
                qovery_namespace,
                cloud_provider.credentials_environment_variables(),
            );
            match deletion {
                Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!("Namespace {qovery_namespace} is fully deleted")),
                )),
                Err(e) => {
                    if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                        kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Can't delete namespace {qovery_namespace}.")),
                        ))
                    }
                }
            }
        }

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Delete all remaining deployed helm applications".to_string()),
        ));

        // Do not uninstall Karpenter to be able to delete the nodes properly .
        match helm.list_release(None, &[]) {
            Ok(helm_charts) => {
                for chart in helm_charts.into_iter().filter(|helm_chart| {
                    helm_chart.name != KarpenterChart::chart_name()
                        && helm_chart.name != KarpenterConfigurationChart::chart_name()
                        && helm_chart.name != KarpenterCrdChart::chart_name()
                }) {
                    let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                    match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                        Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                        )),
                        Err(e) => kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new(format!("Error deleting chart `{}`", chart.name), Some(e.to_string())),
                        )),
                    }
                }
            }
            Err(e) => kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new("Unable to get helm list".to_string(), Some(e.to_string())),
            )),
        }
    };

    let message = format!("Deleting Kubernetes cluster {}/{}", kubernetes.name(), kubernetes.short_id());
    kubernetes
        .logger()
        .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

    if let Some(kubernetes) = kubernetes.as_eks() {
        // remove all node groups to avoid issues because of nodegroups manually added by user, making terraform unable to delete the EKS cluster
        block_on(delete_eks_nodegroups(
            aws_conn,
            kubernetes.cluster_name(),
            kubernetes.context().is_first_cluster_deployment(),
            NodeGroupsDeletionType::All,
            event_details.clone(),
        ))?;

        if kubernetes.is_karpenter_enabled() {
            let kube_client = infra_ctx.mk_kube_client()?;
            block_on(Karpenter::delete(kubernetes, cloud_provider, &kube_client))?;
        }

        // remove S3 buckets from tf state
        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Removing S3 logs bucket from tf state".to_string()),
        ));
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
                temp_dir.to_string_lossy().as_ref(),
                resource_to_be_removed_from_tf_state.0,
                &TerraformValidators::None,
            ) {
                Ok(_) => {
                    kubernetes.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!(
                            "{} successfully removed from tf state.",
                            resource_to_be_removed_from_tf_state.1
                        )),
                    ));
                }
                Err(err) => {
                    // We weren't able to remove S3 bucket from tf state, maybe it's not there?
                    // Anyways, this is not blocking
                    kubernetes.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new_from_engine_error(EngineError::new_terraform_error(
                            event_details.clone(),
                            err,
                        )),
                    ));
                }
            }
        }
    }

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform destroy".to_string()),
    ));

    if kubernetes.kind() == Kind::Ec2 {
        match cloud_provider.aws_sdk_client() {
            None => return Err(Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details))),
            Some(client) => block_on(client.detach_ec2_volumes(kubernetes.short_id(), &event_details))?,
        };
    }

    if let Err(err) = cmd::terraform::terraform_init_validate_destroy(
        temp_dir.to_string_lossy().as_ref(),
        false,
        cloud_provider.credentials_environment_variables().as_slice(),
        &TerraformValidators::None,
    ) {
        return Err(Box::new(EngineError::new_terraform_error(event_details, err)));
    }
    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Kubernetes cluster successfully deleted".to_string()),
    ));

    // delete info on vault
    let vault_conn = QVaultClient::new(event_details);
    if let Ok(vault_conn) = vault_conn {
        let mount = secret_manager::vault::get_vault_mount_name(kubernetes.context().is_test_cluster());

        // ignore on failure
        let _ = vault_conn.delete_secret(mount.as_str(), kubernetes.short_id());
    };

    Ok(())
}
