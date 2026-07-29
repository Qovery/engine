use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_job::job::{
    JobRunError, await_job_pod_to_terminate, await_job_to_complete, job_is_failed, kill_job,
    retrieve_output_and_terminate_pod,
};
use crate::environment::action::log_job_output_error;
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
use crate::runtime::block_on;
use std::path::PathBuf;
use std::time::Duration;

impl DeploymentAction for AgenticWorkflow {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let task = |logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
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

            // Wait for the agent's container to terminate (the output-waiter sidecar keeps the
            // pod alive so we can still retrieve the job output afterwards), mirroring
            // deploy_job's monitoring flow.
            let max_execution_duration = Duration::from_secs(60) + Duration::from_secs(self.max_duration_in_sec());
            let pod = block_on(await_job_pod_to_terminate(
                self.kube_name(),
                max_execution_duration,
                target.environment.namespace(),
                target.kube.client(),
                target.abort,
            ));
            let pod_name = match pod {
                Ok(pod) => pod.metadata.name.unwrap_or_default(),
                Err(JobRunError::Aborted) => {
                    let _ = block_on(kill_job(target.kube.client(), target.environment.namespace(), self.kube_name()));
                    return Err(Box::new(EngineError::new_task_cancellation_requested(event_details.clone())));
                }
                Err(err) => return Err(Box::new(EngineError::new_job_error(event_details.clone(), err.to_string()))),
            };
            info!("Targeting agentic workflow job pod name: {}", pod_name);

            // Fetch the Qovery JSON output if any, and surface it to the core for the next
            // deployment stage - same contract as deploy_job.
            match block_on(retrieve_output_and_terminate_pod(
                target.kube.client(),
                target.environment.namespace(),
                &pod_name,
                self.output_variable_validation_pattern(),
            )) {
                Ok(None) => {}
                Ok(Some(output)) => logger.core_configuration_for_job(
                    "Job output succeeded. Environment variables will be synchronized.".to_string(),
                    serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string()),
                ),
                Err(err) => log_job_output_error(logger, &event_details, err),
            }

            let job = block_on(await_job_to_complete(
                self.kube_name(),
                max_execution_duration,
                target.environment.namespace(),
                target.kube.client(),
                target.abort,
            ))
            .map_err(|err| Box::new(EngineError::new_job_error(event_details.clone(), err.to_string())))?;

            if let Some(condition) = job_is_failed(&job) {
                let msg = format!(
                    "Agentic workflow job failed to correctly run due to {} {}",
                    condition.reason, condition.message
                );
                return Err(Box::new(EngineError::new_job_error(event_details.clone(), msg)));
            }

            Ok(())
        };

        execute_long_deployment(AgenticWorkflowDeploymentReporter::new(self, target, Action::Create), task)
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        // Not supported yet: AgenticWorkflow runs to completion as a single Job, it has no
        // long-lived process to pause.
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
        // Not supported yet for this service: a Job run-to-completion has no restart semantics.
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
