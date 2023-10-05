use crate::build_platform::{Build, Credentials, SshKey};
use crate::cloud_provider::service::{Action, Service, ServiceType};
use crate::deployment_action::DeploymentAction;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::helm_chart::{HelmChartAdvancedSettings, HelmCredentials, HelmRawValues};
use crate::models::types::CloudProvider;
use crate::utilities::to_short_id;
use std::borrow::Cow;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum HelmChartError {
    #[error("Container invalid configuration: {0}")]
    InvalidConfig(String),
}

// TODO (helm): Remove this when we will have a real implementation of helm chart services
#[allow(dead_code)]
pub struct HelmChart<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(super) id: String,
    pub(super) long_id: Uuid,
    pub(super) name: String,
    pub(super) kube_name: String,
    pub(super) action: Action,
    pub(super) chart_source: HelmChartSource,
    pub(super) chart_values: HelmValueSource,
    pub(super) set_values: Vec<String>,
    pub(super) set_string_values: Vec<String>,
    pub(super) set_json_values: Vec<String>,
    pub(super) command_args: Vec<String>,
    pub(super) timeout: Duration,
    pub(super) allow_cluster_wide_resources: bool,
    pub(super) environment_variables: HashMap<String, String>,
    pub(super) advanced_settings: HelmChartAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
    pub(super) workspace_directory: PathBuf,
    pub(super) chart_workspace_directory: PathBuf,
    pub(super) lib_root_directory: String,
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> HelmChart<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        mut chart_source: HelmChartSource,
        mut chart_values: HelmValueSource,
        set_values: Vec<String>,
        set_string_values: Vec<String>,
        set_json_values: Vec<String>,
        command_args: Vec<String>,
        timeout: Duration,
        allow_cluster_wide_resources: bool,
        environment_variables: HashMap<String, String>,
        advanced_settings: HelmChartAdvancedSettings,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, HelmChartError> {
        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("helm_charts/{long_id}"),
        )
        .map_err(|_| HelmChartError::InvalidConfig("Can't create workspace directory".to_string()))?;

        // Normalize paths to be relative paths in order to concat them easily
        match &mut chart_source {
            HelmChartSource::Repository { .. } => {}
            HelmChartSource::Git { ref mut root_path, .. } => {
                if root_path.is_absolute() {
                    *root_path = to_relative_path(root_path)?;
                }
            }
        }

        match &mut chart_values {
            HelmValueSource::Raw { .. } => {}
            HelmValueSource::Git {
                ref mut values_path, ..
            } => {
                for path in values_path {
                    *path = to_relative_path(path)?;
                }
            }
        }

        let event_details = mk_event_details(Transmitter::Helm(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        let workspace_directory = PathBuf::from(workspace_directory);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            action,
            name,
            kube_name,
            chart_source,
            chart_values,
            set_values,
            set_string_values,
            set_json_values,
            command_args,
            timeout,
            allow_cluster_wide_resources,
            environment_variables,
            advanced_settings,
            _extra_settings: extra_settings,
            chart_workspace_directory: workspace_directory.join("chart"),
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
    }

    pub fn helm_selector(&self) -> Option<String> {
        Some(self.kube_label_selector())
    }

    pub fn helm_release_name(&self) -> String {
        format!("qovery-{}-{}", self.id(), self.kube_name)
    }

    pub fn chart_source(&self) -> &HelmChartSource {
        &self.chart_source
    }

    pub fn chart_values(&self) -> &HelmValueSource {
        &self.chart_values
    }

    pub fn service_type(&self) -> ServiceType {
        ServiceType::HelmChart
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn action(&self) -> &Action {
        &self.action
    }

    pub fn service_version(&self) -> String {
        match &self.chart_source {
            HelmChartSource::Repository { chart_version, .. } => chart_version.clone(),
            HelmChartSource::Git { commit_id, .. } => commit_id.clone(),
        }
    }

    pub fn environment_variables(&self) -> &HashMap<String, String> {
        &self.environment_variables
    }

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub fn workspace_directory(&self) -> &Path {
        &self.workspace_directory
    }
    pub fn chart_workspace_directory(&self) -> &Path {
        &self.chart_workspace_directory
    }

    pub fn is_cluster_wide_resources_allowed(&self) -> bool {
        self.allow_cluster_wide_resources
    }

    pub fn helm_timeout(&self) -> Duration {
        self.timeout
    }

    fn helm_values_arguments(&self) -> impl Iterator<Item = Cow<'_, str>> {
        let chart_dir = self.chart_workspace_directory();
        let values: Vec<Cow<'_, str>> = match &self.chart_values {
            HelmValueSource::Raw { values, .. } => values
                .iter()
                .map(|v| Cow::from(chart_dir.join(&v.name).to_string_lossy().to_string()))
                .collect(),
            HelmValueSource::Git { values_path, .. } => values_path
                .iter()
                .map(|v| {
                    let filename = v.file_name().unwrap_or_default().to_str().unwrap_or_default();
                    Cow::from(chart_dir.join(filename).to_string_lossy().to_string())
                })
                .collect(),
        };

        values
            .into_iter()
            .flat_map(|v| [Cow::from("--values"), v])
            .chain(
                self.set_values
                    .iter()
                    .flat_map(|v| [Cow::from("--set"), Cow::from(v.as_str())]),
            )
            .chain(
                self.set_string_values
                    .iter()
                    .flat_map(|v| [Cow::from("--set-string"), Cow::from(v.as_str())]),
            )
            .chain(
                self.set_json_values
                    .iter()
                    .flat_map(|v| [Cow::from("--set-json"), Cow::from(v.as_str())]),
            )
    }

    pub fn helm_template_arguments(&self) -> impl Iterator<Item = Cow<'_, str>> {
        self.helm_values_arguments()
    }

    pub fn helm_upgrade_arguments(&self) -> impl Iterator<Item = Cow<'_, str>> {
        self.helm_values_arguments()
            .chain([
                Cow::from("--timeout"),
                Cow::from(format!("{}s", self.timeout.as_secs())),
            ])
            .chain(self.command_args.iter().map(|v| Cow::from(v.as_str())))
    }
}

impl<T: CloudProvider> Service for HelmChart<T> {
    fn service_type(&self) -> ServiceType {
        self.service_type()
    }

    fn id(&self) -> &str {
        self.id()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name()
    }

    fn version(&self) -> String {
        self.service_version()
    }

    fn kube_name(&self) -> &str {
        &self.kube_name
    }

    fn kube_label_selector(&self) -> String {
        self.kube_label_selector()
    }

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        self.action()
    }

    fn as_service(&self) -> &dyn Service {
        self
    }

    fn as_service_mut(&mut self) -> &mut dyn Service {
        self
    }

    fn build(&self) -> Option<&Build> {
        None
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        None
    }
}

pub trait HelmChartService: Service + DeploymentAction + Send {
    fn as_deployment_action(&self) -> &dyn DeploymentAction;
}

impl<T: CloudProvider> HelmChartService for HelmChart<T>
where
    HelmChart<T>: Service + DeploymentAction,
{
    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }
}

pub enum HelmChartSource {
    Repository {
        url: Url,
        credentials: Option<HelmCredentials>,
        skip_tls_verify: bool,
        chart_name: String,
        chart_version: String,
    },
    Git {
        git_url: Url,
        get_credentials: Box<dyn Fn() -> anyhow::Result<Option<Credentials>> + Send + Sync>,
        commit_id: String,
        root_path: PathBuf,
        ssh_keys: Vec<SshKey>,
    },
}

pub enum HelmValueSource {
    Raw {
        values: Vec<HelmRawValues>,
    },
    Git {
        git_url: Url,
        get_credentials: Box<dyn Fn() -> anyhow::Result<Option<Credentials>> + Send + Sync>,
        commit_id: String,
        values_path: Vec<PathBuf>,
        ssh_keys: Vec<SshKey>,
    },
}

fn to_relative_path(path: &Path) -> Result<PathBuf, HelmChartError> {
    if path.is_relative() {
        return Ok(path.to_path_buf());
    }
    Ok(path
        .strip_prefix("/")
        .map_err(|err| HelmChartError::InvalidConfig(format!("Can't convert to relative path: {path:?} {err}")))?
        .to_path_buf())
}
