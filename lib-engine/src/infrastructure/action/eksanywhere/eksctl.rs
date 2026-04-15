use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::environment::models::types::VersionsNumber;
use crate::errors::{CommandError, EngineError};
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::action::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhere;
use std::env;
use std::path::Path;

const COMMAND_STDOUT_PREFIX: &str = "CMD│ ";
const COMMAND_STDERR_PREFIX: &str = "CMD┃ ";
const EKSCTL_VERBOSITY_LEVEL: &str = "2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EksAnywhereUpgradePlanSummary {
    pub expected_eksd_release_tag: Option<String>,
    pub kubernetes_version_transition: Option<(String, String)>,
}

impl EksAnywhereUpgradePlanSummary {
    pub fn has_kubernetes_version_change(&self) -> bool {
        self.kubernetes_version_transition
            .as_ref()
            .is_some_and(|(current, next)| !current.eq_ignore_ascii_case(next))
    }

    pub fn has_kubernetes_major_or_minor_change(&self) -> bool {
        self.kubernetes_version_transition
            .as_ref()
            .is_some_and(|(current, next)| {
                match (
                    extract_kubernetes_major_minor(current.as_str()),
                    extract_kubernetes_major_minor(next.as_str()),
                ) {
                    (Some(current_branch), Some(next_branch)) => current_branch != next_branch,
                    _ => !current.eq_ignore_ascii_case(next),
                }
            })
    }

    pub fn kubernetes_major_upgrade_jump(&self) -> Option<u64> {
        let (current, next) = self.kubernetes_version_transition.as_ref()?;
        let (current_major, _) = extract_kubernetes_major_minor(current.as_str())?;
        let (next_major, _) = extract_kubernetes_major_minor(next.as_str())?;
        Some(next_major.saturating_sub(current_major))
    }

    pub fn has_kubernetes_major_upgrade_jump_over_one(&self) -> bool {
        self.kubernetes_major_upgrade_jump().is_some_and(|jump| jump > 1)
    }

    pub fn target_kubernetes_version(&self) -> Option<VersionsNumber> {
        let (_, next) = self.kubernetes_version_transition.as_ref()?;
        normalize_kubernetes_target_version_for_pluto(next)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EksAnywhereUpgradeCommand {
    PlanCluster,
    UpgradeCluster,
}

impl EksAnywhereUpgradeCommand {
    fn eksctl_args<'a>(&self, config_path: &'a str, kubeconfig_path: &'a str) -> Vec<&'a str> {
        match self {
            Self::PlanCluster => vec![
                "anywhere",
                "upgrade",
                "plan",
                "cluster",
                "-f",
                config_path,
                "--kubeconfig",
                kubeconfig_path,
                "-v",
                EKSCTL_VERBOSITY_LEVEL,
            ],
            Self::UpgradeCluster => vec![
                "anywhere",
                "upgrade",
                "cluster",
                "-f",
                config_path,
                "--kubeconfig",
                kubeconfig_path,
                "--no-timeouts",
                "--skip-validations=vsphere-user-privilege",
                "--skip-validations=pod-disruption",
                "-v",
                EKSCTL_VERBOSITY_LEVEL,
            ],
        }
    }

    fn log_section_header(self) -> (&'static str, &'static str) {
        match self {
            Self::PlanCluster => ("📋", "EKS Anywhere upgrade plan"),
            Self::UpgradeCluster => ("🚀", "EKS Anywhere cluster upgrade"),
        }
    }

    fn log_running_label(self) -> &'static str {
        match self {
            Self::PlanCluster => "`eksctl anywhere upgrade plan cluster`",
            Self::UpgradeCluster => "`eksctl anywhere upgrade cluster`",
        }
    }

    fn engine_error_message(self) -> &'static str {
        match self {
            Self::PlanCluster => "EKS Anywhere upgrade plan failed",
            Self::UpgradeCluster => "EKS Anywhere cluster upgrade failed",
        }
    }

    fn command_error_message(self) -> &'static str {
        match self {
            Self::PlanCluster => "Cannot run `eksctl anywhere upgrade plan cluster`",
            Self::UpgradeCluster => "Cannot run `eksctl anywhere upgrade cluster`",
        }
    }

    fn log_output_line(self, logger: &impl InfraLogger, line: &str, is_stderr: bool) {
        match self {
            Self::PlanCluster => log_live_upgrade_plan_line(logger, line, is_stderr),
            Self::UpgradeCluster => log_live_upgrade_progress_line(logger, line, is_stderr),
        }
    }

    fn log_pre_execution_info(self, config_path_str: &str) {
        if let Self::PlanCluster = self {
            info!("Running `eksctl anywhere upgrade plan cluster` against {}", config_path_str);
        }
    }
}

pub(super) fn run_eks_anywhere_upgrade_plan(
    cluster: &EksAnywhere,
    cluster_config_path: &Path,
    cloud_provider: &dyn CloudProvider,
    logger: &impl InfraLogger,
) -> Result<EksAnywhereUpgradePlanSummary, Box<EngineError>> {
    let stdout = run_eks_anywhere_upgrade_command(
        cluster,
        cluster_config_path,
        cloud_provider,
        EksAnywhereUpgradeCommand::PlanCluster,
        logger,
    )?;

    let no_upgrade_detected = upgrade_plan_reports_no_changes(&stdout);
    let expected_eksd_release_tag = expected_eksd_release_tag_from_upgrade_plan(&stdout);
    let kubernetes_version_transition = kubernetes_version_transition_from_upgrade_plan(&stdout);
    let summary = EksAnywhereUpgradePlanSummary {
        expected_eksd_release_tag,
        kubernetes_version_transition,
    };

    if let Some(expected_tag) = summary.expected_eksd_release_tag.as_deref() {
        logger.info(format!("🎯 Expected vSphere `eksdRelease` tag for upgrade: `{expected_tag}`."));
    } else if no_upgrade_detected {
        logger.info("🎯 No vSphere `eksdRelease` target from plan (cluster already up to date).");
    } else {
        logger.warn("Unable to infer expected vSphere `eksdRelease` tag from upgrade plan output.");
    }

    if let Some((current, next)) = summary.kubernetes_version_transition.as_ref() {
        logger.info(format!("🎯 Kubernetes version transition from plan: `{current}` -> `{next}`."));
    } else if no_upgrade_detected {
        logger.info("🎯 No Kubernetes version transition in plan (cluster already up to date).");
    } else {
        logger.warn("Unable to infer Kubernetes version transition from upgrade plan output.");
    }

    if summary.has_kubernetes_major_or_minor_change() {
        logger.warn("🎯 Kubernetes major/minor upgrade detected in plan.");
    } else {
        logger.info("🎯 No Kubernetes major/minor upgrade detected in plan.");
    }
    log_section_title(logger, "✅", "Upgrade plan completed");

    Ok(summary)
}

pub(super) fn run_eks_anywhere_upgrade_cluster(
    cluster: &EksAnywhere,
    cluster_config_path: &Path,
    cloud_provider: &dyn CloudProvider,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    run_eks_anywhere_upgrade_command(
        cluster,
        cluster_config_path,
        cloud_provider,
        EksAnywhereUpgradeCommand::UpgradeCluster,
        logger,
    )?;
    logger.info("📦 Upgrade command finished.");
    log_section_title(logger, "✅", "Cluster upgrade command completed");

    Ok(())
}

fn run_eks_anywhere_upgrade_command(
    cluster: &EksAnywhere,
    cluster_config_path: &Path,
    cloud_provider: &dyn CloudProvider,
    command: EksAnywhereUpgradeCommand,
    logger: &impl InfraLogger,
) -> Result<Vec<String>, Box<EngineError>> {
    let config_path_str = cluster_config_path.to_string_lossy().to_string();
    let kubeconfig_path = cluster.kubeconfig_local_file_path().to_string_lossy().to_string();

    // Ensure the kubeconfig exists for the current execution workspace before invoking eksctl.
    write_kubeconfig_on_disk(
        cluster.kubeconfig_local_file_path().as_path(),
        &cluster.kubeconfig,
        cluster.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
    )?;

    let envs = build_eks_anywhere_command_envs(kubeconfig_path.as_str(), cloud_provider);
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let args = command.eksctl_args(&config_path_str, &kubeconfig_path);

    let (icon, title) = command.log_section_header();
    log_section_title(logger, icon, title);
    logger.info(format!(
        "▶️ Running {} for `{}`.",
        command.log_running_label(),
        filename_for_user(Path::new(&config_path_str))
    ));
    command.log_pre_execution_info(&config_path_str);

    let mut cmd = QoveryCommand::new("eksctl", &args, &env_refs);
    cmd.set_current_dir(cluster.temp_dir());

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    if let Err(execution_error) = cmd.exec_with_output(
        &mut |line| {
            command.log_output_line(logger, line.as_str(), false);
            stdout.push(line);
        },
        &mut |line| {
            command.log_output_line(logger, line.as_str(), true);
            stderr.push(line);
        },
    ) {
        logger.warn(format!("{COMMAND_STDERR_PREFIX}❌ {} failed.", command.log_running_label()));
        return Err(Box::new(EngineError::new_unknown(
            cluster.get_event_details(Infrastructure(InfrastructureStep::Create)),
            command.engine_error_message().to_string(),
            Some(CommandError::new(
                command.command_error_message().to_string(),
                Some(if stderr.is_empty() {
                    execution_error.to_string()
                } else {
                    stderr.join("\n")
                }),
                None,
            )),
            None,
            None,
        )));
    }

    logger.info(format!("{COMMAND_STDOUT_PREFIX}✅ {} completed.", command.log_running_label()));

    Ok(stdout)
}

fn log_live_upgrade_progress_line(logger: &impl InfraLogger, line: &str, is_stderr: bool) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }

    if trimmed == "Command still running. No output available. Waiting for next line..." {
        logger.info(format!(
            "{}⏳ `eksctl anywhere upgrade cluster` is still running...",
            COMMAND_STDOUT_PREFIX
        ));
        return;
    }

    if trimmed.starts_with("Warning:") {
        logger.warn(format!("{COMMAND_STDERR_PREFIX}{trimmed}"));
        return;
    }

    let lower = trimmed.to_ascii_lowercase();
    if is_stderr && (lower.contains("error") || lower.contains("failed")) {
        logger.warn(format!("{COMMAND_STDERR_PREFIX}{trimmed}"));
    } else {
        logger.info(format!("{COMMAND_STDOUT_PREFIX}⏳ {trimmed}"));
    }
}

fn log_live_upgrade_plan_line(logger: &impl InfraLogger, line: &str, is_stderr: bool) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }

    if trimmed == "Command still running. No output available. Waiting for next line..." {
        logger.info(format!(
            "{}⏳ `eksctl anywhere upgrade plan cluster` is still running...",
            COMMAND_STDOUT_PREFIX
        ));
        return;
    }

    if trimmed.starts_with("Warning:") {
        logger.warn(format!("{COMMAND_STDERR_PREFIX}{trimmed}"));
        return;
    }

    if is_stderr {
        logger.warn(format!("{COMMAND_STDERR_PREFIX}{trimmed}"));
        return;
    }

    if trimmed.starts_with("Checking new release availability") {
        logger.info(format!("{COMMAND_STDOUT_PREFIX}🔎 {trimmed}"));
    } else if trimmed.starts_with("NAME ") {
        logger.info(format!(
            "{COMMAND_STDOUT_PREFIX}📈 Components upgrade matrix (current -> next):"
        ));
        logger.info(format!("{COMMAND_STDOUT_PREFIX}{trimmed}"));
    } else {
        logger.info(format!("{COMMAND_STDOUT_PREFIX}{trimmed}"));
    }
}

fn build_eks_anywhere_command_envs(kubeconfig_path: &str, cloud_provider: &dyn CloudProvider) -> Vec<(String, String)> {
    let mut envs = Vec::new();
    envs.push(("KUBECONFIG".to_string(), kubeconfig_path.to_string()));

    for (key, value) in cloud_provider.credentials_environment_variables() {
        insert_env_if_missing(&mut envs, key, value);
    }

    // Ensure eksctl always receives the vSphere variables it expects.
    // The target is listed first in sources so it is kept as-is if already set.
    let alias_rules: [(&str, &[&str]); 4] = [
        (
            "VSPHERE_USERNAME",
            &[
                "VSPHERE_USERNAME",
                "VSPHERE_USER",
                "EKSA_VSPHERE_USERNAME",
                "GOVC_USERNAME",
            ],
        ),
        (
            "VSPHERE_PASSWORD",
            &["VSPHERE_PASSWORD", "EKSA_VSPHERE_PASSWORD", "GOVC_PASSWORD"],
        ),
        (
            "EKSA_VSPHERE_USERNAME",
            &[
                "EKSA_VSPHERE_USERNAME",
                "VSPHERE_USERNAME",
                "VSPHERE_USER",
                "GOVC_USERNAME",
            ],
        ),
        (
            "EKSA_VSPHERE_PASSWORD",
            &["EKSA_VSPHERE_PASSWORD", "VSPHERE_PASSWORD", "GOVC_PASSWORD"],
        ),
    ];

    for (target, sources) in alias_rules {
        alias_env_if_missing(&mut envs, target, sources);
    }

    envs
}

fn insert_env_if_missing(envs: &mut Vec<(String, String)>, key: &str, value: &str) {
    if value.trim().is_empty() || envs.iter().any(|(existing_key, _)| existing_key == key) {
        return;
    }
    envs.push((key.to_string(), value.to_string()));
}

fn alias_env_if_missing(envs: &mut Vec<(String, String)>, target: &str, sources: &[&str]) {
    if envs.iter().any(|(key, _)| key == target) {
        return;
    }

    for source in sources {
        if let Some((_, value)) = envs.iter().find(|(key, _)| key == source)
            && !value.trim().is_empty()
        {
            envs.push((target.to_string(), value.to_string()));
            return;
        }
    }

    for source in sources {
        let Ok(value) = env::var(source) else {
            continue;
        };
        if value.trim().is_empty() {
            continue;
        }
        envs.push((target.to_string(), value));
        return;
    }
}

fn expected_eksd_release_tag_from_upgrade_plan(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.to_ascii_lowercase().starts_with("kubernetes") {
            return None;
        }
        let next_version = trimmed.split_whitespace().last()?;
        if !next_version.contains("-eks-") {
            return None;
        }
        eksd_release_tag_from_kubernetes_next_version(next_version)
    })
}

fn kubernetes_version_transition_from_upgrade_plan(lines: &[String]) -> Option<(String, String)> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.to_ascii_lowercase().starts_with("kubernetes") {
            return None;
        }

        let columns = trimmed.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 3 {
            return None;
        }

        let current = columns[columns.len() - 2].to_string();
        let next = columns[columns.len() - 1].to_string();
        Some((current, next))
    })
}

fn upgrade_plan_reports_no_changes(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        let normalized = line.trim().to_ascii_lowercase();
        normalized.contains("nothing to upgrade")
            || normalized.contains("all the components are up to date with the latest versions")
    })
}

fn extract_kubernetes_major_minor(version: &str) -> Option<(u64, u64)> {
    let normalized = version.trim().trim_start_matches('v');
    let k8s_version = normalized.split("-eks-").next().unwrap_or(normalized);
    let mut parts = k8s_version.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts
        .next()?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<u64>()
        .ok()?;
    Some((major, minor))
}

fn normalize_kubernetes_target_version_for_pluto(version: &str) -> Option<VersionsNumber> {
    let normalized = version.trim().trim_start_matches('v');
    let k8s_version = normalized.split("-eks-").next().unwrap_or(normalized);
    let mut parts = k8s_version.split('.');

    let major = parts
        .next()?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<u64>()
        .ok()?;
    let minor = parts
        .next()?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<u64>()
        .ok()?;
    let patch = parts
        .next()
        .map(|raw_patch| raw_patch.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .filter(|digits| !digits.is_empty())
        .and_then(|digits| digits.parse::<u64>().ok())
        .unwrap_or(0);

    Some(VersionsNumber::new(
        major.to_string(),
        Some(minor.to_string()),
        Some(patch.to_string()),
        None,
    ))
}

fn eksd_release_tag_from_kubernetes_next_version(next_version: &str) -> Option<String> {
    let next_version = next_version.trim().trim_start_matches('v');
    let (k8s_version, eks_release) = next_version.split_once("-eks-")?;
    let mut version_parts = k8s_version.split('.');
    let major = version_parts.next()?;
    let minor = version_parts.next()?;
    let release_suffix = eks_release.split('-').next_back()?;
    if major.is_empty() || minor.is_empty() || release_suffix.is_empty() {
        return None;
    }

    Some(format!("kubernetes-{major}-{minor}-eks-{release_suffix}"))
}

fn filename_for_user(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn log_section_title(logger: &impl InfraLogger, icon: &str, title: &str) {
    logger.info("");
    logger.info(format!("***** {icon} {title} *****"));
    logger.info("");
}
