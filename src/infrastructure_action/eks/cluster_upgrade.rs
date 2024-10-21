use crate::cloud_provider::aws::kubernetes::eks::EKS;
use crate::cloud_provider::kubernetes::{Kubernetes, KubernetesNodesType, KubernetesUpgradeStatus};
use crate::cloud_provider::models::KubernetesClusterAction;
use crate::cmd::kubectl::{kubectl_exec_scale_replicas, ScalingKind};
use crate::cmd::terraform::terraform_init_validate_plan_apply;
use crate::cmd::terraform_validators::TerraformValidators;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep};
use crate::infrastructure_action::eks::nodegroup::should_update_desired_nodes;
use crate::infrastructure_action::eks::tera_context::eks_tera_context;
use crate::infrastructure_action::eks::utils::{
    check_workers_on_upgrade, define_cluster_upgrade_timeout, get_rusoto_eks_client,
};
use crate::infrastructure_action::eks::AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
use crate::infrastructure_action::utils::{delete_completed_jobs, delete_crashlooping_pods};
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;

pub fn upgrade_eks_cluster(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    kubernetes_upgrade_status: KubernetesUpgradeStatus,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Infrastructure(InfrastructureStep::Upgrade));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Start preparing EKS cluster upgrade process".to_string()),
    ));

    let temp_dir = kubernetes.temp_dir();
    let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), kubernetes, infra_ctx.cloud_provider()) {
        Ok(value) => Some(value),
        Err(_) => None,
    };

    let node_groups_with_desired_states = should_update_desired_nodes(
        event_details.clone(),
        kubernetes,
        KubernetesClusterAction::Upgrade(None),
        &kubernetes.nodes_groups,
        aws_eks_client,
    )?;

    // in case error, this should no be in the blocking process
    let mut cluster_upgrade_timeout_in_min = AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
    if let Ok(kube_client) = infra_ctx.mk_kube_client() {
        let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
            .unwrap_or_else(|_| Vec::with_capacity(0));

        let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Upgrade(None));
        cluster_upgrade_timeout_in_min = timeout;

        if let Some(x) = message {
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
        }
    };

    // generate terraform files and copy them into temp dir
    let mut context = eks_tera_context(
        kubernetes,
        infra_ctx.cloud_provider(),
        infra_ctx.dns_provider(),
        &kubernetes.zones,
        &node_groups_with_desired_states,
        &kubernetes.options,
        cluster_upgrade_timeout_in_min,
        false,
        &kubernetes.advanced_settings,
        kubernetes.qovery_allowed_public_access_cidrs.as_ref(),
    )?;

    //
    // Upgrade master nodes
    //
    match &kubernetes_upgrade_status.required_upgrade_on {
        Some(KubernetesNodesType::Masters) => {
            kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe("Start upgrading process for master nodes.".to_string()),
            ));

            // AWS requires the upgrade to be done in 2 steps (masters, then workers)
            // use the current kubernetes masters' version for workers, in order to avoid migration in one step
            context.insert(
                "kubernetes_master_version",
                format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
            );
            // use the current master version for workers, they will be updated later
            context.insert(
                "eks_workers_version",
                format!("{}", &kubernetes_upgrade_status.deployed_masters_version).as_str(),
            );

            if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
                kubernetes.template_directory.as_str(),
                temp_dir,
                context.clone(),
            ) {
                return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    event_details,
                    kubernetes.template_directory.to_string(),
                    temp_dir.to_string_lossy().to_string(),
                    e,
                )));
            }

            let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
            let common_bootstrap_charts = format!("{}/common/bootstrap/charts", kubernetes.context.lib_root_dir());
            if let Err(e) = crate::template::copy_non_template_files(
                common_bootstrap_charts.as_str(),
                common_charts_temp_dir.as_str(),
            ) {
                return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    event_details,
                    common_bootstrap_charts,
                    common_charts_temp_dir,
                    e,
                )));
            }

            kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe("Upgrading Kubernetes master nodes.".to_string()),
            ));

            match terraform_init_validate_plan_apply(
                temp_dir.to_string_lossy().as_ref(),
                kubernetes.context.is_dry_run_deploy(),
                infra_ctx
                    .cloud_provider()
                    .credentials_environment_variables()
                    .as_slice(),
                &TerraformValidators::Default,
            ) {
                Ok(_) => {
                    kubernetes.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(
                            "Kubernetes master nodes have been successfully upgraded.".to_string(),
                        ),
                    ));
                }
                Err(e) => {
                    return Err(Box::new(EngineError::new_terraform_error(event_details, e)));
                }
            }
        }
        Some(KubernetesNodesType::Workers) => {
            kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(
                    "No need to perform Kubernetes master upgrade, they are already up to date.".to_string(),
                ),
            ));
        }
        None => {
            kubernetes.logger().log(EngineEvent::Info(
                event_details,
                EventMessage::new_from_safe(
                    "No Kubernetes upgrade required, masters and workers are already up to date.".to_string(),
                ),
            ));
            return Ok(());
        }
    }

    //
    // Upgrade worker nodes
    //
    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing workers nodes for upgrade for Kubernetes cluster.".to_string()),
    ));

    // disable cluster autoscaler to avoid interfering with AWS upgrade procedure
    context.insert("enable_cluster_autoscaler", &false);
    context.insert(
        "eks_workers_version",
        format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
    );

    if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
        kubernetes.template_directory.as_str(),
        temp_dir,
        context.clone(),
    ) {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            kubernetes.template_directory.to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
    let common_bootstrap_charts = format!("{}/common/bootstrap/charts", kubernetes.context.lib_root_dir());
    if let Err(e) =
        crate::template::copy_non_template_files(common_bootstrap_charts.as_str(), common_charts_temp_dir.as_str())
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            common_bootstrap_charts,
            common_charts_temp_dir,
            e,
        )));
    }

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Starting Kubernetes worker nodes upgrade".to_string()),
    ));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Checking clusters content health".to_string()),
    ));

    // disable all replicas with issues to avoid upgrade failures
    let kube_client = infra_ctx.mk_kube_client()?;
    let deployments = block_on(kube_client.get_deployments(event_details.clone(), None, SelectK8sResourceBy::All))?;
    for deploy in deployments {
        let status = match deploy.status {
            Some(s) => s,
            None => continue,
        };

        let replicas = status.replicas.unwrap_or(0);
        let ready_replicas = status.ready_replicas.unwrap_or(0);

        // if number of replicas > 0: it is not already disabled
        // ready_replicas == 0: there is something in progress (rolling restart...) so we should not touch it
        if replicas > 0 && ready_replicas == 0 {
            kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!(
                    "Deployment {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                    deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
                )),
            ));
            block_on(kube_client.set_deployment_replicas_number(
                event_details.clone(),
                deploy.metadata.name.as_str(),
                deploy.metadata.namespace.as_str(),
                0,
            ))?;
        } else {
            info!(
                "Deployment {}/{} has {}/{} replicas ready. No action needed.",
                deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
            );
        }
    }
    // same with statefulsets
    let statefulsets = block_on(kube_client.get_statefulsets(event_details.clone(), None, SelectK8sResourceBy::All))?;
    for sts in statefulsets {
        let status = match sts.status {
            Some(s) => s,
            None => continue,
        };

        let ready_replicas = status.ready_replicas.unwrap_or(0);

        // if number of replicas > 0: it is not already disabled
        // ready_replicas == 0: there is something in progress (rolling restart...) so we should not touch it
        if status.replicas > 0 && ready_replicas == 0 {
            kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!(
                    "Statefulset {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                    sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
                )),
            ));
            block_on(kube_client.set_statefulset_replicas_number(
                event_details.clone(),
                sts.metadata.name.as_str(),
                sts.metadata.namespace.as_str(),
                0,
            ))?;
        } else {
            info!(
                "Statefulset {}/{} has {}/{} replicas ready. No action needed.",
                sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
            );
        }
    }

    if let Err(e) = delete_crashlooping_pods(
        kubernetes,
        None,
        None,
        Some(3),
        infra_ctx.cloud_provider().credentials_environment_variables(),
        Infrastructure(InfrastructureStep::Upgrade),
    ) {
        kubernetes.logger().log(EngineEvent::Error(*e.clone(), None));
        return Err(e);
    }

    if let Err(e) = delete_completed_jobs(
        kubernetes,
        infra_ctx.cloud_provider().credentials_environment_variables(),
        Infrastructure(InfrastructureStep::Upgrade),
        None,
    ) {
        kubernetes.logger().log(EngineEvent::Error(*e.clone(), None));
        return Err(e);
    }

    if !infra_ctx.kubernetes().is_karpenter_enabled() {
        // Disable cluster autoscaler deployment and be sure we re-enable it on exist
        let ev = event_details.clone();
        let _guard = scopeguard::guard(
            set_cluster_autoscaler_replicas(kubernetes, event_details.clone(), 0, infra_ctx)?,
            |_| {
                let _ = set_cluster_autoscaler_replicas(kubernetes, ev, 1, infra_ctx);
            },
        );
    }

    terraform_init_validate_plan_apply(
        temp_dir.to_string_lossy().as_ref(),
        kubernetes.context.is_dry_run_deploy(),
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
        &TerraformValidators::Default,
    )
    .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

    check_workers_on_upgrade(
        kubernetes,
        infra_ctx.cloud_provider(),
        kubernetes_upgrade_status.requested_version.to_string(),
        match kubernetes.is_karpenter_enabled() {
            true => Some("eks.amazonaws.com/compute-type!=fargate"),
            false => None,
        },
    )
    .map_err(|e| EngineError::new_k8s_node_not_ready(event_details.clone(), e))?;

    kubernetes.logger().log(EngineEvent::Info(
        event_details,
        EventMessage::new_from_safe("Kubernetes nodes have been successfully upgraded".to_string()),
    ));

    Ok(())
}

fn set_cluster_autoscaler_replicas(
    kubernetes: &EKS,
    event_details: EventDetails,
    replicas_count: u32,
    infra_ctx: &InfrastructureContext,
) -> Result<(), Box<EngineError>> {
    let autoscaler_new_state = match replicas_count {
        0 => "disable",
        _ => "enable",
    };
    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Set cluster autoscaler to: `{autoscaler_new_state}`.")),
    ));
    let selector = "cluster-autoscaler-aws-cluster-autoscaler";
    let namespace = "kube-system";
    kubectl_exec_scale_replicas(
        kubernetes.kubeconfig_local_file_path(),
        infra_ctx.cloud_provider().credentials_environment_variables(),
        namespace,
        ScalingKind::Deployment,
        selector,
        replicas_count,
    )
    .map_err(|e| {
        Box::new(EngineError::new_k8s_scale_replicas(
            event_details.clone(),
            selector.to_string(),
            namespace.to_string(),
            replicas_count,
            e,
        ))
    })?;

    Ok(())
}
