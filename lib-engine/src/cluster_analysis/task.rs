use crate::cmd::docker::Docker;
use crate::cmd::krr::{Krr, KrrOptions, KrrOutputFormat};
use crate::cmd::kubectl::{KubectlPortForward, KubernetesServicePortForwardTarget};
use crate::engine_task::Task;
use crate::engine_task::qovery_api::QoveryApi;
use crate::environment::models::abort::{Abort, AbortStatus, AtomicAbortStatus};
use crate::environment::models::types::{DeployedEngineVersion, VersionsNumber};
use crate::errors::{CommandError, EngineError};
use crate::events::{ClusterAnalysisStep, EngineEvent, EventDetails, EventMessage, Stage};
use crate::fs::workspace_directory;
use crate::infrastructure::models::kubernetes;
use crate::io_models::context::Context;
use crate::io_models::engine_request::{
    AnalysisOutputFormat, ClusterAnalysisEngineRequest, ClusterAnalysisRequest, CostRecommendationRequest,
    DeprecatedApiCheckRequest,
};
use crate::io_models::metrics::{MetricsConfiguration, MetricsParameters};
use crate::log_file_writer::LogFileWriter;
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use crate::services::kube_client::QubeClient;
use crate::services::kubernetes_api_deprecation_service::{Deprecation, KubernetesApiDeprecationServiceGranuality};
use crate::{engine_task, hack};
use serde::Serialize;
use serde_json::Value;
use std::fmt::Write as _;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use url::Url;

const QOVERY_OBS_THANOS_QUERY_SERVICE: &str = "thanos-query";
const QOVERY_OBS_THANOS_QUERY_PORT: u16 = 9090;

pub struct ClusterAnalysisTask {
    workspace_root_dir: String,
    lib_root_dir: String,
    aws_apn_id: String,
    engine_version: DeployedEngineVersion,
    docker: Arc<Docker>,
    request: ClusterAnalysisEngineRequest,
    cancel_requested: Arc<AtomicAbortStatus>,
    logger: Box<dyn Logger>,
    qovery_api: Arc<dyn QoveryApi>,
    span: tracing::Span,
    is_terminated: (RwLock<Option<broadcast::Sender<()>>>, broadcast::Receiver<()>),
    log_file_writer: Option<LogFileWriter>,
}

impl ClusterAnalysisTask {
    pub fn new(
        request: ClusterAnalysisEngineRequest,
        workspace_root_dir: String,
        lib_root_dir: String,
        aws_apn_id: String,
        engine_version: DeployedEngineVersion,
        docker: Arc<Docker>,
        logger: Box<dyn Logger>,
        _metrics_registry: Box<dyn MetricsRegistry>,
        qovery_api: Box<dyn QoveryApi>,
        log_file_writer: Option<LogFileWriter>,
    ) -> Self {
        let span = info_span!(
            "cluster_analysis_task",
            organization_id = request.organization_long_id.to_string(),
            cluster_id = request.kubernetes.long_id.to_string(),
            execution_id = request.id,
        );

        Self {
            workspace_root_dir,
            lib_root_dir,
            aws_apn_id,
            engine_version,
            docker,
            request,
            cancel_requested: Arc::new(AtomicAbortStatus::new(AbortStatus::None)),
            logger,
            qovery_api: Arc::from(qovery_api),
            span,
            is_terminated: {
                let (tx, rx) = broadcast::channel(1);
                (RwLock::new(Some(tx)), rx)
            },
            log_file_writer,
        }
    }

    fn get_event_details(&self, step: ClusterAnalysisStep) -> EventDetails {
        EventDetails::clone_changing_stage(self.request.event_details(), Stage::ClusterAnalysis(step))
    }

    fn run_analysis(&self) -> Result<String, Box<EngineError>> {
        let (cloud_provider, kubernetes) = self.request.to_cluster_analysis_kubernetes(
            &self.info_context(),
            self.request.event_details(),
            self.logger.clone(),
        )?;
        let kubeconfig_path = kubernetes.kubeconfig_local_file_path();
        let credentials_envs = cloud_provider
            .credentials_environment_variables()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<Vec<_>>();
        let credentials_envs_ref = credentials_envs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>();

        match &self.request.target_analysis {
            ClusterAnalysisRequest::CostRecommendation(payload) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(ClusterAnalysisStep::CostRecommendation),
                    EventMessage::new_from_safe("Running KRR cost recommendation analysis".to_string()),
                ));
                self.run_krr(payload, &kubeconfig_path, credentials_envs_ref.as_slice())
            }
            ClusterAnalysisRequest::DeprecatedApiCheck(payload) => {
                self.logger.log(EngineEvent::Info(
                    self.get_event_details(ClusterAnalysisStep::DeprecatedApiCheck),
                    EventMessage::new_from_safe("Running Kubernetes deprecated API analysis".to_string()),
                ));
                self.run_deprecated_api_check(
                    payload,
                    &kubeconfig_path,
                    credentials_envs.as_slice(),
                    credentials_envs_ref.as_slice(),
                )
            }
        }
    }

    fn run_krr(
        &self,
        payload: &CostRecommendationRequest,
        kubeconfig_path: &std::path::Path,
        envs: &[(&str, &str)],
    ) -> Result<String, Box<EngineError>> {
        let requested_prometheus_url = match &payload.prometheus_url {
            Some(url) => url.clone(),
            None => self.default_qovery_obs_thanos_query_url()?,
        };
        let port_forward_guard = KubernetesServicePortForwardTarget::from_service_url(&requested_prometheus_url)
            .map(|target| KubectlPortForward::start(kubeconfig_path, target, envs))
            .transpose()
            .map_err(|error| {
                Box::new(EngineError::new_unknown(
                    self.get_event_details(ClusterAnalysisStep::CostRecommendation),
                    "Cannot open Prometheus port-forward for KRR analysis".to_string(),
                    Some(error),
                    None,
                    None,
                ))
            })?;
        let prometheus_url = match &port_forward_guard {
            Some(port_forward) => port_forward.local_url().map_err(|error| {
                Box::new(EngineError::new_unknown(
                    self.get_event_details(ClusterAnalysisStep::CostRecommendation),
                    "Cannot build Prometheus port-forward URL for KRR analysis".to_string(),
                    Some(error),
                    None,
                    None,
                ))
            })?,
            None => requested_prometheus_url,
        };
        let output_file = self.report_file_path(payload.output_format.as_krr_formatter())?;
        let options = KrrOptions {
            output_format: krr_output_format(payload.output_format),
            prometheus_url,
            extra_args: payload.cmd_args.clone(),
            file_output: Some(output_file),
        };

        Krr::new()
            .get_recommendations(kubeconfig_path, &options, envs, self.cancel_checker().as_ref())
            .map_err(|error| {
                Box::new(EngineError::new_unknown(
                    self.get_event_details(ClusterAnalysisStep::CostRecommendation),
                    "KRR cost recommendation analysis failed".to_string(),
                    Some(CommandError::new(error.to_string(), Some(format!("{error:?}")), None)),
                    None,
                    None,
                ))
            })
    }

    fn run_deprecated_api_check(
        &self,
        payload: &DeprecatedApiCheckRequest,
        kubeconfig_path: &std::path::Path,
        kube_credentials: &[(String, String)],
        command_envs: &[(&str, &str)],
    ) -> Result<String, Box<EngineError>> {
        let target_version = payload
            .target_kubernetes_version
            .as_ref()
            .map(|version| {
                VersionsNumber::from_str(version)
                    .map_err(|_| self.invalid_payload_error(format!("Invalid Kubernetes target version `{version}`")))
            })
            .transpose()?;
        let kube_client = QubeClient::new(
            self.get_event_details(ClusterAnalysisStep::DeprecatedApiCheck),
            Some(kubeconfig_path.to_path_buf()),
            kube_credentials.to_vec(),
        )?;
        let deprecations =
            crate::services::kubernetes_api_deprecation_service::KubernetesApiDeprecationService::default()
                .get_deprecated_kubernetes_apis(
                    kubeconfig_path,
                    target_version.as_ref(),
                    command_envs,
                    KubernetesApiDeprecationServiceGranuality::WithQoveryMetadata {
                        kube_client: kube_client.as_ref(),
                    },
                )
                .map_err(|error| {
                    Box::new(EngineError::new_unknown(
                        self.get_event_details(ClusterAnalysisStep::DeprecatedApiCheck),
                        "Kubernetes deprecated API analysis failed".to_string(),
                        Some(CommandError::new(error.to_string(), Some(format!("{error:?}")), None)),
                        None,
                        None,
                    ))
                })?;

        Ok(format_deprecations(&deprecations, payload.output_format))
    }

    fn invalid_payload_error(&self, message: String) -> Box<EngineError> {
        Box::new(EngineError::new_invalid_engine_payload(
            self.get_event_details(ClusterAnalysisStep::Error),
            &message,
            None,
        ))
    }

    fn default_qovery_obs_thanos_query_url(&self) -> Result<Url, Box<EngineError>> {
        if !is_qovery_obs_enabled(&self.request.kubernetes.options) {
            return Err(self.invalid_payload_error(
                "KRR analysis requires `prometheus_url` when Qovery OBS is not enabled on the cluster".to_string(),
            ));
        }

        let namespace = qovery_obs_namespace(self.request.kubernetes.kind).ok_or_else(|| {
            self.invalid_payload_error(format!(
                "Cannot infer Qovery OBS namespace for Kubernetes kind `{}`; please provide `prometheus_url`",
                self.request.kubernetes.kind
            ))
        })?;
        let url = format!(
            "http://{QOVERY_OBS_THANOS_QUERY_SERVICE}.{namespace}.svc.cluster.local:{QOVERY_OBS_THANOS_QUERY_PORT}"
        );

        Url::parse(&url)
            .map_err(|error| self.invalid_payload_error(format!("Invalid default Thanos Query URL: {error}")))
    }

    fn report_file_path(&self, extension: &str) -> Result<std::path::PathBuf, Box<EngineError>> {
        let dir = workspace_directory(&self.workspace_root_dir, self.request.id.as_str(), "cluster-analysis").map_err(
            |error| {
                Box::new(EngineError::new_cannot_get_workspace_directory(
                    self.get_event_details(ClusterAnalysisStep::Error),
                    CommandError::new(
                        "Cannot create cluster analysis workspace directory".to_string(),
                        Some(error.to_string()),
                        None,
                    ),
                ))
            },
        )?;

        Ok(dir.join(format!("report.{extension}")))
    }

    fn report_success(&self, report: String) {
        let running_step = match &self.request.target_analysis {
            ClusterAnalysisRequest::CostRecommendation(_) => ClusterAnalysisStep::CostRecommendation,
            ClusterAnalysisRequest::DeprecatedApiCheck(_) => ClusterAnalysisStep::DeprecatedApiCheck,
        };

        emit_report_lines(&report, running_step, |step, line| {
            self.logger.log(EngineEvent::Info(
                self.get_event_details(step),
                EventMessage::new_from_safe(line.to_string()),
            ));
        });
    }
}

impl Task for ClusterAnalysisTask {
    fn id(&self) -> &str {
        self.request.id.as_str()
    }

    fn run(&self) {
        engine_task::enable_log_file_writer(&self.info_context(), &self.log_file_writer);
        let _span = self.span.enter();

        self.logger.log(EngineEvent::Info(
            self.get_event_details(ClusterAnalysisStep::Start),
            EventMessage::new_from_safe("Qovery Engine has started the cluster analysis".to_string()),
        ));

        let guard = scopeguard::guard((), |_| {
            hack::remove_gke_gcloud_auth_plugin_cache();
            self.logger.log(EngineEvent::Info(
                self.get_event_details(ClusterAnalysisStep::Terminated),
                EventMessage::new_from_safe("Qovery Engine has terminated the cluster analysis".to_string()),
            ));
            if let Some(is_terminated_tx) = self.is_terminated.0.write().unwrap().take() {
                let _ = is_terminated_tx.send(());
            }
        });

        match self.run_analysis() {
            Ok(report) => self.report_success(report),
            Err(error) => self.logger.log(EngineEvent::Error(
                error.clone_engine_error_with_stage(Stage::ClusterAnalysis(ClusterAnalysisStep::Error)),
                Some(EventMessage::new_from_safe("Cluster analysis failed".to_string())),
            )),
        }

        drop(guard);
        engine_task::disable_log_file_writer(&self.log_file_writer);
    }

    fn cancel(&self, force_requested: bool) -> bool {
        self.cancel_requested.store(
            match force_requested {
                true => AbortStatus::UserForceRequested,
                false => AbortStatus::Requested,
            },
            Ordering::Relaxed,
        );
        true
    }

    fn cancel_checker(&self) -> Box<dyn Abort> {
        let cancel_requested = self.cancel_requested.clone();
        Box::new(move || cancel_requested.load(Ordering::Relaxed))
    }

    fn is_terminated(&self) -> bool {
        self.is_terminated.0.read().map(|tx| tx.is_none()).unwrap_or(true)
    }

    fn await_terminated(&self) -> broadcast::Receiver<()> {
        self.is_terminated.1.resubscribe()
    }

    fn info_context(&self) -> Context {
        Context::new(
            self.request.organization_long_id,
            self.request.kubernetes.long_id,
            self.request.id.to_string(),
            self.workspace_root_dir.to_string(),
            self.lib_root_dir.to_string(),
            self.engine_version.clone(),
            self.request.test_cluster,
            self.request.features.clone(),
            self.request.metadata.clone(),
            self.aws_apn_id.clone(),
            self.docker.clone(),
            self.qovery_api.clone(),
            self.request.event_details(),
        )
    }
}

fn krr_output_format(format: AnalysisOutputFormat) -> KrrOutputFormat {
    match format {
        AnalysisOutputFormat::Table => KrrOutputFormat::Table,
        AnalysisOutputFormat::Json => KrrOutputFormat::Json,
        AnalysisOutputFormat::Csv => KrrOutputFormat::Csv,
    }
}

fn emit_report_lines(report: &str, running_step: ClusterAnalysisStep, mut emit: impl FnMut(ClusterAnalysisStep, &str)) {
    let mut lines = report.lines().peekable();
    if lines.peek().is_none() {
        emit(ClusterAnalysisStep::Succeeded, "");
        return;
    }

    while let Some(line) = lines.next() {
        let step = if lines.peek().is_none() {
            ClusterAnalysisStep::Succeeded
        } else {
            running_step.clone()
        };
        emit(step, line);
    }
}

fn is_qovery_obs_enabled(kubernetes_options: &Value) -> bool {
    kubernetes_options
        .get("metrics_parameters")
        .filter(|value| !value.is_null())
        .and_then(|value| serde_json::from_value::<MetricsParameters>(value.clone()).ok())
        .is_some_and(|metrics_parameters| {
            matches!(metrics_parameters.config, MetricsConfiguration::MetricsInstalledByQovery { .. })
        })
}

fn qovery_obs_namespace(kind: kubernetes::Kind) -> Option<&'static str> {
    match kind {
        kubernetes::Kind::Gke
        | kubernetes::Kind::GkeSelfManaged
        | kubernetes::Kind::Aks
        | kubernetes::Kind::AksSelfManaged => Some("qovery"),
        kubernetes::Kind::Eks
        | kubernetes::Kind::EksSelfManaged
        | kubernetes::Kind::EksAnywhere
        | kubernetes::Kind::ScwKapsule
        | kubernetes::Kind::ScwSelfManaged => Some("prometheus"),
        kubernetes::Kind::OnPremiseSelfManaged => None,
    }
}

#[derive(Serialize)]
struct DeprecatedApiReportItem<'a> {
    name: Option<&'a str>,
    namespace: Option<&'a str>,
    kind: Option<&'a str>,
    api_version: Option<&'a str>,
    rule_set: Option<&'a str>,
    replace_with: Option<&'a str>,
    since: Option<String>,
    qovery_service_id: Option<String>,
    qovery_environment_id: Option<String>,
    qovery_project_id: Option<String>,
    qovery_service_type: Option<&'a str>,
}

fn format_deprecations(deprecations: &[Deprecation], output_format: AnalysisOutputFormat) -> String {
    match output_format {
        AnalysisOutputFormat::Json => serde_json::to_string_pretty(&to_report_items(deprecations))
            .unwrap_or_else(|error| format!("Cannot serialize deprecated API report: {error}")),
        AnalysisOutputFormat::Csv => format_deprecations_csv(deprecations),
        AnalysisOutputFormat::Table => format_deprecations_table(deprecations),
    }
}

fn to_report_items(deprecations: &[Deprecation]) -> Vec<DeprecatedApiReportItem<'_>> {
    deprecations
        .iter()
        .map(|deprecation| DeprecatedApiReportItem {
            name: deprecation.name.as_deref(),
            namespace: deprecation.namespace.as_deref(),
            kind: deprecation.kind.as_deref(),
            api_version: deprecation.api_version.as_deref(),
            rule_set: deprecation.rule_set.as_deref(),
            replace_with: deprecation.replace_with.as_deref(),
            since: deprecation.since.as_ref().map(ToString::to_string),
            qovery_service_id: deprecation
                .qovery_metadata
                .as_ref()
                .and_then(|metadata| metadata.qovery_service_id.as_ref())
                .map(|id| id.to_string()),
            qovery_environment_id: deprecation
                .qovery_metadata
                .as_ref()
                .and_then(|metadata| metadata.qovery_environment_id.as_ref())
                .map(|id| id.to_string()),
            qovery_project_id: deprecation
                .qovery_metadata
                .as_ref()
                .and_then(|metadata| metadata.qovery_project_id.as_ref())
                .map(|id| id.to_string()),
            qovery_service_type: deprecation
                .qovery_metadata
                .as_ref()
                .and_then(|metadata| metadata.qovery_service_type.as_deref()),
        })
        .collect()
}

fn format_deprecations_table(deprecations: &[Deprecation]) -> String {
    if deprecations.is_empty() {
        return "No deprecated Kubernetes APIs found.".to_string();
    }

    deprecations
        .iter()
        .enumerate()
        .map(|(idx, deprecation)| format!("{}. {}", idx + 1, deprecation))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_deprecations_csv(deprecations: &[Deprecation]) -> String {
    let mut report = String::from("name,namespace,kind,api_version,rule_set,replace_with,since\n");
    for item in to_report_items(deprecations) {
        let _ = writeln!(
            report,
            "{},{},{},{},{},{},{}",
            csv_escape(item.name),
            csv_escape(item.namespace),
            csv_escape(item.kind),
            csv_escape(item.api_version),
            csv_escape(item.rule_set),
            csv_escape(item.replace_with),
            csv_escape(item.since.as_deref()),
        );
    }
    report
}

fn csv_escape(value: Option<&str>) -> String {
    match value {
        Some(value) if value.contains([',', '"', '\n']) => format!("\"{}\"", value.replace('"', "\"\"")),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{emit_report_lines, is_qovery_obs_enabled, qovery_obs_namespace};
    use crate::events::ClusterAnalysisStep;
    use crate::infrastructure::models::kubernetes;
    use serde_json::json;

    #[test]
    fn marks_only_the_last_line_as_succeeded_for_a_multi_batch_report() {
        const REPORT_LINE_COUNT: usize = 4 * 1024 + 1;
        let report = (0..REPORT_LINE_COUNT)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut emitted = Vec::with_capacity(REPORT_LINE_COUNT);

        emit_report_lines(&report, ClusterAnalysisStep::CostRecommendation, |step, line| {
            emitted.push((step, line.to_string()));
        });

        assert_eq!(emitted.len(), REPORT_LINE_COUNT);
        assert!(
            emitted[..REPORT_LINE_COUNT - 1]
                .iter()
                .all(|(step, _)| *step == ClusterAnalysisStep::CostRecommendation)
        );
        assert_eq!(
            emitted.last(),
            Some(&(ClusterAnalysisStep::Succeeded, format!("line-{}", REPORT_LINE_COUNT - 1)))
        );
    }

    #[test]
    fn emits_a_terminal_event_for_an_empty_report() {
        let mut emitted = Vec::new();

        emit_report_lines("", ClusterAnalysisStep::CostRecommendation, |step, line| {
            emitted.push((step, line.to_string()));
        });

        assert_eq!(emitted, vec![(ClusterAnalysisStep::Succeeded, String::new())]);
    }

    #[test]
    fn detects_qovery_obs_metrics_parameters() {
        let options = json!({
            "metrics_parameters": {
                "config": {
                    "metrics_installed_by_qovery": {
                        "install_prometheus_adapter": false,
                        "enable_redundancy": null,
                        "beyla_config": null,
                        "alert_config": null
                    }
                }
            }
        });

        assert!(is_qovery_obs_enabled(&options));
    }

    #[test]
    fn rejects_missing_metrics_parameters_as_obs_disabled() {
        assert!(!is_qovery_obs_enabled(&json!({})));
        assert!(!is_qovery_obs_enabled(&json!({ "metrics_parameters": null })));
    }

    #[test]
    fn rejects_non_qovery_metrics_parameters_for_default_url() {
        let options = json!({
            "metrics_parameters": {
                "config": {
                    "aws_s3": {
                        "region": "eu-west-3",
                        "bucket_name": "metrics",
                        "aws_iam_prometheus_role_arn": "arn",
                        "endpoint": "https://aps-workspaces.eu-west-3.amazonaws.com"
                    }
                }
            }
        });

        assert!(!is_qovery_obs_enabled(&options));
    }

    #[test]
    fn resolves_qovery_obs_namespace_by_kubernetes_kind() {
        assert_eq!(qovery_obs_namespace(kubernetes::Kind::Eks), Some("prometheus"));
        assert_eq!(qovery_obs_namespace(kubernetes::Kind::EksAnywhere), Some("prometheus"));
        assert_eq!(qovery_obs_namespace(kubernetes::Kind::ScwKapsule), Some("prometheus"));
        assert_eq!(qovery_obs_namespace(kubernetes::Kind::Gke), Some("qovery"));
        assert_eq!(qovery_obs_namespace(kubernetes::Kind::Aks), Some("qovery"));
        assert_eq!(qovery_obs_namespace(kubernetes::Kind::OnPremiseSelfManaged), None);
    }
}
