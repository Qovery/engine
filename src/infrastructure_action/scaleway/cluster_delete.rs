use crate::cloud_provider::helm::ChartInfo;
use crate::cloud_provider::kubernetes::{uninstall_cert_manager, Kubernetes};
use crate::cloud_provider::scaleway::kubernetes::Kapsule;
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::cmd::kubectl::{kubectl_exec_delete_namespace, kubectl_exec_get_all_namespaces};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::engine::InfrastructureContext;
use crate::errors::{EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EventMessage, InfrastructureStep};
use crate::infrastructure_action::deploy_terraform::TerraformInfraResources;
use crate::infrastructure_action::scaleway::ScalewayQoveryTerraformOutput;
use crate::infrastructure_action::{InfraLogger, ToInfraTeraContext};
use crate::secret_manager;
use crate::secret_manager::vault::QVaultClient;

pub fn delete_kapsule_cluster(
    cluster: &Kapsule,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Delete));
    let skip_kubernetes_step = false;

    logger.info("Preparing to delete cluster.");

    let temp_dir = cluster.temp_dir();

    // generate terraform files and copy them into temp dir
    // We re-update the cluster to be sure it is in a correct state before deleting it
    let tera_context = cluster.to_infra_tera_context(infra_ctx)?;
    let tf_resources = TerraformInfraResources::new(
        tera_context.clone(),
        cluster.template_directory.join("terraform"),
        temp_dir.join("terraform"),
        event_details.clone(),
        cluster.context().is_dry_run_deploy(),
    );

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    logger.info(format!(
        "Ensuring everything is up to date before deleting cluster {}/{}",
        cluster.name(),
        cluster.short_id()
    ));
    logger.info("Running Terraform apply before running a delete.");

    let _qovery_terraform_output: ScalewayQoveryTerraformOutput = tf_resources.create(
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    )?;

    let kubeconfig_path = cluster.kubeconfig_local_file_path();

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(&cluster.template_directory, temp_dir, &tera_context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            cluster.template_directory.to_string_lossy().to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and subdirectory) into the lib/scaleway/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", cluster.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.to_string_lossy());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        )));
    }

    if !skip_kubernetes_step {
        // should make the diff between all namespaces and qovery managed namespaces
        let message = format!(
            "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
            cluster.name(),
            cluster.short_id()
        );
        logger.info(message);

        let all_namespaces = kubectl_exec_get_all_namespaces(
            &kubeconfig_path,
            infra_ctx.cloud_provider().credentials_environment_variables(),
        );

        match all_namespaces {
            Ok(namespace_vec) => {
                let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                logger.info("Deleting non Qovery namespaces");
                for namespace_to_delete in namespaces_to_delete.iter() {
                    match kubectl_exec_delete_namespace(
                        &kubeconfig_path,
                        namespace_to_delete,
                        infra_ctx.cloud_provider().credentials_environment_variables(),
                    ) {
                        Ok(_) => logger.info(format!("Namespace `{}` deleted successfully.", namespace_to_delete)),
                        Err(e) if !e.message(ErrorMessageVerbosity::FullDetails).contains("not found") => {
                            logger.warn(format!("Can't delete the namespace `{}`", namespace_to_delete));
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                let message_safe = format!(
                    "Error while getting all namespaces for Kubernetes cluster {}",
                    cluster.name_with_id(),
                );
                logger.warn(EventMessage::new(
                    message_safe,
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                ));
            }
        }

        let message = format!(
            "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
            cluster.name(),
            cluster.short_id()
        );
        logger.info(message);

        // delete custom metrics api to avoid stale namespaces on deletion
        let helm = Helm::new(
            Some(&kubeconfig_path),
            &infra_ctx.cloud_provider().credentials_environment_variables(),
        )
        .map_err(|e| to_engine_error(&event_details, e))?;
        let chart = ChartInfo::new_from_release_name("metrics-server", "kube-system");

        if let Err(e) = helm.uninstall(&chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
            // this error is not blocking
            logger.warn(EventMessage::new_from_engine_error(to_engine_error(&event_details, e)));
        }

        // required to avoid namespace stuck on deletion
        if let Err(e) = uninstall_cert_manager(
            &kubeconfig_path,
            infra_ctx.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            cluster.logger(),
        ) {
            // this error is not blocking, logging a warning and move on
            logger.warn(EventMessage::new(
                "An error occurred while trying to uninstall cert-manager. This is not blocking.".to_string(),
                Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
            ));
        }

        logger.info("Deleting Qovery managed elements");
        let qovery_namespaces = get_qovery_managed_namespaces();
        for qovery_namespace in qovery_namespaces.iter() {
            let charts_to_delete = helm
                .list_release(Some(qovery_namespace), &[])
                .map_err(|e| to_engine_error(&event_details, e))?;

            for chart in charts_to_delete {
                let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                    Ok(_) => logger.info(format!("Chart `{}` deleted", chart.name)),
                    Err(e) => {
                        let message_safe = format!("Can't delete chart `{}`", chart.name);
                        logger.warn(EventMessage::new(message_safe, Some(e.to_string())));
                    }
                }
            }
        }

        logger.info("Deleting Qovery managed namespaces");
        for qovery_namespace in qovery_namespaces.iter() {
            let deletion = kubectl_exec_delete_namespace(
                &kubeconfig_path,
                qovery_namespace,
                infra_ctx.cloud_provider().credentials_environment_variables(),
            );
            match deletion {
                Ok(_) => logger.info(format!("Namespace `{}` is fully deleted.", qovery_namespace)),
                Err(e) if !e.message(ErrorMessageVerbosity::FullDetails).contains("not found") => {
                    logger.warn(format!("Can't delete the namespace `{}`", qovery_namespace));
                }
                _ => {}
            }
        }

        logger.info("Deleting all remaining deployed helm applications");
        match helm.list_release(None, &[]) {
            Ok(helm_charts) => {
                for chart in helm_charts {
                    let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                    match helm.uninstall(&chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {}) {
                        Ok(_) => logger.info(format!("Chart `{}` deleted", chart.name)),
                        Err(e) => {
                            let message_safe = format!("Error deleting chart `{}`", chart.name);
                            logger.warn(EventMessage::new(message_safe, Some(e.to_string())));
                        }
                    }
                }
            }
            Err(e) => {
                logger.warn(EventMessage::new("Unable to get helm list".to_string(), Some(e.to_string())));
            }
        }
    };

    logger.info(format!("Deleting Kubernetes cluster {}/{}", cluster.name(), cluster.short_id()));
    logger.info("Running Terraform destroy");
    tf_resources.delete(
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    )?;

    // delete info on vault
    let vault_conn = QVaultClient::new(event_details.clone());
    if let Ok(vault_conn) = vault_conn {
        let mount = secret_manager::vault::get_vault_mount_name(cluster.context().is_test_cluster());

        // ignore on failure
        let _ = vault_conn.delete_secret(mount.as_str(), cluster.long_id().to_string().as_str());
    };

    logger.info("Kubernetes cluster successfully deleted");
    Ok(())
}
