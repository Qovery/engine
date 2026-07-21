use super::InfraLogger;
use super::utils::mk_logger;
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::Helm;
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, EventMessage, InfrastructureDiffType, InfrastructureStep};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::io_models::container::Registry;
use crate::io_models::engine_request::InfrastructureEngineRequest;
use crate::io_models::platform_components::{
    PlatformExecutionResult, PlatformHelmUnit, PlatformHelmUnitAction, PlatformUnitErrorCode, PlatformUnitResult,
};
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tempfile::NamedTempFile;
use url::Url;
use uuid::Uuid;

const HELM_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const HELM_DIFF_TIMEOUT: Duration = Duration::from_secs(120);
const HELM_UPGRADE_TIMEOUT: Duration = Duration::from_secs(600);

/// The only request schema version this engine build understands for platform units.
const SUPPORTED_REQUEST_SCHEMA_VERSION: u32 = 1;

/// Platform units are Qovery-owned: the namespace is protected configuration, not an input.
const PROTECTED_PLATFORM_NAMESPACE: &str = "qovery";

/// Kubernetes reads the container termination message from this path by default and truncates
/// it at 4096 bytes. Override with `QOVERY_TERMINATION_MESSAGE_PATH` (tests, local runs).
const DEFAULT_TERMINATION_MESSAGE_PATH: &str = "/dev/termination-log";
const TERMINATION_MESSAGE_MAX_BYTES: usize = 4096;

enum PlatformHelmDeploymentEvent<'a> {
    ShowingDiff { chart_name: &'a str },
    Deploying { chart_name: &'a str },
    Deployed { chart_name: &'a str },
}

impl Display for PlatformHelmDeploymentEvent<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformHelmDeploymentEvent::ShowingDiff { chart_name } => {
                write!(formatter, "🔍 Showing diff for chart: {chart_name}")
            }
            PlatformHelmDeploymentEvent::Deploying { chart_name } => {
                write!(formatter, "🛳️ Deploying chart: 📥 {chart_name}")
            }
            PlatformHelmDeploymentEvent::Deployed { chart_name } => {
                write!(formatter, "✅ Chart {chart_name} deployed")
            }
        }
    }
}

pub fn deploy_platform_components(
    infra_ctx: &InfrastructureContext,
    request: &InfrastructureEngineRequest,
) -> Result<(), Box<EngineError>> {
    let kubernetes = infra_ctx.kubernetes();
    let logger = mk_logger(kubernetes, InfrastructureStep::Create);
    let result_logger = mk_logger(kubernetes, InfrastructureStep::PlatformExecutionResult);
    let event_details = kubernetes.get_event_details(Infrastructure(InfrastructureStep::Create));

    let units = request
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.engine_v2_options.as_ref())
        .map(|engine_v2_options| engine_v2_options.platform_helm_units.as_slice())
        .unwrap_or(&[]);
    let schema_version = request
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.engine_v2_options.as_ref())
        .map(|engine_v2_options| engine_v2_options.schema_version.as_str());

    // Whitelist validation, fail-closed: nothing executes when any part of the request is
    // outside what this path explicitly supports.
    if let Err(violation) = validate_platform_request(schema_version, units) {
        let unit_results = units
            .iter()
            .map(|unit| match &violation.unit_key {
                Some(key) if *key == unit.key => {
                    PlatformUnitResult::failed(&unit.key, violation.code, &violation.message)
                }
                Some(_) => PlatformUnitResult::skipped(&unit.key, "VALIDATION_FAILED"),
                None => PlatformUnitResult::failed(&unit.key, violation.code, &violation.message),
            })
            .collect();
        write_platform_execution_result(&logger, &result_logger, &request.id, unit_results);
        return Err(Box::new(EngineError::new_invalid_engine_payload(
            event_details,
            &violation.message,
            None,
        )));
    }

    logger.info(format!(
        "🧩 Platform components only: {} Helm unit(s) to apply, cluster lifecycle is skipped",
        units.len()
    ));

    // The worker runs inside the customer cluster: use the kubeconfig when the request provided
    // one, otherwise fall back to in-cluster ServiceAccount credentials (no kubeconfig file).
    let kubeconfig_path = kubernetes.kubeconfig_local_file_path();
    let helm = match if kubeconfig_path.exists() {
        logger.info("Using the provided kubeconfig");
        Helm::new(Some(&kubeconfig_path), &[])
    } else {
        logger.info("Using in-cluster Kubernetes credentials (no kubeconfig provided)");
        Helm::new(Option::<&Path>::None, &[])
    } {
        Ok(helm) => helm,
        Err(err) => {
            let unit_results = units
                .iter()
                .map(|unit| {
                    PlatformUnitResult::failed(
                        &unit.key,
                        PlatformUnitErrorCode::Internal,
                        "cannot initialize the Helm client",
                    )
                })
                .collect();
            write_platform_execution_result(&logger, &result_logger, &request.id, unit_results);
            return Err(Box::new(EngineError::new_helm_chart_error(event_details, err.into())));
        }
    };

    // Units execute sequentially (one step for Slice 1). After the first failure, remaining
    // units are reported SKIPPED/UPSTREAM_FAILED and never start (docs-v2 step semantics).
    let mut unit_results: Vec<PlatformUnitResult> = Vec::with_capacity(units.len());
    let mut first_failure: Option<Box<EngineError>> = None;
    for unit in units {
        if first_failure.is_some() {
            unit_results.push(PlatformUnitResult::skipped(&unit.key, "UPSTREAM_FAILED"));
            continue;
        }
        match apply_platform_helm_unit(infra_ctx, &helm, &logger, &event_details, unit) {
            Ok(()) => unit_results.push(PlatformUnitResult::succeeded(&unit.key)),
            Err((code, message, err)) => {
                unit_results.push(PlatformUnitResult::failed(&unit.key, code, &message));
                first_failure = Some(err);
            }
        }
    }

    write_platform_execution_result(&logger, &result_logger, &request.id, unit_results);

    match first_failure {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

pub fn fail_unknown_execution_mode(
    infra_ctx: &InfrastructureContext,
    request: &InfrastructureEngineRequest,
) -> Box<EngineError> {
    const MESSAGE: &str = "unsupported execution_mode: this engine version does not know this mode; refusing to fall back to the cluster lifecycle";

    let kubernetes = infra_ctx.kubernetes();
    let logger = mk_logger(kubernetes, InfrastructureStep::Create);
    let result_logger = mk_logger(kubernetes, InfrastructureStep::PlatformExecutionResult);
    let event_details = kubernetes.get_event_details(Infrastructure(InfrastructureStep::Create));

    let unit_results = request
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.engine_v2_options.as_ref())
        .map(|engine_v2_options| engine_v2_options.platform_helm_units.as_slice())
        .unwrap_or(&[])
        .iter()
        .map(|unit| PlatformUnitResult::failed(&unit.key, PlatformUnitErrorCode::InvalidPayload, MESSAGE))
        .collect();
    write_platform_execution_result(&logger, &result_logger, &request.id, unit_results);

    Box::new(EngineError::new_invalid_engine_payload(event_details, MESSAGE, None))
}

struct PlatformValidationError {
    code: PlatformUnitErrorCode,
    message: String,
    /// `None` for request-level violations; `Some(unit key)` when one unit is at fault.
    unit_key: Option<String>,
}

fn validate_platform_request(
    schema_version: Option<&str>,
    units: &[PlatformHelmUnit],
) -> Result<(), PlatformValidationError> {
    let request_error = |code: PlatformUnitErrorCode, message: String| PlatformValidationError {
        code,
        message,
        unit_key: None,
    };

    match schema_version.map(str::parse::<u32>) {
        Some(Ok(SUPPORTED_REQUEST_SCHEMA_VERSION)) => {}
        Some(Ok(version)) => {
            return Err(request_error(
                PlatformUnitErrorCode::UnsupportedSchemaVersion,
                format!(
                    "unsupported request schema_version {version}: this engine supports {SUPPORTED_REQUEST_SCHEMA_VERSION}"
                ),
            ));
        }
        Some(Err(_)) | None => {
            return Err(request_error(
                PlatformUnitErrorCode::UnsupportedSchemaVersion,
                "missing or non-numeric request schema_version".to_string(),
            ));
        }
    }

    if units.is_empty() {
        return Err(request_error(
            PlatformUnitErrorCode::InvalidPayload,
            "execution_mode is platform_components_only but the request carries no platform_helm_units".to_string(),
        ));
    }

    for unit in units {
        let unit_error = |code: PlatformUnitErrorCode, message: String| PlatformValidationError {
            code,
            message,
            unit_key: Some(unit.key.clone()),
        };

        if unit.key.is_empty()
            || unit.key.len() > 63
            || !unit
                .key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(unit_error(
                PlatformUnitErrorCode::InvalidPayload,
                format!(
                    "platform Helm unit key `{}` must be a 1-63 character alphanumeric/dash identifier",
                    unit.key
                ),
            ));
        }
        if unit.action != PlatformHelmUnitAction::Create {
            return Err(unit_error(
                PlatformUnitErrorCode::ForbiddenAction,
                format!("platform Helm unit `{}` has an unsupported action", unit.key),
            ));
        }
        if unit.namespace != PROTECTED_PLATFORM_NAMESPACE {
            return Err(unit_error(
                PlatformUnitErrorCode::ForbiddenAction,
                format!(
                    "platform Helm unit `{}` targets namespace `{}`: only the protected `{PROTECTED_PLATFORM_NAMESPACE}` namespace is allowed",
                    unit.key, unit.namespace
                ),
            ));
        }
        if unit.release_name.trim().is_empty() {
            return Err(unit_error(
                PlatformUnitErrorCode::InvalidPayload,
                format!("platform Helm unit `{}` has an empty release name", unit.key),
            ));
        }
        if unit.chart.version.trim().is_empty() || unit.chart.version.trim().eq_ignore_ascii_case("latest") {
            return Err(unit_error(
                PlatformUnitErrorCode::InvalidPayload,
                format!(
                    "platform Helm unit `{}` chart version `{}` must be an explicit, non-latest version",
                    unit.key, unit.chart.version
                ),
            ));
        }
        if Url::parse(&unit.chart.repository).is_err() {
            return Err(unit_error(
                PlatformUnitErrorCode::InvalidPayload,
                format!(
                    "platform Helm unit `{}` has an invalid chart repository `{}`",
                    unit.key, unit.chart.repository
                ),
            ));
        }
        // Do not embed the serde_yaml error in the message: values may contain secrets and
        // parser errors can quote the offending content.
        if serde_yaml::from_str::<serde_yaml::Value>(&unit.values_yaml).is_err() {
            return Err(unit_error(
                PlatformUnitErrorCode::InvalidPayload,
                format!("platform Helm unit `{}` values_yaml is not valid YAML", unit.key),
            ));
        }
    }

    Ok(())
}

fn write_platform_execution_result(
    logger: &impl InfraLogger,
    result_logger: &impl InfraLogger,
    execution_id: &str,
    units: Vec<PlatformUnitResult>,
) {
    let path = std::env::var("QOVERY_TERMINATION_MESSAGE_PATH")
        .unwrap_or_else(|_| DEFAULT_TERMINATION_MESSAGE_PATH.to_string());
    let (json, warnings) = write_termination_message_to(Path::new(&path), execution_id, units);
    for warning in warnings {
        logger.warn(warning);
    }
    result_logger.info(EventMessage::new_for_sending_core_data(json.clone(), json));
}

fn write_termination_message_to(
    path: &Path,
    execution_id: &str,
    units: Vec<PlatformUnitResult>,
) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let mut result = PlatformExecutionResult {
        schema_version: 1,
        execution_id: execution_id.to_string(),
        units,
    };

    let mut json = serde_json::to_string(&result).unwrap_or_default();
    if json.is_empty() || json.len() > TERMINATION_MESSAGE_MAX_BYTES {
        for unit in &mut result.units {
            unit.message = None;
        }
        json = serde_json::to_string(&result).unwrap_or_default();
    }
    if json.is_empty() || json.len() > TERMINATION_MESSAGE_MAX_BYTES {
        warnings.push(format!(
            "platform execution result ({} unit(s)) exceeds the {TERMINATION_MESSAGE_MAX_BYTES}-byte termination-message limit even without messages: falling back to the empty-units sentinel",
            result.units.len()
        ));
        result.units = Vec::new();
        json = serde_json::to_string(&result).unwrap_or_default();
    }

    if let Err(err) = std::fs::write(path, &json) {
        warnings.push(format!("cannot write the platform execution result to {path:?}: {err}"));
    }
    (json, warnings)
}

fn apply_platform_helm_unit(
    infra_ctx: &InfrastructureContext,
    helm: &Helm,
    logger: &impl InfraLogger,
    event_details: &EventDetails,
    unit: &PlatformHelmUnit,
) -> Result<(), (PlatformUnitErrorCode, String, Box<EngineError>)> {
    logger.info(format!(
        "⚓ Preparing platform Helm unit `{}`: release `{}` in namespace `{}` from {} {} {}",
        unit.key, unit.release_name, unit.namespace, unit.chart.repository, unit.chart.name, unit.chart.version,
    ));

    let chart_dir = download_platform_chart(infra_ctx, helm, logger, event_details, unit)?;
    let values_file = write_platform_values_to_temporary_file(unit).map_err(|err| {
        internal_fs_error(
            event_details,
            format!("cannot prepare temporary Helm values for platform unit `{}`: {err}", unit.key),
        )
    })?;

    let chart_info = ChartInfo {
        name: unit.release_name.clone(),
        path: chart_dir,
        namespace: HelmChartNamespaces::Custom(unit.namespace.clone()),
        timeout_in_seconds: HELM_UPGRADE_TIMEOUT.as_secs() as i64,
        // values_yaml may contain secrets (e.g. cluster JWT): keep the temporary file outside
        // the archived workspace, never pass its contents as command-line arguments, and keep it
        // alive until both Helm commands have completed.
        values_files: vec![values_file.path().to_string_lossy().into_owned()],
        ..Default::default()
    };

    logger.info(
        PlatformHelmDeploymentEvent::ShowingDiff {
            chart_name: &unit.release_name,
        }
        .to_string(),
    );
    // Keep the traditional cluster behavior: a best-effort diff must never block the deployment.
    // Do not log the Helm error because command errors may contain sensitive values.
    if helm
        .upgrade_diff_with_secrets_suppressed(
            &chart_info,
            &[],
            &CommandKiller::from_timeout(HELM_DIFF_TIMEOUT),
            &mut |line| {
                logger.diff(InfrastructureDiffType::Helm, line);
            },
        )
        .is_err()
    {
        logger.warn(format!(
            "Unable to show diff for chart {}; continuing deployment",
            unit.release_name
        ));
    }

    if infra_ctx.context().is_dry_run_deploy() {
        logger.warn(format!(
            "👻 Dry run mode enabled, skipping installation of platform Helm unit `{}`",
            unit.key
        ));
        return Ok(());
    }

    logger.info(
        PlatformHelmDeploymentEvent::Deploying {
            chart_name: &unit.release_name,
        }
        .to_string(),
    );
    helm.upgrade(&chart_info, &[], &CommandKiller::from_timeout(HELM_UPGRADE_TIMEOUT))
        .map_err(|err| {
            (
                PlatformUnitErrorCode::HelmFailed,
                format!(
                    "helm upgrade failed for release `{}` in namespace `{}`",
                    unit.release_name, unit.namespace
                ),
                Box::new(EngineError::new_helm_chart_error(event_details.clone(), err.into())),
            )
        })?;

    logger.info(
        PlatformHelmDeploymentEvent::Deployed {
            chart_name: &unit.release_name,
        }
        .to_string(),
    );

    Ok(())
}

fn write_platform_values_to_temporary_file(unit: &PlatformHelmUnit) -> std::io::Result<NamedTempFile> {
    let mut values_file = tempfile::Builder::new()
        .prefix("qovery-platform-values-")
        .suffix(".yaml")
        .tempfile()?;
    values_file.write_all(unit.values_yaml.as_bytes())?;
    values_file.flush()?;
    Ok(values_file)
}

/// Downloads the unit chart from its snapshotted repository into the workspace and returns the
/// local chart directory the Helm upgrade runs from. Reuses the same Helm-level building blocks
/// as the Helm service deployment (`deploy_helm_chart.rs`): the https/oci download dispatcher
/// and the chart dependency build.
fn download_platform_chart(
    infra_ctx: &InfrastructureContext,
    helm: &Helm,
    logger: &impl InfraLogger,
    event_details: &EventDetails,
    unit: &PlatformHelmUnit,
) -> Result<String, (PlatformUnitErrorCode, String, Box<EngineError>)> {
    // Already checked by the validation phase; kept as defense in depth.
    let repository_url = Url::parse(&unit.chart.repository).map_err(|err| {
        let message = format!(
            "platform Helm unit `{}` has an invalid chart repository `{}`",
            unit.key, unit.chart.repository
        );
        (
            PlatformUnitErrorCode::InvalidPayload,
            message.clone(),
            Box::new(EngineError::new_invalid_engine_payload(
                event_details.clone(),
                &format!("{message}: {err}"),
                None,
            )),
        )
    })?;

    // The helm download helper requires the target directory to exist and the final rename
    // requires it to be empty: clean any leftover from a previous attempt, then recreate it.
    let charts_root = Path::new(infra_ctx.context().workspace_root_dir()).join("platform-components");
    let chart_dir = charts_root.join(&unit.key);
    if chart_dir.exists() {
        std::fs::remove_dir_all(&chart_dir).map_err(|err| {
            internal_fs_error(
                event_details,
                format!("cannot clean platform components chart directory {chart_dir:?}: {err}"),
            )
        })?;
    }
    std::fs::create_dir_all(&chart_dir).map_err(|err| {
        internal_fs_error(
            event_details,
            format!("cannot create platform components chart directory {chart_dir:?}: {err}"),
        )
    })?;

    // Anonymous registry: platform unit repositories are public (https or oci) in Slice 1.
    // Credentialed repositories will need a registry model on the unit.
    let registry = Registry::GenericCr {
        long_id: Uuid::nil(),
        url: repository_url.clone(),
        credentials: None,
    };
    helm.download_chart(
        &repository_url,
        &registry,
        &unit.chart.name,
        &unit.chart.version,
        &chart_dir,
        false,
        &[],
        &CommandKiller::from_timeout(HELM_DOWNLOAD_TIMEOUT),
    )
    .map_err(|err| {
        (
            PlatformUnitErrorCode::ChartFetchFailed,
            format!(
                "cannot download chart `{}` version `{}` from `{}`",
                unit.chart.name, unit.chart.version, unit.chart.repository
            ),
            Box::new(EngineError::new_helm_chart_error(event_details.clone(), err.into())),
        )
    })?;

    // Fetch chart dependencies, as the Helm service deployment does.
    helm.dependency_build(
        &unit.release_name,
        &charts_root,
        &chart_dir,
        &[],
        &[],
        &CommandKiller::from_timeout(HELM_DOWNLOAD_TIMEOUT),
        &mut |line| logger.info(line),
        &mut |line| logger.warn(line),
    )
    .map_err(|err| {
        (
            PlatformUnitErrorCode::ChartFetchFailed,
            format!("cannot build chart dependencies for `{}`", unit.chart.name),
            Box::new(EngineError::new_helm_chart_error(event_details.clone(), err.into())),
        )
    })?;

    Ok(chart_dir.to_string_lossy().to_string())
}

fn internal_fs_error(
    event_details: &EventDetails,
    message: String,
) -> (PlatformUnitErrorCode, String, Box<EngineError>) {
    (
        PlatformUnitErrorCode::Internal,
        message.clone(),
        Box::new(EngineError::new_invalid_engine_payload(event_details.clone(), &message, None)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io_models::platform_components::PlatformHelmChartSource;

    fn valid_unit() -> PlatformHelmUnit {
        PlatformHelmUnit {
            key: "cluster-agent".to_string(),
            action: PlatformHelmUnitAction::Create,
            release_name: "cluster-agent".to_string(),
            namespace: "qovery".to_string(),
            chart: PlatformHelmChartSource {
                repository: "https://helm.qovery.com".to_string(),
                name: "qovery-cluster-agent".to_string(),
                version: "0.1.0".to_string(),
            },
            values_yaml: "image:\n  tag: \"0.1.0\"\n".to_string(),
            images: vec![],
        }
    }

    #[test]
    fn platform_helm_deployment_events_use_expected_messages() {
        let chart_name = "cluster-agent";

        assert_eq!(
            PlatformHelmDeploymentEvent::ShowingDiff { chart_name }.to_string(),
            "🔍 Showing diff for chart: cluster-agent"
        );
        assert_eq!(
            PlatformHelmDeploymentEvent::Deploying { chart_name }.to_string(),
            "🛳️ Deploying chart: 📥 cluster-agent"
        );
        assert_eq!(
            PlatformHelmDeploymentEvent::Deployed { chart_name }.to_string(),
            "✅ Chart cluster-agent deployed"
        );
    }

    #[test]
    fn platform_values_file_is_removed_when_dropped() {
        let unit = valid_unit();
        let values_file_path;

        {
            let values_file = write_platform_values_to_temporary_file(&unit).unwrap();
            values_file_path = values_file.path().to_path_buf();
            assert_eq!(std::fs::read_to_string(&values_file_path).unwrap(), unit.values_yaml);
        }

        assert!(!values_file_path.exists());
    }

    #[test]
    fn valid_request_passes_validation() {
        assert!(validate_platform_request(Some("1"), &[valid_unit()]).is_ok());
    }

    #[test]
    fn missing_or_unsupported_schema_version_is_rejected_before_execution() {
        for schema_version in [None, Some("2"), Some("abc")] {
            let err = validate_platform_request(schema_version, &[valid_unit()])
                .err()
                .unwrap();
            assert_eq!(err.code, PlatformUnitErrorCode::UnsupportedSchemaVersion);
            assert_eq!(err.unit_key, None);
        }
    }

    #[test]
    fn empty_units_are_rejected() {
        let err = validate_platform_request(Some("1"), &[]).err().unwrap();
        assert_eq!(err.code, PlatformUnitErrorCode::InvalidPayload);
    }

    #[test]
    fn unknown_action_is_a_forbidden_action() {
        let mut unit = valid_unit();
        unit.action = PlatformHelmUnitAction::Unknown;
        let err = validate_platform_request(Some("1"), &[unit]).err().unwrap();
        assert_eq!(err.code, PlatformUnitErrorCode::ForbiddenAction);
        assert_eq!(err.unit_key.as_deref(), Some("cluster-agent"));
    }

    #[test]
    fn non_protected_namespace_is_a_forbidden_action() {
        let mut unit = valid_unit();
        unit.namespace = "kube-system".to_string();
        let err = validate_platform_request(Some("1"), &[unit]).err().unwrap();
        assert_eq!(err.code, PlatformUnitErrorCode::ForbiddenAction);
    }

    #[test]
    fn latest_or_empty_chart_version_is_rejected() {
        for version in ["latest", "LATEST", "", "  "] {
            let mut unit = valid_unit();
            unit.chart.version = version.to_string();
            let err = validate_platform_request(Some("1"), &[unit]).err().unwrap();
            assert_eq!(err.code, PlatformUnitErrorCode::InvalidPayload);
        }
    }

    #[test]
    fn invalid_chart_repository_is_rejected() {
        let mut unit = valid_unit();
        unit.chart.repository = "not a url".to_string();
        let err = validate_platform_request(Some("1"), &[unit]).err().unwrap();
        assert_eq!(err.code, PlatformUnitErrorCode::InvalidPayload);
    }

    #[test]
    fn invalid_values_yaml_is_rejected_without_leaking_its_content() {
        let mut unit = valid_unit();
        unit.values_yaml = "secret: [unclosed".to_string();
        let err = validate_platform_request(Some("1"), &[unit]).err().unwrap();
        assert_eq!(err.code, PlatformUnitErrorCode::InvalidPayload);
        assert!(!err.message.contains("unclosed"));
    }

    #[test]
    fn empty_release_name_is_rejected() {
        let mut unit = valid_unit();
        unit.release_name = " ".to_string();
        let err = validate_platform_request(Some("1"), &[unit]).err().unwrap();
        assert_eq!(err.code, PlatformUnitErrorCode::InvalidPayload);
    }

    #[test]
    fn path_traversal_or_invalid_unit_keys_are_rejected() {
        // The key becomes a workspace directory later removed with remove_dir_all: anything
        // that could escape the workspace must be refused at validation time.
        let long_key = "x".repeat(64);
        for key in ["../../etc", "a/b", "a\\b", "..", ".", "", " ", &long_key] {
            let mut unit = valid_unit();
            unit.key = key.to_string();
            let err = validate_platform_request(Some("1"), &[unit]).err().unwrap();
            assert_eq!(err.code, PlatformUnitErrorCode::InvalidPayload, "key `{key}` must be rejected");
        }
    }

    #[test]
    fn termination_message_is_written_as_parseable_json() {
        let path = std::env::temp_dir().join(format!("qovery-termination-test-{}-parseable", std::process::id()));
        let (json, warnings) = write_termination_message_to(
            &path,
            "exec-1",
            vec![
                PlatformUnitResult::succeeded("cluster-agent"),
                PlatformUnitResult::failed("shell-agent", PlatformUnitErrorCode::HelmFailed, "boom"),
            ],
        );
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(json, written);
        let parsed: PlatformExecutionResult = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed.units.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn oversized_termination_message_always_degrades_to_parseable_json_under_the_limit() {
        let path = std::env::temp_dir().join(format!("qovery-termination-test-{}-oversized", std::process::id()));
        // Enough units that even the message-stripped JSON exceeds 4096 bytes: the writer must
        // fall back to the empty-units sentinel instead of letting Kubernetes truncate mid-JSON.
        let units: Vec<PlatformUnitResult> = (0..200)
            .map(|i| {
                PlatformUnitResult::failed(
                    &format!("unit-with-a-rather-long-key-{i}"),
                    PlatformUnitErrorCode::HelmFailed,
                    &"m".repeat(200),
                )
            })
            .collect();
        let (json, warnings) = write_termination_message_to(&path, "exec-1", units);
        assert!(!warnings.is_empty(), "the sentinel fallback must be reported as a warning");
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(json, written);
        assert!(written.len() <= TERMINATION_MESSAGE_MAX_BYTES);
        let parsed: PlatformExecutionResult = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed.execution_id, "exec-1");
        assert!(parsed.units.is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
