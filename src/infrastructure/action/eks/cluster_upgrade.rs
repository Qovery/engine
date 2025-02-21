use crate::cmd::kubectl::{ScalingKind, kubectl_exec_scale_replicas};
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, InfrastructureStep};
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::delete_kube_apps::prepare_kube_upgrade;
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::eks::AwsEksQoveryTerraformOutput;
use crate::infrastructure::action::eks::nodegroup::should_update_desired_nodes;
use crate::infrastructure::action::eks::tera_context::eks_tera_context;
use crate::infrastructure::action::eks::utils::{define_cluster_upgrade_timeout, get_rusoto_eks_client};
use crate::infrastructure::action::kubectl_utils::check_workers_on_upgrade;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesUpgradeStatus};
use crate::io_models::models::KubernetesClusterAction;
use crate::runtime::block_on;
use crate::services::kube_client::SelectK8sResourceBy;
use crate::utilities::envs_to_string;
use std::path::PathBuf;

pub fn upgrade_eks_cluster(
    kubernetes: &EKS,
    infra_ctx: &InfrastructureContext,
    kubernetes_upgrade_status: KubernetesUpgradeStatus,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Infrastructure(InfrastructureStep::Upgrade));

    logger.info("Start preparing EKS cluster upgrade process");

    let temp_dir = kubernetes.temp_dir();
    let aws_eks_client = get_rusoto_eks_client(event_details.clone(), kubernetes, infra_ctx.cloud_provider()).ok();

    let nodes_groups = should_update_desired_nodes(
        event_details.clone(),
        kubernetes,
        KubernetesClusterAction::Upgrade(None),
        &kubernetes.nodes_groups,
        aws_eks_client,
    )?;

    let kube_client = infra_ctx.mk_kube_client()?;
    let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
        .unwrap_or(Vec::with_capacity(0));

    let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Upgrade(None));
    let cluster_upgrade_timeout_in_min = timeout;
    if let Some(x) = message {
        logger.info(x);
    }

    // generate terraform files and copy them into temp dir
    let mut context = eks_tera_context(
        kubernetes,
        infra_ctx.cloud_provider(),
        infra_ctx.dns_provider(),
        &kubernetes.zones,
        &nodes_groups,
        &kubernetes.options,
        cluster_upgrade_timeout_in_min,
        false,
        &kubernetes.advanced_settings,
        kubernetes.qovery_allowed_public_access_cidrs.as_ref(),
    )?;

    //
    // Upgrade master nodes
    //
    logger.info("Start upgrading process for master nodes.");

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

    logger.info("Upgrading Kubernetes master nodes.");
    let tf_resources = TerraformInfraResources::new(
        context.clone(),
        PathBuf::from(&kubernetes.template_directory).join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        infra_ctx.context().is_dry_run_deploy(),
    );
    let _: AwsEksQoveryTerraformOutput = tf_resources.create(&logger)?;

    //
    // Upgrade worker nodes
    //
    // disable cluster autoscaler to avoid interfering with AWS upgrade procedure
    logger.info("Preparing workers nodes for upgrade for Kubernetes cluster.");
    context.insert("enable_cluster_autoscaler", &false);
    context.insert(
        "eks_workers_version",
        format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
    );
    let tf_resources = TerraformInfraResources::new(
        context.clone(),
        PathBuf::from(&kubernetes.template_directory).join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        infra_ctx.context().is_dry_run_deploy(),
    );

    logger.info("Start upgrading process for worker nodes.");
    logger.info("Checking clusters content health");
    // disable all replicas with issues to avoid upgrade failures
    prepare_kube_upgrade(kubernetes as &dyn Kubernetes, infra_ctx, event_details.clone(), &logger)?;

    if !infra_ctx.kubernetes().is_karpenter_enabled() {
        // Disable cluster autoscaler deployment and be sure we re-enable it on exist
        let ev = event_details.clone();
        let _guard = scopeguard::guard(
            set_cluster_autoscaler_replicas(kubernetes, event_details.clone(), 0, infra_ctx, &logger)?,
            |_| {
                let _ = set_cluster_autoscaler_replicas(kubernetes, ev, 1, infra_ctx, &logger);
            },
        );
    }

    let _: AwsEksQoveryTerraformOutput = tf_resources.create(&logger)?;

    // In case of karpenter, we don't need to upgrade workers, it will do it by itself
    if !infra_ctx.kubernetes().is_karpenter_enabled() {
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

        logger.info("Kubernetes worker nodes have been successfully upgraded.");
    } else {
        // Karpenter asynchronously manages worker node upgrades, meaning that when the control plane version is updated,
        // it detects the change in the AMI (Amazon Machine Image) and identifies which nodes need upgrading by marking them as "drifted".
        // Once a new AMI is detected, Karpenter marks the existing version as drifted and starts upgrading it automatically.
        //
        // The amount of time that a node can be draining before it's forcibly deleted. A node begins draining when a delete call is made against it,
        // starting its finalization flow.
        // Pods with TerminationGracePeriodSeconds will be deleted preemptively before this terminationGracePeriod ends to give as much time to cleanup as possible.
        // If pod's terminationGracePeriodSeconds is larger than this terminationGracePeriod, Karpenter may forcibly delete the pod before it has its full terminationGracePeriod to cleanup.
        // Note: changing this value in the nodepool will drift the nodeclaims.
        // `terminationGracePeriod: 48h`
        logger.info("Kubernetes nodes will be upgraded by karpenter.")
    }

    Ok(())
}

fn set_cluster_autoscaler_replicas(
    kubernetes: &EKS,
    event_details: EventDetails,
    replicas_count: u32,
    infra_ctx: &InfrastructureContext,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let autoscaler_new_state = match replicas_count {
        0 => "disable",
        _ => "enable",
    };
    logger.info(format!("Set cluster autoscaler to: `{autoscaler_new_state}`."));
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
