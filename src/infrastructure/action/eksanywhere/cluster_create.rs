use super::EksAnywhereRunMode;
use super::cluster_config_git::prepare_eks_anywhere_cluster_config;
use super::cluster_install::install_eks_anywhere_charts;
use super::eksctl::{run_eks_anywhere_upgrade_cluster, run_eks_anywhere_upgrade_plan};
use super::etcd_backup::{run_eks_anywhere_cluster_backup, upload_eks_anywhere_capi_backup};
use super::provider::{
    EksAnywhereProviderMode, ParsedEksAnywhereClusterConfig, parse_eks_anywhere_cluster_config,
    run_provider_preflight_for_mode,
};
use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::environment::models::types::VersionsNumber;
use crate::errors::{CommandError, EngineError};
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhere;
use crate::services::kubernetes_api_deprecation_service::KubernetesApiDeprecationServiceGranuality;
use std::path::Path;

pub(super) fn create_eks_anywhere_cluster(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
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
    let parsed_cluster_config = parse_cluster_config(cluster_config_path, logger);
    let provider_mode = parsed_cluster_config.provider_mode();
    log_provider_mode(logger, provider_mode);
    // Always run the upgrade plan (dry-run): its output is required to extract
    // the expected eksdRelease tag passed to the vSphere preflight.
    let upgrade_plan_summary =
        run_eks_anywhere_upgrade_plan(cluster, cluster_config_path, infra_ctx.cloud_provider(), logger)?;
    enforce_supported_kubernetes_version_jump(cluster, &upgrade_plan_summary)?;
    run_pluto_check_before_upgrade_if_needed(cluster, infra_ctx, &upgrade_plan_summary, logger)?;
    run_provider_preflight_stage(
        cluster,
        infra_ctx,
        provider_mode,
        &parsed_cluster_config,
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

fn enforce_supported_kubernetes_version_jump(
    cluster: &EksAnywhere,
    upgrade_plan_summary: &super::eksctl::EksAnywhereUpgradePlanSummary,
) -> Result<(), Box<EngineError>> {
    if !upgrade_plan_summary.has_kubernetes_major_upgrade_jump_over_one() {
        return Ok(());
    }

    let details = cluster.get_event_details(Infrastructure(InfrastructureStep::CreateError));
    let message = match upgrade_plan_summary.kubernetes_version_transition.as_ref() {
        Some((current, next)) => format!(
            "Unsupported Kubernetes upgrade path detected in upgrade plan: `{current}` -> `{next}`. \
Only upgrades with a maximum +1 major version jump are allowed."
        ),
        None => "Unsupported Kubernetes upgrade path detected in upgrade plan: major version jump is greater than one."
            .to_string(),
    };

    Err(Box::new(EngineError::new_unknown(
        details,
        "Unsupported Kubernetes major upgrade jump".to_string(),
        Some(CommandError::new_from_safe_message(message)),
        None,
        None,
    )))
}

fn run_pluto_check_before_upgrade_if_needed(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    upgrade_plan_summary: &super::eksctl::EksAnywhereUpgradePlanSummary,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    if !upgrade_plan_summary.has_kubernetes_version_change() {
        logger.info(
            "No Kubernetes upgrade detected in plan; generic deprecated API compatibility check will run after create.",
        );
        return Ok(());
    }

    let Some(target_kubernetes_version) = upgrade_plan_summary.target_kubernetes_version() else {
        let details = cluster.get_event_details(Infrastructure(InfrastructureStep::CreateError));
        return Err(Box::new(EngineError::new_unknown(
            details,
            "Cannot parse planned Kubernetes target version from upgrade plan".to_string(),
            Some(CommandError::new_from_safe_message(
                "Kubernetes upgrade detected in upgrade plan, but target version parsing failed. Cannot run deprecated API compatibility check."
                    .to_string(),
            )),
            None,
            None,
        )));
    };
    logger.info(format!(
        "Kubernetes upgrade detected in plan; running blocking deprecated API compatibility check before upgrade (target `{target_kubernetes_version}`)."
    ));

    run_pluto_compatibility_check(cluster, infra_ctx, &target_kubernetes_version, logger)?;

    Ok(())
}

fn run_pluto_compatibility_check(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    target_kubernetes_version: &VersionsNumber,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    logger.info(format!(
        "Check if cluster has no calls to deprecated kubernetes API for target version `{target_kubernetes_version}`"
    ));

    let kube_client = infra_ctx.mk_kube_client()?;
    let compatibility_check = infra_ctx
        .kubernetes_api_deprecation_service()
        .is_cluster_fully_compatible_with_kubernetes_version(
            cluster.kubeconfig_local_file_path().as_path(),
            Some(target_kubernetes_version),
            &infra_ctx.cloud_provider().credentials_environment_variables(),
            KubernetesApiDeprecationServiceGranuality::WithQoveryMetadata {
                kube_client: kube_client.as_ref(),
            },
        );

    match compatibility_check {
        Ok(_) => logger.info("Cluster has no calls to deprecated kubernetes API calls"),
        Err(e) => {
            let deprecation_error = EngineError::new_k8s_deprecated_api_calls_found_error(
                cluster.get_event_details(Infrastructure(InfrastructureStep::CreateError)),
                target_kubernetes_version,
                e,
            );
            return Err(Box::new(deprecation_error));
        }
    }

    Ok(())
}

fn parse_cluster_config(cluster_config_path: &Path, logger: &impl InfraLogger) -> ParsedEksAnywhereClusterConfig {
    match parse_eks_anywhere_cluster_config(cluster_config_path) {
        Ok(parsed_cluster_config) => parsed_cluster_config,
        Err(err) => {
            logger.warn(format!(
                "Unable to parse EKS Anywhere cluster config for provider detection/preflight, defaulting to generic mode: {}",
                err
            ));
            ParsedEksAnywhereClusterConfig::default()
        }
    }
}

fn run_provider_preflight_stage(
    cluster: &EksAnywhere,
    infra_ctx: &InfrastructureContext,
    provider_mode: EksAnywhereProviderMode,
    parsed_cluster_config: &ParsedEksAnywhereClusterConfig,
    cluster_config_path: &Path,
    run_mode: EksAnywhereRunMode,
    expected_eksd_release_tag: Option<&str>,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    run_provider_preflight_for_mode(
        provider_mode,
        parsed_cluster_config,
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
