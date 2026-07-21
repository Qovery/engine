use crate::environment::action::DeploymentAction;
use crate::environment::models::types::ToTeraContext;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::Build;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType};
use crate::io_models::context::Context;
use crate::io_models::models::EnvironmentVariable;
use crate::utilities::to_short_id;
use serde::Serialize;
use std::path::PathBuf;
use tera::Context as TeraContext;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum AgenticWorkflowError {
    #[error("AgenticWorkflow invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Stub deployment of an agentic workflow: renders and installs a single Kubernetes `Job`
/// running `busybox` and printing "hello world". It is intentionally a single concrete
/// struct (not generic over `CloudProvider`) because it renders identical output for every
/// cloud provider.
pub struct AgenticWorkflow {
    pub(crate) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) deployment_id: String,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) action: Action,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) lib_root_directory: String,
}

impl AgenticWorkflow {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, AgenticWorkflowError> {
        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("agentic_workflows/{long_id}"),
        )
        .map_err(|err| AgenticWorkflowError::InvalidConfig(format!("Can't create workspace directory: {err}")))?;

        // Reuse Transmitter::Job on purpose: q-core's engine-side status handling (a separate
        // Kotlin-side team) already knows how to process Job transmitters, and this stub does
        // not warrant a dedicated Transmitter variant.
        let event_details = mk_event_details(Transmitter::Job(long_id, name.clone()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);

        Ok(Self {
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            deployment_id: context
                .execution_id()
                .rsplit_once('-')
                .map(|s| s.0.to_string())
                .unwrap_or_default(),
            name,
            kube_name,
            action,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
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

    pub fn workspace_directory(&self) -> &str {
        self.workspace_directory.to_str().unwrap_or("")
    }

    pub fn helm_release_name(&self) -> String {
        self.kube_name.clone()
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-agentic-workflow", self.lib_root_directory)
    }

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub(crate) fn default_tera_context(&self, target: &DeploymentTarget) -> AgenticWorkflowTeraContext {
        AgenticWorkflowTeraContext {
            namespace: target.environment.namespace().to_string(),
            project_long_id: target.environment.project_long_id,
            environment_long_id: target.environment.long_id,
            deployment_id: self.deployment_id.clone(),
            service: ServiceTeraContext {
                short_id: to_short_id(&self.long_id),
                long_id: self.long_id,
                name: self.name.clone(),
                kube_name: self.kube_name.clone(),
            },
        }
    }
}

impl Service for AgenticWorkflow {
    fn service_type(&self) -> ServiceType {
        ServiceType::AgenticWorkflow
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
        String::new()
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

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        vec![]
    }
}

impl ToTeraContext for AgenticWorkflow {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        Ok(TeraContext::from_serialize(self.default_tera_context(target)).unwrap_or_default())
    }
}

pub trait AgenticWorkflowService: Service + DeploymentAction + ToTeraContext + Send {
    fn as_deployment_action(&self) -> &dyn DeploymentAction;
}

impl AgenticWorkflowService for AgenticWorkflow
where
    AgenticWorkflow: Service + DeploymentAction + ToTeraContext,
{
    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct ServiceTeraContext {
    pub(crate) short_id: String,
    pub(crate) long_id: Uuid,
    pub(crate) name: String,
    pub(crate) kube_name: String,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct AgenticWorkflowTeraContext {
    pub(crate) namespace: String,
    pub(crate) project_long_id: Uuid,
    pub(crate) environment_long_id: Uuid,
    pub(crate) deployment_id: String,
    pub(crate) service: ServiceTeraContext,
}

#[cfg(test)]
mod tests {
    use super::{AgenticWorkflowTeraContext, ServiceTeraContext};
    use tera::{Context, Tera};
    use uuid::Uuid;

    fn render_template(template: &str, context: AgenticWorkflowTeraContext) -> String {
        let tera_context = Context::from_serialize(context).expect("agentic workflow tera context should serialize");
        Tera::one_off(template, &tera_context, false).expect("template should render")
    }

    fn build_agentic_workflow_tera_context() -> AgenticWorkflowTeraContext {
        AgenticWorkflowTeraContext {
            namespace: "test-namespace".to_string(),
            project_long_id: Uuid::new_v4(),
            environment_long_id: Uuid::new_v4(),
            deployment_id: "test-deployment-id".to_string(),
            service: ServiceTeraContext {
                short_id: "zabc12345".to_string(),
                long_id: Uuid::new_v4(),
                name: "test agentic workflow".to_string(),
                kube_name: "test-agentic-workflow".to_string(),
            },
        }
    }

    #[test]
    fn renders_job_template_with_expected_content() {
        let rendered = render_template(
            include_str!("../../../lib/common/charts/q-agentic-workflow/templates/job.j2.yaml"),
            build_agentic_workflow_tera_context(),
        );

        assert!(rendered.contains("kind: Job"));
        assert!(rendered.contains("apiVersion: batch/v1"));
        assert!(rendered.contains("busybox"));
        assert!(rendered.contains("hello world"));
        assert!(rendered.contains("test-namespace"));
        assert!(rendered.contains("qovery.com/service-id"));
        assert!(rendered.contains("qovery.com/environment-id"));
        assert!(rendered.contains("qovery.com/project-id"));
        assert!(rendered.contains("qovery.com/deployment-id"));
        assert!(rendered.contains("test-deployment-id"));
    }

    #[test]
    fn renders_job_template_using_kube_name_not_unsafe_name() {
        let rendered = render_template(
            include_str!("../../../lib/common/charts/q-agentic-workflow/templates/job.j2.yaml"),
            build_agentic_workflow_tera_context(),
        );

        assert!(rendered.contains("name: test-agentic-workflow"));
        assert!(!rendered.contains("test agentic workflow"));
    }
}
