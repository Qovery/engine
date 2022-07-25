use crate::cloud_provider::helm::{ChartInfo, ChartSetValue};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::runtime::block_on;
use crate::template::generate_and_copy_all_files_into_dir;
use k8s_openapi::api::core::v1::Pod;
use kube::api::ListParams;
use kube::Api;
use std::path::PathBuf;
use std::time::Duration;
use tera::Context as TeraContext;
use tokio::time::Instant;

const DEFAULT_HELM_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub struct HelmDeployment {
    release_name: String,
    tera_context: TeraContext,
    chart_folder: PathBuf,
    destination_folder: PathBuf,
    value_file_override: Option<PathBuf>,
    values: Vec<ChartSetValue>,
    event_details: EventDetails,
    pod_selector: Option<String>,
}

impl HelmDeployment {
    pub fn new(
        release_name: String,
        tera_context: TeraContext,
        chart_folder: PathBuf,
        destination_folder: PathBuf,
        event_details: EventDetails,
        pod_selector: Option<String>,
    ) -> HelmDeployment {
        HelmDeployment {
            release_name,
            tera_context,
            chart_folder,
            destination_folder,
            value_file_override: None,
            values: vec![],
            event_details,
            pod_selector,
        }
    }

    pub fn new_with_values_file_override(
        release_name: String,
        tera_context: TeraContext,
        chart_folder: PathBuf,
        destination_folder: PathBuf,
        value_override: PathBuf,
        event_details: EventDetails,
        pod_selector: Option<String>,
    ) -> HelmDeployment {
        HelmDeployment {
            release_name,
            tera_context,
            chart_folder,
            destination_folder,
            value_file_override: Some(value_override),
            values: vec![],
            event_details,
            pod_selector,
        }
    }

    pub fn new_with_values(
        release_name: String,
        tera_context: TeraContext,
        chart_folder: PathBuf,
        destination_folder: PathBuf,
        values: Vec<ChartSetValue>,
        event_details: EventDetails,
        pod_selector: Option<String>,
    ) -> HelmDeployment {
        HelmDeployment {
            release_name,
            tera_context,
            chart_folder,
            destination_folder,
            value_file_override: None,
            values,
            event_details,
            pod_selector,
        }
    }

    fn prepare_helm_chart(&self) -> Result<(), EngineError> {
        // Copy the root folder
        generate_and_copy_all_files_into_dir(&self.chart_folder, &self.destination_folder, self.tera_context.clone())
            .map_err(|e| {
            EngineError::new_cannot_copy_files_from_one_directory_to_another(
                self.event_details.clone(),
                self.chart_folder.to_string_lossy().to_string(),
                self.destination_folder.to_string_lossy().to_string(),
                e,
            )
        })?;

        // If we have some special value override, replace it also
        if let Some(value_override) = &self.value_file_override {
            generate_and_copy_all_files_into_dir(value_override, &self.destination_folder, self.tera_context.clone())
                .map_err(|e| {
                EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    self.event_details.clone(),
                    self.chart_folder.to_string_lossy().to_string(),
                    self.destination_folder.to_string_lossy().to_string(),
                    e,
                )
            })?;
        }

        Ok(())
    }
}

impl DeploymentAction for HelmDeployment {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        self.prepare_helm_chart()?;

        let workspace_dir = self.destination_folder.to_str().unwrap_or_default();
        let chart = ChartInfo::new_from_custom_namespace(
            self.release_name.to_string(),
            workspace_dir.to_string(),
            target.environment.namespace().to_string(),
            DEFAULT_HELM_TIMEOUT.as_secs() as i64,
            vec![],
            self.values.clone(),
            vec![],
            false,
            None,
        );

        target
            .helm
            .upgrade(&chart, &[])
            .map_err(|e| EngineError::new_helm_error(self.event_details.clone(), e))
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let chart = ChartInfo::new_from_release_name(&self.release_name, target.environment.namespace());

        target
            .helm
            .uninstall(&chart, &[])
            .map_err(|e| EngineError::new_helm_error(self.event_details.clone(), e))?;

        // helm does not wait for pod to terminate https://github.com/helm/helm/issues/10586
        // So wait for
        if let Some(pod_selector) = &self.pod_selector {
            block_on(async {
                let started = Instant::now();

                let pods: Api<Pod> = Api::namespaced(target.kube.clone(), target.environment.namespace());
                while let Ok(pod) = pods.list(&ListParams::default().labels(pod_selector)).await {
                    if pod.items.is_empty() {
                        break;
                    }

                    if started.elapsed() > DEFAULT_HELM_TIMEOUT {
                        break;
                    }

                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
        }

        Ok(())
    }
}

#[cfg(feature = "test-local-kube")]
#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::ChartInfo;
    use crate::cmd::helm::Helm;
    use crate::deployment_action::deploy_helm::HelmDeployment;
    use crate::events::{EventDetails, GeneralStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;
    use function_name::named;

    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            QoveryIdentifier::from("puic".to_string()),
            QoveryIdentifier::from("puic".to_string()),
            QoveryIdentifier::from("puic".to_string()),
            None,
            Stage::General(GeneralStep::RetrieveClusterConfig),
            Transmitter::SecretManager("sdfsdf".to_string()),
        );

        let dest_folder = PathBuf::from(format!("/tmp/{}", namespace));
        let chart = ChartInfo::new_from_custom_namespace(
            "test_app_helm_deployment".to_string(),
            dest_folder.to_string_lossy().to_string(),
            namespace,
            600,
            vec![],
            vec![],
            vec![],
            false,
            None,
        );

        let mut tera_context = tera::Context::default();
        tera_context.insert("app_name", "pause");
        let helm_deployment = HelmDeployment::new(
            chart.name.clone(),
            tera_context,
            PathBuf::from("tests/helm/simple_app_deployment"),
            dest_folder,
            event_details,
            Some("app=pause".to_string()),
        );

        // Render a simple chart
        helm_deployment.prepare_helm_chart().unwrap();

        let mut kube_config = dirs::home_dir().unwrap();
        kube_config.push(".kube/config");
        let helm = Helm::new(kube_config.to_str().unwrap(), &[])?;

        // Check that helm can validate our chart
        helm.template_validate(&chart, &[])?;

        Ok(())
    }
}
