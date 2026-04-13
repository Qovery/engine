use super::EksAnywhereRunMode;
use super::cluster_config_git::prepare_eks_anywhere_cluster_config;
use super::cluster_install::install_eks_anywhere_charts;
use super::eksctl::{run_eks_anywhere_upgrade_cluster, run_eks_anywhere_upgrade_plan};
use super::etcd_backup::{run_eks_anywhere_cluster_backup, upload_eks_anywhere_capi_backup};
use super::provider::{
    EksAnywhereProviderMode, detect_provider_mode_from_cluster_config, run_provider_preflight_for_mode,
};
use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::errors::{CommandError, EngineError};
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhere;
use std::path::Path;

pub(super) fn create_eks_anywhere_cluster(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    if let Err(error) = validate_supported_cluster_deployment_mode(cluster, infra_ctx) {
        logger.error((*error).clone(), None::<&str>);
        return Err(error);
    }

    let run_mode = EksAnywhereRunMode::from_context(infra_ctx);

    if let Some(cluster_config_path) = prepare_eks_anywhere_cluster_config(cluster, &logger)?.as_ref() {
        log_section_title(&logger, "🧪", "EKS Anywhere preflight");
        logger.info(format!("Execution mode: {}.", run_mode.label()));
        log_command_version(&logger, "eksctl", &["version"]);
        log_command_version(&logger, "eksctl anywhere", &["anywhere", "version"]);
        run_cluster_config_workflow(cluster, infra_ctx, cluster_config_path, run_mode, &logger)?;
    }

    install_eks_anywhere_charts(cluster, infra_ctx, logger)
}

fn run_cluster_config_workflow(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    cluster_config_path: &Path,
    run_mode: EksAnywhereRunMode,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let provider_mode = detect_provider_mode(cluster_config_path, logger);
    // Always run the upgrade plan (dry-run): its output is required to extract
    // the expected eksdRelease tag passed to the vSphere preflight.
    let upgrade_plan_summary =
        run_eks_anywhere_upgrade_plan(cluster, cluster_config_path, infra_ctx.cloud_provider(), logger)?;

    // TODO(eks-anywhere): temporary guard requested for validation.
    // Remove this early-exit once kube version-change workflow is finalized.
    if upgrade_plan_summary.has_kubernetes_major_or_minor_change() {
        if let Some((current, next)) = upgrade_plan_summary.kubernetes_version_transition.as_ref() {
            logger.warn(format!(
                "Temporary behavior: kubernetes major/minor change detected in upgrade plan (`{current}` -> `{next}`). Stopping workflow early."
            ));
        } else {
            logger.warn(
                "Temporary behavior: kubernetes major/minor change detected in upgrade plan. Stopping workflow early.",
            );
        }
        return Ok(());
    }
    run_provider_preflight_stage(
        cluster,
        infra_ctx,
        provider_mode,
        cluster_config_path,
        run_mode,
        upgrade_plan_summary.expected_eksd_release_tag.as_deref(),
        logger,
    )?;

    if run_mode.should_execute_upgrade_cluster() {
        run_eks_anywhere_cluster_backup(cluster, infra_ctx, cluster_config_path, logger)
            .map_err(|error| map_cluster_backup_error(cluster, error))?;
        if let Err(upgrade_error) =
            run_eks_anywhere_upgrade_cluster(cluster, cluster_config_path, infra_ctx.cloud_provider(), logger)
        {
            // Best-effort: even when upgrade fails, try uploading CAPI backup artifacts if they were generated.
            match upload_eks_anywhere_capi_backup(cluster, cluster_config_path, logger) {
                Ok(()) => logger.warn("Upgrade failed, but best-effort CAPI backup upload succeeded."),
                Err(error) => logger.warn(format!(
                    "Upgrade failed and best-effort CAPI backup upload also failed: {}",
                    error.message_safe()
                )),
            }

            return Err(upgrade_error);
        }

        upload_eks_anywhere_capi_backup(cluster, cluster_config_path, logger)
            .map_err(|error| map_capi_backup_upload_error(cluster, error))?;
    } else {
        logger.info("Dry-run mode: skipping EKS Anywhere backup and upgrade execution.");
    }

    Ok(())
}

fn detect_provider_mode(cluster_config_path: &Path, logger: &impl InfraLogger) -> EksAnywhereProviderMode {
    let provider_mode = match detect_provider_mode_from_cluster_config(cluster_config_path) {
        Ok(provider_mode) => provider_mode,
        Err(err) => {
            logger.warn(format!(
                "Unable to detect EKS Anywhere provider mode from cluster config, defaulting to generic mode: {}",
                err
            ));
            EksAnywhereProviderMode::Unknown
        }
    };
    log_provider_mode(logger, provider_mode);
    provider_mode
}

fn run_provider_preflight_stage(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    provider_mode: EksAnywhereProviderMode,
    cluster_config_path: &Path,
    run_mode: EksAnywhereRunMode,
    expected_eksd_release_tag: Option<&str>,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    run_provider_preflight_for_mode(
        provider_mode,
        cluster_config_path,
        infra_ctx.cloud_provider(),
        run_mode.install_missing_templates(),
        expected_eksd_release_tag,
        logger,
    )
    .map_err(|error| map_provider_preflight_error(cluster, provider_mode, error))
}

fn log_provider_mode(logger: &impl InfraLogger, provider_mode: EksAnywhereProviderMode) {
    match provider_mode {
        EksAnywhereProviderMode::VSphere => logger.info("🧩 Provider mode: vSphere."),
        EksAnywhereProviderMode::Unknown => logger.info("🧩 Provider mode: generic."),
    }
}

fn map_provider_preflight_error(
    cluster: &EksAnywhere,
    provider_mode: EksAnywhereProviderMode,
    error: CommandError,
) -> Box<EngineError> {
    let message = match provider_mode {
        EksAnywhereProviderMode::VSphere => "vSphere preflight checks failed",
        EksAnywhereProviderMode::Unknown => "Provider preflight checks failed",
    };

    Box::new(EngineError::new_unknown(
        cluster.get_event_details(Infrastructure(InfrastructureStep::CreateError)),
        message.to_string(),
        Some(error),
        None,
        None,
    ))
}

fn map_cluster_backup_error(cluster: &EksAnywhere, error: CommandError) -> Box<EngineError> {
    Box::new(EngineError::new_unknown(
        cluster.get_event_details(Infrastructure(InfrastructureStep::CreateError)),
        "EKS Anywhere cluster backup failed".to_string(),
        Some(error),
        None,
        None,
    ))
}

fn map_capi_backup_upload_error(cluster: &EksAnywhere, error: CommandError) -> Box<EngineError> {
    Box::new(EngineError::new_unknown(
        cluster.get_event_details(Infrastructure(InfrastructureStep::CreateError)),
        "EKS Anywhere CAPI backup upload failed".to_string(),
        Some(error),
        None,
        None,
    ))
}

fn validate_supported_cluster_deployment_mode(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
) -> Result<(), Box<EngineError>> {
    if !infra_ctx.context().is_first_cluster_deployment() {
        return Ok(());
    }

    Err(Box::new(EngineError::new_unknown(
        cluster.get_event_details(Infrastructure(InfrastructureStep::CreateError)),
        "Cluster creation is not supported on first install for EKS Anywhere".to_string(),
        Some(CommandError::new_from_safe_message(
            "first cluster deployment is not supported for EKS Anywhere".to_string(),
        )),
        None,
        None,
    )))
}

fn log_command_version(logger: &impl InfraLogger, binary_display_name: &str, args: &[&str]) {
    let mut version_lines = Vec::new();
    let mut cmd = QoveryCommand::new("eksctl", args, &[]);

    if cmd
        .exec_with_output(&mut |line| version_lines.push(line), &mut |line| {
            warn!("Error while getting `{}` version: {}", binary_display_name, line)
        })
        .is_err()
        || version_lines.iter().all(|line| line.trim().is_empty())
    {
        logger.warn(format!(
            "Unable to get `{}` version using `eksctl {}`.",
            binary_display_name,
            args.join(" ")
        ));
        return;
    }

    logger.info(format!(
        "Using {}: {}",
        binary_display_name,
        format_version_for_user(binary_display_name, &version_lines)
    ));
}

fn format_version_for_user(binary_display_name: &str, version_lines: &[String]) -> String {
    let non_empty_lines: Vec<&str> = version_lines
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if non_empty_lines.is_empty() {
        return "<unknown>".to_string();
    }

    if binary_display_name == "eksctl anywhere"
        && let Some(version_line) = non_empty_lines
            .iter()
            .find(|line| line.to_ascii_lowercase().starts_with("version:"))
    {
        return version_line.to_string();
    }

    non_empty_lines.join(" ")
}

fn log_section_title(logger: &impl InfraLogger, icon: &str, title: &str) {
    logger.info("");
    logger.info(format!("***** {icon} {title} *****"));
    logger.info("");
}
