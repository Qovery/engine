use crate::cloud_provider::helm::{ChartInfo, HelmChart, ServiceChart};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::command::CommandKiller;
use crate::deployment_action::DeploymentAction;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::template::generate_and_copy_all_files_into_dir;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tera::Context as TeraContext;

pub fn default_helm_timeout() -> Duration {
    match env::var("HELM_TIMEOUT_IN_SECS") {
        Ok(env_var) => match env_var.parse::<u64>() {
            Ok(timeout) => Duration::from_secs(timeout),
            Err(_) => Duration::from_secs(10 * 60),
        },
        Err(_) => Duration::from_secs(10 * 60),
    }
}

/// Helm Deployment manages Helm + jinja support
pub struct HelmDeployment {
    event_details: EventDetails,
    tera_context: TeraContext,
    /// The chart source directory which will be copied into the workspace
    chart_orginal_dir: PathBuf,
    /// name of the value files to render and use during helm deploy
    pub render_custom_values_file: Option<PathBuf>,
    /// Path should be inside the workspace directory because it will be copied there
    pub helm_chart: ChartInfo,
}

impl HelmDeployment {
    pub fn new(
        event_details: EventDetails,
        tera_context: TeraContext,
        chart_orginal_dir: PathBuf,
        render_custom_values_file: Option<PathBuf>,
        helm_chart: ChartInfo,
    ) -> HelmDeployment {
        HelmDeployment {
            event_details,
            tera_context,
            chart_orginal_dir,
            render_custom_values_file,
            helm_chart,
        }
    }

    pub fn prepare_helm_chart(&self) -> Result<(), Box<EngineError>> {
        // Copy the root folder
        generate_and_copy_all_files_into_dir(&self.chart_orginal_dir, &self.helm_chart.path, &self.tera_context)
            .map_err(|e| {
                EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    self.event_details.clone(),
                    self.chart_orginal_dir.to_string_lossy().to_string(),
                    self.helm_chart.path.clone(),
                    e,
                )
            })?;

        // If we have some special value override, render and copy it
        if let Some(custom_value) = self.render_custom_values_file.clone() {
            let custom_value_dir_path = custom_value.parent().unwrap_or_else(|| Path::new("./"));

            generate_and_copy_all_files_into_dir(custom_value_dir_path, &self.helm_chart.path, &self.tera_context)
                .map_err(|e| {
                    EngineError::new_cannot_copy_files_from_one_directory_to_another(
                        self.event_details.clone(),
                        self.chart_orginal_dir.to_string_lossy().to_string(),
                        self.helm_chart.path.clone(),
                        e,
                    )
                })?;
        }

        Ok(())
    }
}

impl DeploymentAction for HelmDeployment {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        self.prepare_helm_chart()?;

        let service_chart = ServiceChart::new(target.helm.clone(), self.helm_chart.clone());
        let chart: Box<dyn HelmChart> = Box::new(service_chart);
        chart
            .run(
                &target.kube,
                &target.kubernetes.kubeconfig_local_file_path(),
                target.cloud_provider.credentials_environment_variables().as_slice(),
                &CommandKiller::from_cancelable(target.abort),
            )
            .map_err(|e| Box::new(EngineError::new_helm_chart_error(self.event_details.clone(), e)))?;
        Ok(())
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        target
            .helm
            .uninstall(
                &self.helm_chart,
                &[],
                &CommandKiller::from_cancelable(target.abort),
                &mut |line| {
                    info!("{}", line);
                },
                &mut |line| {
                    info!("{}", line);
                },
            )
            .map_err(|e| EngineError::new_helm_error(self.event_details.clone(), e))?;

        Ok(())
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let command_error = CommandError::new_from_safe_message("Cannot restart Helm deployment".to_string());
        return Err(Box::new(EngineError::new_cannot_restart_service(
            EventDetails::clone_changing_stage(
                self.event_details.clone(),
                Stage::Environment(EnvironmentStep::Restart),
            ),
            target.environment.namespace(),
            "",
            command_error,
        )));
    }
}

#[cfg(feature = "test-local-kube")]
#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::ChartInfo;
    use crate::cmd::helm::Helm;
    use crate::deployment_action::deploy_helm::{default_helm_timeout, HelmDeployment};
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;
    use function_name::named;

    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    #[test]
    #[named]
    fn test_helm_deployment() -> Result<(), Box<dyn std::error::Error>> {
        let namespace = format!(
            "{}-{:?}",
            function_name!().replace('_', "-"),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );

        let event_details = EventDetails::new(
            None,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            Uuid::new_v4().to_string(),
            Stage::Infrastructure(InfrastructureStep::RetrieveClusterConfig),
            Transmitter::TaskManager(Uuid::new_v4(), "engine".to_string()),
        );

        let dest_folder = PathBuf::from(format!("/tmp/{namespace}"));
        let chart = ChartInfo::new_from_custom_namespace(
            "test-app-helm-deployment".to_string(),
            dest_folder.to_string_lossy().to_string(),
            namespace,
            default_helm_timeout().as_secs() as i64,
            vec![],
            vec![],
            vec![],
            false,
            None,
        );

        let mut tera_context = tera::Context::default();
        tera_context.insert("app_name", "pause");
        let helm = HelmDeployment::new(
            event_details,
            tera_context,
            PathBuf::from("tests/helm/simple_app_deployment"),
            None,
            chart.clone(),
        );

        // Render a simple chart
        helm.prepare_helm_chart().unwrap();

        let mut kube_config = dirs::home_dir().unwrap();
        kube_config.push(".kube/config");
        let helm = Helm::new(Some(kube_config.to_str().unwrap()), &[])?;

        // Check that helm can validate our chart
        helm.template_validate(&chart, &[], None)?;

        Ok(())
    }
}
