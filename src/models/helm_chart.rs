use crate::build_platform::Build;
use crate::cloud_provider::models::EnvironmentVariable;
use crate::cloud_provider::service::{Action, Service, ServiceType};
use crate::deployment_action::DeploymentAction;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::helm_chart::{HelmChartAdvancedSettings, HelmRepository};
use crate::models::types::CloudProvider;
use crate::utilities::to_short_id;
use std::marker::PhantomData;
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
    pub(super) repository: HelmRepository,
    pub(super) chart_name: String,
    pub(super) chart_version: String,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) advanced_settings: HelmChartAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
    pub(super) workspace_directory: String,
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
        repository: HelmRepository,
        chart_name: String,
        chart_version: String,
        environment_variables: Vec<EnvironmentVariable>,
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

        let event_details = mk_event_details(Transmitter::HelmChart(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            action,
            name,
            kube_name,
            repository,
            chart_name,
            chart_version,
            environment_variables,
            advanced_settings,
            _extra_settings: extra_settings,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
    }

    pub fn helm_selector(&self) -> Option<String> {
        Some(self.kube_label_selector())
    }

    pub fn helm_release_name(&self) -> String {
        format!("helmchart-{}", self.long_id)
    }

    pub fn repository(&self) -> &HelmRepository {
        &self.repository
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
        self.chart_version.to_string()
    }

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
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
