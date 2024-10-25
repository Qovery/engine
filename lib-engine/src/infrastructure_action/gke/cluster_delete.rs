use crate::cloud_provider::gcp::kubernetes::{Gke, GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES};
use crate::cloud_provider::helm::ChartInfo;
use crate::cloud_provider::kubernetes::{uninstall_cert_manager, Kubernetes};
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::Helm;
use crate::cmd::kubectl::{kubectl_exec_delete_namespace, kubectl_exec_get_all_namespaces};
use crate::cmd::terraform::{terraform_init_validate_destroy, terraform_init_validate_plan_apply};
use crate::cmd::terraform_validators::TerraformValidators;
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::engine::InfrastructureContext;
use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventMessage, InfrastructureStep};
use crate::infrastructure_action::gke::tera_context::gke_tera_context;
use crate::object_storage::{BucketDeleteStrategy, ObjectStorage};
use crate::secret_manager;
use crate::secret_manager::vault::QVaultClient;

pub(super) fn delete_gke_cluster(cluster: &Gke, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Delete));
    let skip_kubernetes_step = false;

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing to delete cluster.".to_string()),
    ));

    let temp_dir = cluster.temp_dir();

    // generate terraform files and copy them into temp dir
    let context = gke_tera_context(cluster, infra_ctx)?;

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(cluster.template_directory.as_str(), temp_dir, context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            cluster.template_directory.to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/gcp/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/gcp/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", cluster.context.lib_root_dir());
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
        cluster.name(),
        cluster.short_id()
    );
    cluster
        .logger()
        .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform apply before running a delete.".to_string()),
    ));

    if let Err(e) = terraform_init_validate_plan_apply(
        temp_dir.to_string_lossy().as_ref(),
        false,
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
        &TerraformValidators::None,
    ) {
        // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
        cluster.logger().log(EngineEvent::Error(
            EngineError::new_terraform_error(event_details.clone(), e),
            None,
        ));
    };

    let kubeconfig_path = cluster.kubeconfig_local_file_path();
    if !skip_kubernetes_step {
        // Configure kubectl to be able to connect to cluster
        let _ = cluster.configure_gcloud_for_cluster(infra_ctx); // TODO(ENG-1802): properly handle this error

        // should make the diff between all namespaces and qovery managed namespaces
        let message = format!(
            "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
            cluster.name(),
            cluster.short_id()
        );
        cluster
            .logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        let all_namespaces = kubectl_exec_get_all_namespaces(
            &kubeconfig_path,
            infra_ctx.cloud_provider().credentials_environment_variables(),
        );

        match all_namespaces {
            Ok(namespace_vec) => {
                let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                cluster.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Deleting non Qovery namespaces".to_string()),
                ));

                for namespace_to_delete in namespaces_to_delete
                    .into_iter()
                    .filter(|ns| !(*GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES).contains(ns))
                {
                    match kubectl_exec_delete_namespace(
                        &kubeconfig_path,
                        namespace_to_delete,
                        infra_ctx.cloud_provider().credentials_environment_variables(),
                    ) {
                        Ok(_) => cluster.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!(
                                "Namespace `{namespace_to_delete}` deleted successfully."
                            )),
                        )),
                        Err(e) => {
                            if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                                cluster.logger().log(EngineEvent::Warning(
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
                    cluster.name_with_id(),
                );
                cluster.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(message_safe, Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars))),
                ));
            }
        }

        let message = format!(
            "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
            cluster.name(),
            cluster.short_id()
        );
        cluster
            .logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        let helm = Helm::new(
            Some(&kubeconfig_path),
            &infra_ctx.cloud_provider().credentials_environment_variables(),
        )
        .map_err(|e| EngineError::new_helm_error(event_details.clone(), e))?;

        // required to avoid namespace stuck on deletion
        if let Err(e) = uninstall_cert_manager(
            &kubeconfig_path,
            infra_ctx.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            cluster.logger(),
        ) {
            // this error is not blocking, logging a warning and move on
            cluster.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(
                    "An error occurred while trying to uninstall cert-manager. This is not blocking.".to_string(),
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                ),
            ));
        }

        cluster.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deleting Qovery managed helm charts".to_string()),
        ));

        let qovery_namespaces = get_qovery_managed_namespaces();
        for qovery_namespace in qovery_namespaces.iter() {
            let charts_to_delete = helm
                .list_release(Some(qovery_namespace), &[])
                .map_err(|e| EngineError::new_helm_error(event_details.clone(), e.clone()))?;

            for chart in charts_to_delete {
                let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                    Ok(_) => cluster.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                    )),
                    Err(e) => {
                        let message_safe = format!("Can't delete chart `{}`", chart.name);
                        cluster.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new(message_safe, Some(e.to_string())),
                        ))
                    }
                }
            }
        }

        cluster.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deleting Qovery managed namespaces".to_string()),
        ));

        for qovery_namespace in qovery_namespaces.iter() {
            let deletion = kubectl_exec_delete_namespace(
                &kubeconfig_path,
                qovery_namespace,
                infra_ctx.cloud_provider().credentials_environment_variables(),
            );
            match deletion {
                Ok(_) => cluster.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!("Namespace {qovery_namespace} is fully deleted")),
                )),
                Err(e) => {
                    if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                        cluster.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Can't delete namespace {qovery_namespace}.")),
                        ))
                    }
                }
            }
        }

        cluster.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Delete all remaining deployed helm applications".to_string()),
        ));

        match helm.list_release(None, &[]) {
            Ok(helm_charts) => {
                for chart in helm_charts {
                    let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                    match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                        Ok(_) => cluster.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                        )),
                        Err(e) => {
                            let message_safe = format!("Error deleting chart `{}`", chart.name);
                            cluster.logger().log(EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new(message_safe, Some(e.to_string())),
                            ))
                        }
                    }
                }
            }
            Err(e) => {
                let message_safe = "Unable to get helm list";
                cluster.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(message_safe.to_string(), Some(e.to_string())),
                ))
            }
        }
    };

    let message = format!("Deleting Kubernetes cluster {}/{}", cluster.name(), cluster.short_id());
    cluster
        .logger()
        .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform destroy".to_string()),
    ));

    if let Err(err) = terraform_init_validate_destroy(
        temp_dir.to_string_lossy().as_ref(),
        false,
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
        &TerraformValidators::None,
    ) {
        return Err(Box::new(EngineError::new_terraform_error(event_details, err)));
    }

    // delete info on vault
    let vault_conn = QVaultClient::new(event_details.clone());
    if let Ok(vault_conn) = vault_conn {
        let mount = secret_manager::vault::get_vault_mount_name(cluster.context().is_test_cluster());

        // ignore on failure
        if let Err(e) = vault_conn.delete_secret(mount.as_str(), cluster.long_id().to_string().as_str()) {
            cluster.logger.log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new("Cannot delete cluster config from Vault".to_string(), Some(e.to_string())),
            ))
        }
    }

    // delete object storages
    if let Err(e) = cluster
        .object_storage
        .delete_bucket(&cluster.kubeconfig_bucket_name(), BucketDeleteStrategy::HardDelete)
    {
        cluster.logger.log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new(
                format!(
                    "Cannot delete cluster kubeconfig object storage `{}`",
                    &cluster.kubeconfig_bucket_name()
                ),
                Some(e.to_string()),
            ),
        ))
    }
    // Because cluster logs buckets can be sometimes very beefy, we delete them in a non-blocking way via a GCP job.
    if let Err(e) = cluster
        .object_storage
        .delete_bucket_non_blocking(&cluster.logs_bucket_name())
    {
        cluster.logger.log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new(
                format!("Cannot delete cluster logs object storage `{}`", &cluster.logs_bucket_name()),
                Some(e.to_string()),
            ),
        ))
    }

    cluster.logger().log(EngineEvent::Info(
        event_details,
        EventMessage::new_from_safe("Kubernetes cluster successfully deleted".to_string()),
    ));

    Ok(())
}
