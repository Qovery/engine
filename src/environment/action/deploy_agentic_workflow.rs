use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::models::agentic_workflow::AgenticWorkflow;
use crate::environment::models::types::ToTeraContext;
use crate::environment::report::agentic_workflow::reporter::AgenticWorkflowDeploymentReporter;
use crate::environment::report::execute_long_deployment;
use crate::environment::report::logger::EnvProgressLogger;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use std::path::PathBuf;

impl DeploymentAction for AgenticWorkflow {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let task = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            let chart = build_agentic_workflow_chart_info(self, target);
            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

            // We first need to delete the old job, because job spec cannot be updated (it is
            // an immutable resource), mirroring the base Job deploy pattern.
            helm.on_delete(target)?;
            helm.on_create(target)?;

            Ok(())
        };

        execute_long_deployment(AgenticWorkflowDeploymentReporter::new(self, target, Action::Create), task)
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        // Not supported yet for this stub service.
        Ok(())
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));

        let task = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            let chart = build_agentic_workflow_chart_info(self, target);
            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

            helm.on_delete(target)
        };

        execute_long_deployment(AgenticWorkflowDeploymentReporter::new(self, target, Action::Delete), task)
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        // Not supported yet for this stub service.
        Ok(())
    }
}

fn build_agentic_workflow_chart_info(agentic_workflow: &AgenticWorkflow, target: &DeploymentTarget) -> ChartInfo {
    ChartInfo {
        name: agentic_workflow.helm_release_name(),
        path: agentic_workflow.workspace_directory().to_string(),
        namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
        k8s_selector: Some(agentic_workflow.kube_label_selector()),
        ..Default::default()
    }
}
