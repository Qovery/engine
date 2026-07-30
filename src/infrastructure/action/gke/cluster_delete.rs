use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventMessage, InfrastructureStep};
use crate::infrastructure::action::cluster_outputs_helper::update_cluster_outputs;
use crate::infrastructure::action::delete_kube_apps::{delete_all_pdbs, delete_kube_apps};
use crate::infrastructure::action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure::action::gke::GkeQoveryTerraformOutput;
use crate::infrastructure::action::{InfraLogger, ToInfraTeraContext};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::gcp::{Gke, VpcMode};
use crate::infrastructure::models::object_storage::ObjectStorage;
use crate::runtime::block_on;
use crate::utilities::envs_to_string;
use google_cloud_lro::Poller as _;
use scopeguard::guard;
use std::collections::HashSet;

pub(super) fn delete_gke_cluster(
    cluster: &Gke,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Delete));

    logger.info("Preparing to delete cluster.");
    let temp_dir = cluster.temp_dir();

    // should apply before destroy to be sure destroy will compute on all resources (skipped on skipReconcile)
    let gcp_access_token_file_path = temp_dir.join("gcp-access-token");
    let tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    let tf_resources = TerraformInfraResources::new(
        tera_context.clone(),
        cluster.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        envs_to_string(infra_ctx.cloud_provider().credentials_environment_variables()),
        cluster.context().is_dry_run_deploy(),
    );
    // Best-effort reconcile: apply may fail and the state may no longer hold deserializable outputs
    // once the cluster is gone. None => skip the cleanup steps that need a reachable cluster.
    // skipReconcile: skip the (hang/credential-prone) Terraform apply, but still init + read existing
    // outputs so in-cluster cleanup can run when the cluster is reachable; only skip cleanup when outputs
    // can't be read (cluster gone).
    // Guard hoisted so the gcloud access-token file is cleaned up whether we apply or only read outputs.
    let _remove_access_token_file = guard(gcp_access_token_file_path, |path| {
        let _ = std::fs::remove_file(path);
    });
    let qovery_terraform_output: Option<GkeQoveryTerraformOutput> = if infra_ctx.context().is_skip_reconcile() {
        logger.info("Skip reconcile requested: skipping the pre-destroy Terraform apply; reading existing outputs so in-cluster cleanup can still run if the cluster is reachable.");
        tf_resources.init_and_read_output().ok()
    } else {
        logger.info(format!(
            "Ensuring everything is up to date before deleting cluster {}/{}",
            cluster.name(),
            cluster.short_id()
        ));
        logger.info("Running Terraform apply before running a delete.");
        tf_resources.create_or_read_output(&logger)
    };

    // kubeconfig + in-cluster cleanup only make sense when we have outputs (cluster still reachable).
    if let Some(output) = &qovery_terraform_output {
        // Best-effort under skipReconcile: attempt the in-cluster cleanup, but never let its failure
        // block the teardown below — force-delete must always proceed to destroy.
        let cleanup = (|| -> Result<(), Box<EngineError>> {
            update_cluster_outputs(cluster, output)?;

            // Configure kubectl to be able to connect to cluster
            let _ = cluster.configure_gcloud_for_cluster(infra_ctx); // TODO(ENG-1802): properly handle this error
            cluster.write_runtime_kubeconfig_with_access_token_if_needed()?;

            // delete all PDBs first, because those will prevent node deletion
            if let Err(_errors) = delete_all_pdbs(infra_ctx, event_details.clone(), &logger) {
                logger.warn("Cannot delete all PDBs, this is not blocking cluster deletion.");
            }

            delete_kube_apps(cluster, infra_ctx, event_details.clone(), &logger, HashSet::with_capacity(0))?;
            Ok(())
        })();
        if let Err(e) = cleanup {
            if infra_ctx.context().is_skip_reconcile() {
                logger.warn(format!(
                    "Skip reconcile: in-cluster cleanup failed; continuing to cluster teardown: {e}"
                ));
            } else {
                return Err(e);
            }
        }
    } else {
        logger.warn("Skipping in-cluster cleanup (PDBs, apps): no Terraform outputs, cluster likely already deleted.");
    }

    // GKE's in-cluster cloud-controller creates `k8s-*-node-http-hc` firewall rules (LoadBalancer /
    // node health-check) that Terraform does not track. They can linger after Service deletion and
    // block the VPC deletion during `terraform destroy`. Best-effort cleanup, only for the
    // Qovery-managed VPC (Terraform deletes the network only in that case).
    delete_leftover_node_http_hc_firewall_rules(cluster, &logger);

    logger.info(format!("Deleting Kubernetes cluster {}/{}", cluster.name(), cluster.short_id()));
    tf_resources.delete(&[], &logger)?;

    delete_object_storage(cluster, &logger)?;
    logger.info("Kubernetes cluster deleted successfully.");
    Ok(())
}

fn delete_object_storage(cluster: &Gke, logger: &impl InfraLogger) -> Result<(), Box<EngineError>> {
    // Because cluster logs buckets can be sometimes very beefy, we delete them in a non-blocking way via a GCP job.
    if let Err(e) = cluster
        .object_storage
        .delete_bucket_non_blocking(&cluster.logs_bucket_name())
    {
        logger.warn(EventMessage::new(
            format!("Cannot delete cluster logs object storage `{}`", cluster.logs_bucket_name()),
            Some(e.to_string()),
        ));
    }

    if let Err(e) = cluster
        .object_storage
        .delete_bucket_non_blocking(&cluster.prometheus_bucket_name())
    {
        logger.warn(EventMessage::new(
            format!(
                "Cannot delete cluster logs object storage `{}`",
                cluster.prometheus_bucket_name()
            ),
            Some(e.to_string()),
        ));
    }

    Ok(())
}

/// Best-effort removal of the GKE-managed `k8s-*-node-http-hc` firewall rules that Terraform does
/// not track and that can block the VPC deletion during `terraform destroy`.
///
/// Only runs when Qovery manages the VPC (`VpcMode::Automatic`). With a user-provided VPC the
/// network is never deleted by Terraform (it is a `data` source), so there is nothing to unblock,
/// and the network may be shared with other clusters — we must not touch it.
fn delete_leftover_node_http_hc_firewall_rules(cluster: &Gke, logger: &impl InfraLogger) {
    if !matches!(cluster.options.vpc_mode, VpcMode::Automatic { .. }) {
        return;
    }

    let network = cluster.cluster_name();
    let project_id = cluster.credentials.project_id();

    let firewalls = match super::firewalls_client(cluster) {
        Ok(c) => c,
        Err(e) => {
            logger.warn(format!(
                "Cannot create GCP Firewalls API client to clean up leftover firewall rules, skipping: {e}"
            ));
            return;
        }
    };

    // List all project firewalls and filter client-side: the Compute REST API filter syntax
    // differs from the gcloud CLI and rejects `~` (regex) as an invalid operator.
    let network_suffix = format!("/networks/{network}");

    let rule_names: Vec<String> = match block_on(async { firewalls.list().set_project(project_id).send().await }) {
        Ok(response) => response
            .items
            .into_iter()
            .filter(|r| {
                r.name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("k8s-") && n.ends_with("-node-http-hc"))
                    && r.network.as_deref().is_some_and(|n| n.ends_with(&network_suffix))
            })
            .filter_map(|r| r.name)
            .collect(),
        Err(e) => {
            logger.warn(format!(
                "Cannot list leftover Kubernetes firewall rules for network `{network}`, skipping cleanup: {e}"
            ));
            return;
        }
    };

    if rule_names.is_empty() {
        return;
    }

    logger.info(format!(
        "Deleting {} leftover Kubernetes firewall rule(s) blocking VPC deletion: {}",
        rule_names.len(),
        rule_names.join(", ")
    ));

    for name in &rule_names {
        if let Err(e) = block_on(async {
            firewalls
                .delete()
                .set_project(project_id)
                .set_firewall(name)
                .poller()
                .until_done()
                .await
        }) {
            logger.warn(format!(
                "Cannot delete leftover Kubernetes firewall rule `{name}`, terraform destroy may fail: {e}"
            ));
        }
    }
}
