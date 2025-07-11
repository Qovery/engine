use crate::cmd::helm::{Helm, HelmError};
use crate::cmd::helm_utils::{
    BackupStatus, CRDSUpdate, apply_chart_backup, delete_unused_chart_backup, prepare_chart_backup_on_upgrade,
    update_crds_on_upgrade,
};
use crate::cmd::kubectl::{
    kubectl_delete_crash_looping_pods, kubectl_exec_delete_crd, kubectl_exec_get_events, kubectl_update_crd,
};
use crate::errors::{CommandError, EngineError};
use crate::helm::HelmAction::Deploy;
use crate::io_models::engine_location::EngineLocation;

use semver::Version;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::cmd::command::CommandKiller;
use crate::environment::action::deploy_helm::default_helm_timeout;
use crate::events::EventDetails;
use crate::infrastructure::helm_charts::{HelmChartDirectoryLocation, HelmPath, HelmPathType};
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use retry::OperationResult;
use retry::delay::Fixed;
use std::fs;

#[derive(Error, Debug, Clone)]
pub enum HelmChartError {
    #[error("Error while creating template: {chart_name:?}: {msg:?}")]
    CreateTemplateError { chart_name: String, msg: String },

    #[error("Error while rendering template: {chart_name:?}: {msg:?}")]
    RenderingError { chart_name: String, msg: String },

    #[error("Error while executing helm command")]
    HelmError(#[from] HelmError),

    #[error("Error while executing command")]
    CommandError(#[from] CommandError),

    #[error("Error while trying to update CRDs for `{crd_path}`")]
    CannotUpdateCrds { crd_path: String },
}

impl<E> From<(EventDetails, E)> for Box<EngineError>
where
    HelmChartError: From<E>,
{
    fn from((event, err): (EventDetails, E)) -> Self {
        Box::new(EngineError::new_helm_chart_error(event, HelmChartError::from(err)))
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HelmAction {
    Deploy,
    Destroy,
}

#[derive(Copy, Clone, Debug)]
pub enum HelmChartNamespaces {
    KubeSystem,
    Prometheus,
    Logging,
    CertManager,
    NginxIngress,
    Qovery,
    Custom,
}

impl Display for HelmChartNamespaces {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            HelmChartNamespaces::Custom => "custom",
            HelmChartNamespaces::KubeSystem => "kube-system",
            HelmChartNamespaces::Prometheus => "prometheus",
            HelmChartNamespaces::Logging => "logging",
            HelmChartNamespaces::CertManager => "cert-manager",
            HelmChartNamespaces::NginxIngress => "nginx-ingress",
            HelmChartNamespaces::Qovery => "qovery",
        };

        f.write_str(str)
    }
}

pub enum UpdateStrategy {
    RollingUpdate,
    Recreate,
}

impl Display for UpdateStrategy {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            UpdateStrategy::RollingUpdate => "RollingUpdate",
            UpdateStrategy::Recreate => "Recreate",
        };
        f.write_str(str)
    }
}

#[derive(Clone, Debug)]
pub struct ChartSetValue {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct ChartValuesGenerated {
    pub filename: String,
    pub yaml_content: String,
}

impl ChartValuesGenerated {
    pub fn new(name: String, yaml_content: String) -> Self {
        ChartValuesGenerated {
            filename: format!("{name}_override.yaml"),
            yaml_content,
        }
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum QoveryPriorityClass {
    HighPriority,
    StandardPriority,
}

impl Display for QoveryPriorityClass {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            QoveryPriorityClass::HighPriority => "qovery-high-priority",
            QoveryPriorityClass::StandardPriority => "qovery-standard-priority",
        })
    }
}

pub enum PriorityClass {
    Default,
    Qovery(QoveryPriorityClass),
}

#[derive(Clone, Debug)]
pub struct VpaConfig {
    pub target_ref: VpaTargetRef,
    pub container_policy: VpaContainerPolicy,
}

#[derive(Clone, Debug)]
pub struct VpaTargetRef {
    pub api_version: VpaTargetRefApiVersion,
    pub kind: VpaTargetRefKind,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum VpaTargetRefKind {
    Deployment,
    StatefulSet,
    DaemonSet,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum VpaTargetRefApiVersion {
    AppsV1,
}

impl Display for VpaTargetRefApiVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            VpaTargetRefApiVersion::AppsV1 => "apps/v1",
        };
        f.write_str(str)
    }
}

impl VpaTargetRef {
    pub fn new(api_version: VpaTargetRefApiVersion, kind: VpaTargetRefKind, name: String) -> Self {
        VpaTargetRef {
            api_version,
            kind,
            name,
        }
    }
}

/// VpaContainerPolicy: holding VPA container policy
///
/// Note: GKE autopilot wants memory to cpu ratio to be in the [1, 6.5] range
/// as a good practice for bin packing, let's do this for all providers
/// https://cloud.google.com/kubernetes-engine/docs/concepts/autopilot-resource-requests
#[derive(Clone, Debug)]
pub struct VpaContainerPolicy {
    pub name: String,
    pub min_allowed_cpu: Option<KubernetesCpuResourceUnit>,
    pub max_allowed_cpu: Option<KubernetesCpuResourceUnit>,
    pub min_allowed_memory: Option<KubernetesMemoryResourceUnit>,
    pub max_allowed_memory: Option<KubernetesMemoryResourceUnit>,
}

impl VpaContainerPolicy {
    pub fn new(
        name: String,
        min_allowed_cpu: Option<KubernetesCpuResourceUnit>,
        max_allowed_cpu: Option<KubernetesCpuResourceUnit>,
        min_allowed_memory: Option<KubernetesMemoryResourceUnit>,
        max_allowed_memory: Option<KubernetesMemoryResourceUnit>,
    ) -> Self {
        VpaContainerPolicy {
            name,
            min_allowed_cpu,
            max_allowed_cpu,
            min_allowed_memory,
            max_allowed_memory,
        }
    }
}

impl VpaConfig {
    pub fn new(target_ref: VpaTargetRef, container_policy: VpaContainerPolicy) -> Self {
        // TODO(benjaminch): make it a try_new and return an error if memory to cpu ratio is not in the [1, 6.5] range
        VpaConfig {
            target_ref,
            container_policy,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VpaConfigHelmChart {
    pub target_ref_name: String,
    pub target_ref_api_version: String,
    pub target_ref_kind: VpaTargetRefKind,
    pub container_name: String,
    pub min_allowed_cpu: Option<String>,
    pub min_allowed_memory: Option<String>,
    pub max_allowed_cpu: Option<String>,
    pub max_allowed_memory: Option<String>,
    pub controlled_resources: Vec<VpaControllerResources>,
}

impl VpaConfigHelmChart {
    pub fn new(vpa_config: VpaConfig) -> Self {
        let mut controlled_resources = Vec::with_capacity(2);
        if vpa_config.container_policy.min_allowed_cpu.is_some()
            || vpa_config.container_policy.max_allowed_cpu.is_some()
        {
            controlled_resources.push(VpaControllerResources::Cpu);
        }
        if vpa_config.container_policy.min_allowed_memory.is_some()
            || vpa_config.container_policy.max_allowed_memory.is_some()
        {
            controlled_resources.push(VpaControllerResources::Memory);
        }

        VpaConfigHelmChart {
            target_ref_name: vpa_config.target_ref.name,
            target_ref_api_version: vpa_config.target_ref.api_version.to_string(),
            target_ref_kind: vpa_config.target_ref.kind,
            container_name: vpa_config.container_policy.name,
            min_allowed_cpu: vpa_config.container_policy.min_allowed_cpu.map(|x| x.to_string()),
            min_allowed_memory: vpa_config.container_policy.min_allowed_memory.map(|x| x.to_string()),
            max_allowed_cpu: vpa_config.container_policy.max_allowed_cpu.map(|x| x.to_string()),
            max_allowed_memory: vpa_config.container_policy.max_allowed_memory.map(|x| x.to_string()),
            controlled_resources,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum VpaControllerResources {
    Cpu,
    Memory,
}

impl Display for VpaControllerResources {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            VpaControllerResources::Cpu => "cpu",
            VpaControllerResources::Memory => "memory",
        };
        f.write_str(str)
    }
}

#[derive(Clone, Debug)]
pub struct ChartInfoUpgradeRetry {
    pub nb_retry: usize,
    pub delay_in_milli_sec: u64,
}

#[derive(Clone, Debug)]
pub struct ChartInfo {
    pub name: String,
    pub path: String,
    pub namespace: HelmChartNamespaces,
    pub custom_namespace: Option<String>,
    pub action: HelmAction,
    pub atomic: bool,
    pub force_upgrade: bool,
    pub recreate_pods: bool,
    pub reinstall_chart_if_installed_version_is_below_than: Option<Version>,
    pub timeout_in_seconds: i64,
    pub dry_run: bool,
    pub wait: bool,
    /// Values used to override values set inside values files.
    pub values: Vec<ChartSetValue>,
    pub values_string: Vec<ChartSetValue>,
    pub values_files: Vec<String>,
    pub yaml_files_content: Vec<ChartValuesGenerated>,
    pub parse_stderr_for_error: bool,
    pub k8s_selector: Option<String>,
    pub backup_resources: Option<Vec<String>>,
    pub crds_update: Option<CRDSUpdate>,
    pub skip_if_already_installed: bool,
    pub upgrade_retry: Option<ChartInfoUpgradeRetry>,
}

impl ChartInfo {
    pub fn new_from_custom_namespace(
        name: String,
        path: String,
        custom_namespace: String,
        timeout_in_seconds: i64,
        values_files: Vec<String>,
        values: Vec<ChartSetValue>,
        values_string: Vec<ChartSetValue>,
        parse_stderr_for_error: bool,
        k8s_selector: Option<String>,
    ) -> Self {
        ChartInfo {
            name,
            path,
            namespace: HelmChartNamespaces::Custom,
            custom_namespace: Some(custom_namespace),
            timeout_in_seconds,
            values_files,
            values,
            values_string,
            parse_stderr_for_error,
            k8s_selector,
            ..Default::default()
        }
    }

    pub fn new_from_release_name(name: &str, custom_namespace: &str) -> ChartInfo {
        ChartInfo {
            name: name.to_string(),
            namespace: HelmChartNamespaces::Custom,
            custom_namespace: Some(custom_namespace.to_string()),
            ..Default::default()
        }
    }

    pub fn get_namespace_string(&self) -> String {
        match self.namespace {
            HelmChartNamespaces::Custom => self
                .custom_namespace
                .clone()
                .unwrap_or_else(|| self.namespace.to_string()),
            _ => self.namespace.to_string(),
        }
    }

    pub fn generate_vpa_helm_config(vpa_configs: Vec<VpaConfig>) -> String {
        let vpa_helm_config = vpa_configs
            .iter()
            .map(|x| VpaConfigHelmChart::new(x.clone()))
            .collect::<Vec<VpaConfigHelmChart>>();

        format!(
            "vpa_config:\n{}",
            // this theorically can't fail
            serde_yaml::to_string(&vpa_helm_config).expect("couldn't serialize VPA helm config")
        )
    }
}

impl Default for ChartInfo {
    fn default() -> ChartInfo {
        ChartInfo {
            name: "undefined".to_string(),
            path: "undefined".to_string(),
            namespace: HelmChartNamespaces::KubeSystem,
            custom_namespace: None,
            action: Deploy,
            atomic: true,
            force_upgrade: false,
            recreate_pods: false,
            reinstall_chart_if_installed_version_is_below_than: None,
            timeout_in_seconds: default_helm_timeout().as_secs() as i64,
            dry_run: false,
            wait: true,
            values: vec![],
            values_string: vec![], // values to force string usage
            values_files: vec![],
            yaml_files_content: vec![],
            parse_stderr_for_error: true,
            k8s_selector: None,
            backup_resources: None,
            crds_update: None,
            skip_if_already_installed: false,
            upgrade_retry: None,
        }
    }
}

pub trait HelmChart: Send {
    fn clone_dyn(&self) -> Box<dyn HelmChart>;

    fn check_prerequisites(&self) -> Result<Option<ChartPayload>, HelmChartError> {
        let chart = self.get_chart_info();
        for file in chart.values_files.iter() {
            if let Err(e) = fs::metadata(file) {
                return Err(HelmChartError::CommandError(CommandError::new(
                    format!("Can't access helm chart override file `{}` for chart `{}`", file, chart.name,),
                    Some(e.to_string()),
                    None,
                )));
            }
        }
        Ok(None)
    }

    fn get_selector(&self) -> &Option<String> {
        &self.get_chart_info().k8s_selector
    }

    fn get_chart_info(&self) -> &ChartInfo;

    fn namespace(&self) -> String {
        self.get_chart_info().get_namespace_string()
    }

    fn pre_exec(
        &self,
        kube_client: &kube::Client,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
        _cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        // Cleaning any existing crash looping pod for this helm chart
        if let Some(selector) = self.get_selector() {
            kubectl_delete_crash_looping_pods(
                kubernetes_config,
                Some(self.get_chart_info().get_namespace_string().as_str()),
                Some(selector.as_str()),
                envs.to_vec(),
            )?;
        }

        // Force install CRDs if needed
        let chart_info = &self.get_chart_info();
        match chart_info.action {
            Deploy => {
                if let Some(crds_update) = &chart_info.crds_update {
                    if let Err(_e) =
                        kubectl_update_crd(kube_client, chart_info.name.as_str(), crds_update.path.as_str())
                    {
                        return Err(HelmChartError::CannotUpdateCrds {
                            crd_path: crds_update.path.clone(),
                        });
                    }
                }
            }
            HelmAction::Destroy => {}
        }

        Ok(payload)
    }

    fn run(
        &self,
        kube_client: &kube::Client,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        info!("prepare and deploy chart {}", &self.get_chart_info().name);
        let payload = self.check_prerequisites()?;
        let payload = self.pre_exec(kube_client, kubernetes_config, envs, payload, cmd_killer)?;
        let payload = match self.exec(kubernetes_config, envs, payload.clone(), cmd_killer) {
            Ok(payload) => payload,
            Err(e) => {
                error!("Error while deploying chart: {:?}", e);
                self.on_deploy_failure(kubernetes_config, envs, payload)?;
                return Err(e);
            }
        };
        let payload = self.post_exec(kube_client, kubernetes_config, envs, payload, cmd_killer)?;
        Ok(payload)
    }

    fn exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
        cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        let chart_info = self.get_chart_info();
        let helm = Helm::new(Some(kubernetes_config), envs)?;

        match chart_info.action {
            Deploy => {
                if let Err(e) = helm.uninstall_chart_if_breaking_version(chart_info, &[]) {
                    warn!(
                        "error while trying to destroy chart if breaking change is detected: {:?}",
                        e.to_string()
                    );
                }

                let installed_chart_version = match helm.get_chart_version(
                    &chart_info.name,
                    Some(chart_info.get_namespace_string().as_str()),
                    envs,
                ) {
                    Ok(versions) => match versions {
                        None => None,
                        Some(versions) => versions.chart_version,
                    },
                    Err(e) => {
                        warn!("error while trying to get installed version: {:?}", e);
                        None
                    }
                };

                // Allow to skip the deployment if the chart is already installed
                if chart_info.skip_if_already_installed && installed_chart_version.is_some() {
                    info!(
                        "chart {} is already installed and option to skip it if installed is set to true, skipping",
                        &chart_info.name
                    );
                    return Ok(payload);
                }

                let upgrade_status = prepare_chart_backup_on_upgrade(
                    kubernetes_config,
                    chart_info.clone(),
                    envs,
                    installed_chart_version,
                )
                .unwrap_or_else(|e| {
                    warn!("error while trying to prepare backup: {:?}", e);
                    BackupStatus {
                        is_backupable: false,
                        backup_path: PathBuf::new(),
                    }
                });

                // Verify that we don't need to upgrade the CRDS
                update_crds_on_upgrade(kubernetes_config, chart_info.clone(), envs, &helm)?;

                let attempts = if let Some(upgrade_retry) = &chart_info.upgrade_retry {
                    Fixed::from_millis(upgrade_retry.delay_in_milli_sec).take(upgrade_retry.nb_retry)
                } else {
                    Fixed::from_millis(0).take(0)
                };
                let result = retry::retry(attempts, || match helm.upgrade(chart_info, &[], cmd_killer) {
                    Ok(_) => OperationResult::Ok(()),
                    Err(e) => {
                        warn!("Helm upgrade failed, retrying... error: {:?}", e);
                        OperationResult::Retry(e)
                    }
                });
                match result {
                    Ok(_) => {
                        if upgrade_status.is_backupable {
                            if let Err(e) = apply_chart_backup(
                                kubernetes_config,
                                upgrade_status.backup_path.as_path(),
                                envs,
                                chart_info,
                            ) {
                                warn!("error while trying to apply backup: {:?}", e);
                            };
                        }
                    }
                    Err(e) => {
                        if upgrade_status.is_backupable {
                            if let Err(e) = delete_unused_chart_backup(kubernetes_config, envs, chart_info) {
                                warn!("error while trying to delete backup: {:?}", e);
                            }
                        }

                        return Err(HelmChartError::HelmError(e.error));
                    }
                };
            }
            HelmAction::Destroy => {
                let chart_info = self.get_chart_info();
                if let Some(crds_update) = &chart_info.crds_update {
                    // FIXME: This can't work as crd as .yaml in the string
                    for crd in &crds_update.resources {
                        if let Err(e) = kubectl_exec_delete_crd(kubernetes_config, crd.as_str(), envs.to_vec()) {
                            warn!("error while trying to delete crd {}: {:?}", crd, e);
                        }
                    }
                }

                // uninstall current chart
                helm.uninstall(chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {})?;
            }
        }
        Ok(payload)
    }

    fn post_exec(
        &self,
        _kube_client: &kube::Client,
        _kubernetes_config: &Path,
        _envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
        _cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        Ok(payload)
    }

    fn on_deploy_failure(
        &self,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        // print events for future investigation
        match kubectl_exec_get_events(
            kubernetes_config,
            Some(self.get_chart_info().get_namespace_string().as_str()),
            envs.to_vec(),
        ) {
            Ok(ok_line) => info!("{}", ok_line),
            Err(err) => {
                error!("{:?}", err);
                return Err(err);
            }
        };
        Ok(payload)
    }
}

impl Clone for Box<dyn HelmChart> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

#[derive(Serialize, Deserialize)]
pub struct ChartReleaseData {
    pub name: String,
    pub chart: ChartReleaseContent,
    pub manifest: String,
    pub version: u32,
    pub namespace: String,
}

#[derive(Serialize, Deserialize)]
pub struct ChartReleaseContent {
    pub metadata: ChartReleaseMetadata,
    pub templates: Vec<ChartReleaseTemplate>,
}

#[derive(Serialize, Deserialize)]
pub struct ChartReleaseMetadata {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize)]
pub struct ChartReleaseTemplate {
    pub name: String,
    pub data: String,
}

//
// Common charts
//

pub trait ChartInstallationChecker: Send {
    fn verify_installation(&self, kube_client: &kube::Client) -> Result<(), CommandError>;
    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker>;
}

impl Clone for Box<dyn ChartInstallationChecker> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

#[derive(Default, Clone)]
pub struct CommonChartVpa {
    pub helm_path: HelmPath,
    pub vpa: Vec<VpaConfig>,
}

impl CommonChartVpa {
    pub fn new(chart_prefix: String, vpa: Vec<VpaConfig>) -> Self {
        let helm_path = HelmPath::new(
            HelmPathType::Chart,
            Some(chart_prefix.as_str()),
            HelmChartDirectoryLocation::CommonFolder,
            "vertical-pod-autoscaler-configs".to_string(),
        );
        CommonChartVpa { helm_path, vpa }
    }
}

#[derive(Default, Clone)]
pub struct CommonChart {
    pub chart_info: ChartInfo,
    pub chart_installation_checker: Option<Box<dyn ChartInstallationChecker>>,
    pub vertical_pod_autoscaler: Option<CommonChartVpa>,
}

impl CommonChart {
    pub fn new(
        chart_info: ChartInfo,
        chart_installation_checker: Option<Box<dyn ChartInstallationChecker>>,
        vertical_pod_autoscaler: Option<CommonChartVpa>,
    ) -> Self {
        CommonChart {
            chart_info,
            chart_installation_checker,
            vertical_pod_autoscaler,
        }
    }

    fn get_vpa_chart_info(&self, vpa_config: Option<CommonChartVpa>) -> ChartInfo {
        let current_chart = self.get_chart_info();
        let chart_name = format!("vpa-{}", current_chart.name);
        ChartInfo {
            name: chart_name.clone(),
            path: match vpa_config.clone() {
                Some(x) => x.helm_path.to_string(),
                None => ".".to_string(),
            },
            action: match vpa_config {
                Some(_) => Deploy,
                None => HelmAction::Destroy,
            },
            namespace: current_chart.namespace,
            custom_namespace: current_chart.custom_namespace.clone(),
            yaml_files_content: match vpa_config {
                Some(config) => vec![ChartValuesGenerated::new(
                    chart_name,
                    ChartInfo::generate_vpa_helm_config(config.vpa),
                )],
                None => vec![],
            },
            timeout_in_seconds: 15,
            ..Default::default()
        }
    }
}

#[derive(Clone)]
pub struct ServiceChart {
    pub chart_info: ChartInfo,
    helm: Helm,
}

impl ServiceChart {
    pub fn new(helm: Helm, chart_info: ChartInfo) -> Self {
        ServiceChart { chart_info, helm }
    }
}

/// using ChartPayload to pass random kind of data between each deployment steps against a chart deployment
#[derive(Clone)]
pub struct ChartPayload {
    data: HashMap<String, String>,
}

impl ChartPayload {
    pub fn new(data: HashMap<String, String>) -> ChartPayload {
        ChartPayload { data }
    }

    pub fn data(&self) -> &HashMap<String, String> {
        &self.data
    }
}

impl HelmChart for CommonChart {
    fn clone_dyn(&self) -> Box<dyn HelmChart> {
        Box::new(self.clone())
    }

    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    fn pre_exec(
        &self,
        kube_client: &kube::Client,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
        cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        // Cleaning any existing crash looping pod for this helm chart
        if let Some(selector) = self.get_selector() {
            kubectl_delete_crash_looping_pods(
                kubernetes_config,
                Some(self.get_chart_info().get_namespace_string().as_str()),
                Some(selector.as_str()),
                envs.to_vec(),
            )?;
        }

        // Force install CRDs if needed
        let chart_info = &self.get_chart_info();
        match chart_info.action {
            Deploy => {
                if let Some(crds_update) = &chart_info.crds_update {
                    if let Err(_e) =
                        kubectl_update_crd(kube_client, chart_info.name.as_str(), crds_update.path.as_str())
                    {
                        return Err(HelmChartError::CannotUpdateCrds {
                            crd_path: crds_update.path.clone(),
                        });
                    }
                }
            }
            HelmAction::Destroy => {}
        }

        let helm = Helm::new(Some(kubernetes_config), envs)?;

        // deploy VPA if exists
        let vpa_chart = match &self.vertical_pod_autoscaler {
            Some(vpa) => self.get_vpa_chart_info(Some(vpa.clone())),
            None => self.get_vpa_chart_info(None),
        };
        warn!("VPA CHART ++++++++++++++++++++++++++++++++ {:?}", &vpa_chart);
        // Deploy VPA only if both VPA and parent are in Deploy mode
        if let (Deploy, Deploy) = (&chart_info.action, &vpa_chart.action) {
            warn!("UPGRADE VPA CHART ++++++++++++++++++++++++++++++++");
            helm.upgrade(&vpa_chart, &[], cmd_killer)?;
        }

        Ok(payload)
    }

    fn post_exec(
        &self,
        kube_client: &kube::Client,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
        _cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        // installation checker
        let chart_payload_res = match &self.chart_installation_checker {
            Some(checker) => match checker.verify_installation(kube_client) {
                Ok(_) => Ok(payload),
                Err(e) => Err(HelmChartError::CommandError(e)),
            },
            // If no checker set, then consider it's ok
            None => Ok(payload),
        };

        let helm = Helm::new(Some(kubernetes_config), envs)?;

        //  uninstall VPA if not wanted
        let vpa_chart = match &self.vertical_pod_autoscaler {
            Some(vpa) => self.get_vpa_chart_info(Some(vpa.clone())),
            None => self.get_vpa_chart_info(None),
        };
        warn!("VPA CHART ++++++++++++++++++++++++++++++++ {:?}", &vpa_chart);
        let chart_info = &self.get_chart_info();
        // Destroy VPA only if both VPA and parent are in Destroy mode
        if let (HelmAction::Destroy, HelmAction::Destroy) = (&chart_info.action, &vpa_chart.action) {
            warn!("DESTROY VPA CHART ++++++++++++++++++++++++++++++++");
            helm.uninstall(&vpa_chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {})?;
        }

        chart_payload_res
    }
}

impl HelmChart for ServiceChart {
    fn clone_dyn(&self) -> Box<dyn HelmChart> {
        Box::new(self.clone())
    }

    fn check_prerequisites(&self) -> Result<Option<ChartPayload>, HelmChartError> {
        Ok(None)
    }

    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    fn pre_exec(
        &self,
        kube_client: &kube::Client,
        kubernetes_config: &Path,
        envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
        _cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        // Cleaning any existing crash looping pod for this helm chart
        if let Some(selector) = self.get_selector() {
            kubectl_delete_crash_looping_pods(
                kubernetes_config,
                Some(self.get_chart_info().get_namespace_string().as_str()),
                Some(selector.as_str()),
                envs.to_vec(),
            )?;
        }

        // Force install CRDs if needed
        let chart_info = &self.get_chart_info();
        match chart_info.action {
            Deploy => {
                if let Some(crds_update) = &chart_info.crds_update {
                    if let Err(_e) =
                        kubectl_update_crd(kube_client, chart_info.name.as_str(), crds_update.path.as_str())
                    {
                        return Err(HelmChartError::CannotUpdateCrds {
                            crd_path: crds_update.path.clone(),
                        });
                    }
                }
            }
            HelmAction::Destroy => {}
        }

        Ok(payload)
    }

    fn exec(
        &self,
        _kubernetes_config: &Path,
        _envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
        cmd_killer: &CommandKiller,
    ) -> Result<Option<ChartPayload>, HelmChartError> {
        let chart_info = self.get_chart_info();
        match chart_info.action {
            Deploy => {
                let _ = self.helm.upgrade_diff(chart_info, &[], &mut |_| {});
                match self.helm.upgrade(chart_info, &[], cmd_killer) {
                    Ok(_) => {}
                    Err(e) => {
                        return Err(HelmChartError::HelmError(e));
                    }
                };
            }
            HelmAction::Destroy => {
                self.helm
                    .uninstall(chart_info, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {})?;
            }
        }
        Ok(payload)
    }

    fn on_deploy_failure(
        &self,
        _kubernetes_config: &Path,
        _envs: &[(&str, &str)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, CommandError> {
        Ok(payload)
    }
}

pub fn get_engine_helm_action_from_location(location: &EngineLocation) -> HelmAction {
    match location {
        EngineLocation::ClientSide => Deploy,
        EngineLocation::QoverySide => HelmAction::Destroy,
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{CommonChart, CommonChartVpa, VpaConfigHelmChart, VpaTargetRefApiVersion, VpaTargetRefKind};
    use crate::io_models::models::KubernetesCpuResourceUnit;
    use crate::io_models::models::KubernetesMemoryResourceUnit;

    use super::{ChartInfo, VpaConfig, VpaContainerPolicy, VpaTargetRef};

    #[test]
    fn test_vpa() {
        let vpa_config = VpaConfig {
            target_ref: VpaTargetRef {
                api_version: VpaTargetRefApiVersion::AppsV1,
                kind: VpaTargetRefKind::Deployment,
                name: "test".to_string(),
            },
            container_policy: VpaContainerPolicy {
                name: "test".to_string(),
                min_allowed_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                min_allowed_memory: Some(KubernetesMemoryResourceUnit::MebiByte(100)),
                max_allowed_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                max_allowed_memory: Some(KubernetesMemoryResourceUnit::MebiByte(100)),
            },
        };

        // install
        let chart = ChartInfo::new_from_release_name("vpa_test", "qovery");
        let common_chart_vpa = CommonChartVpa::new("./".to_string(), vec![vpa_config.clone()]);
        let common_chart = CommonChart::new(chart, None, Some(common_chart_vpa.clone()));
        let vpa_chart = common_chart.get_vpa_chart_info(Some(common_chart_vpa));

        assert_eq!(vpa_chart.name, "vpa-vpa_test".to_string());
        assert_eq!(vpa_chart.path, "./common/charts/vertical-pod-autoscaler-configs");
        assert_eq!(vpa_chart.action, super::HelmAction::Deploy);

        // test Helm generated config
        let vpa_config_string = ChartInfo::generate_vpa_helm_config(vec![vpa_config.clone()]);
        assert_eq!(vpa_config_string, "vpa_config:\n- targetRefName: test\n  targetRefApiVersion: apps/v1\n  targetRefKind: Deployment\n  containerName: test\n  minAllowedCpu: 100m\n  minAllowedMemory: 100Mi\n  maxAllowedCpu: 100m\n  maxAllowedMemory: 100Mi\n  controlledResources:\n  - cpu\n  - memory\n".to_string());

        // uninstall vpa chart if nothing is set
        let vpa_chart = common_chart.get_vpa_chart_info(None);
        assert_eq!(vpa_chart.action, super::HelmAction::Destroy);

        // check vpa all resources are deployed
        let vpa_config_all_resources = VpaConfigHelmChart::new(vpa_config);
        assert_eq!(format!("{:?}", vpa_config_all_resources.controlled_resources), "[Cpu, Memory]");

        // only vpa cpu set
        let vpa_config_no_mem = VpaConfig {
            target_ref: VpaTargetRef {
                api_version: VpaTargetRefApiVersion::AppsV1,
                kind: VpaTargetRefKind::Deployment,
                name: "test".to_string(),
            },
            container_policy: VpaContainerPolicy {
                name: "test".to_string(),
                min_allowed_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                min_allowed_memory: None,
                max_allowed_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                max_allowed_memory: None,
            },
        };
        let vpa_config = VpaConfigHelmChart::new(vpa_config_no_mem);
        assert_eq!(format!("{:?}", vpa_config.controlled_resources), "[Cpu]");

        // only vpa memory set
        let vpa_config_no_cpu = VpaConfig {
            target_ref: VpaTargetRef {
                api_version: VpaTargetRefApiVersion::AppsV1,
                kind: VpaTargetRefKind::Deployment,
                name: "test".to_string(),
            },
            container_policy: VpaContainerPolicy {
                name: "test".to_string(),
                min_allowed_cpu: None,
                min_allowed_memory: Some(KubernetesMemoryResourceUnit::MebiByte(100)),
                max_allowed_cpu: None,
                max_allowed_memory: Some(KubernetesMemoryResourceUnit::MebiByte(100)),
            },
        };
        let vpa_config = VpaConfigHelmChart::new(vpa_config_no_cpu);
        assert_eq!(format!("{:?}", vpa_config.controlled_resources), "[Memory]");
    }
}
