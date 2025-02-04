use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::DeploymentAction;
use crate::environment::models::terraform_service::{TerraformService, TerraformServiceTrait};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::environment::report::terraform_service::reporter::TerraformServiceDeploymentReporter;
use crate::environment::report::{execute_long_deployment, DeploymentTaskImpl};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::helm::{ChartInfo, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::Pod;
use kube::runtime::wait::await_condition;
use kube::Api;
use std::collections::HashSet;
use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for TerraformService<T>
where
    TerraformService<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let pre_run = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) };

        let run = |logger: &EnvProgressLogger, _state: ()| {
            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom,
                custom_namespace: Some(target.environment.namespace().to_string()),
                timeout_in_seconds: self.startup_timeout().as_secs() as i64,
                k8s_selector: Some(self.kube_label_selector()),
                ..Default::default()
            };

            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

            // We first need to delete the old job, because job spec cannot be updated (due to be an immutable resources)
            // TODO TF, change because we must keep the Persistent Volume
            helm.on_delete(target)?;

            // create job
            helm.on_create(target)?;

            // Get kube config file

            let job_pod_selector = format!("job-name={}", self.kube_name());
            let kube_pod_api: Api<Pod> = Api::namespaced(target.kube.clone(), target.environment.namespace());

            let mut set_of_pods_already_processed: HashSet<String> = HashSet::new();
            // loop {

            // Wait for the pod to be started to get its name
            let pod_name = crate::environment::action::deploy_job::get_active_job_pod_by_selector(
                kube_pod_api.clone(),
                &job_pod_selector,
                &event_details,
                &set_of_pods_already_processed,
                self.job_max_duration(),
            )?;
            set_of_pods_already_processed.insert(pod_name.clone());

            // Wait for the job container to be terminated
            logger.info(format!("Waiting for the job container {} to be processed...", self.kube_name()));
            block_on(async {
                tokio::select! {
                    biased;
                    _ = await_condition(
                        kube_pod_api.clone(),
                        &pod_name,
                        crate::environment::action::deploy_job::is_job_pod_container_terminated(self.kube_name()),
                    ) => {},
                }
            });

            // break; // TODO TF check if the pod should start only once
            // }

            Ok(())
        };

        let post_run = |_logger: &EnvSuccessLogger, _state: ()| {};

        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };

        execute_long_deployment(TerraformServiceDeploymentReporter::new(self, target, Action::Create), task)
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_pause().");
        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_delete().");
        Ok(())
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_restart().");
        Ok(())
    }
}
