use crate::cmd::command::CommandKiller;
use crate::environment::action::DeploymentAction;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::helm::{ChartInfo, HelmChart, ServiceChart};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::template::{generate_and_copy_all_files_into_dir, write_chart_values, write_values_file};
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

/// How to prepare values after copying the chart and rendering its legacy Tera files.
pub enum HelmChartValues {
    /// Keep the chart's values.yaml, including any values rendered from its Tera files.
    ChartDefaults,
    /// Replace values.yaml with the serialized deployment context.
    SerializedContext,
    /// Write qovery-values.yaml with the serialized context, preserving chart defaults.
    /// The override must also be listed in ChartInfo::values_files.
    SerializedOverride,
    /// Render a legacy values override and its sibling files, preserving chart defaults.
    /// The rendered override must also be listed in ChartInfo::values_files.
    TeraFile(PathBuf),
}

/// Prepares and deploys a Helm chart, optionally rendering legacy templates with Tera.
pub struct HelmDeployment {
    event_details: EventDetails,
    tera_context: TeraContext,
    /// The chart source directory which will be copied into the workspace
    chart_orginal_dir: PathBuf,
    values: HelmChartValues,
    /// Path should be inside the workspace directory because it will be copied there
    pub helm_chart: ChartInfo,
}

impl HelmDeployment {
    /// Creates a deployment with an explicit policy for preparing chart values.
    pub fn new(
        event_details: EventDetails,
        tera_context: TeraContext,
        chart_orginal_dir: PathBuf,
        values: HelmChartValues,
        helm_chart: ChartInfo,
    ) -> HelmDeployment {
        HelmDeployment {
            event_details,
            tera_context,
            chart_orginal_dir,
            values,
            helm_chart,
        }
    }

    /// Copies native Helm files unchanged, renders legacy .j2 files, then prepares values.
    pub fn prepare_helm_chart(&self) -> Result<(), Box<EngineError>> {
        generate_and_copy_all_files_into_dir(&self.chart_orginal_dir, &self.helm_chart.path, &self.tera_context)
            .and_then(|()| match &self.values {
                HelmChartValues::ChartDefaults => Ok(()),
                HelmChartValues::SerializedContext => write_chart_values(&self.helm_chart.path, &self.tera_context),
                HelmChartValues::SerializedOverride => {
                    write_values_file(Path::new(&self.helm_chart.path).join("qovery-values.yaml"), &self.tera_context)
                }
                HelmChartValues::TeraFile(custom_value) => {
                    let custom_value_dir_path = custom_value.parent().unwrap_or_else(|| Path::new("./"));
                    generate_and_copy_all_files_into_dir(
                        custom_value_dir_path,
                        &self.helm_chart.path,
                        &self.tera_context,
                    )
                }
            })
            .map_err(|e| {
                Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    self.event_details.clone(),
                    self.chart_orginal_dir.to_string_lossy().to_string(),
                    self.helm_chart.path.clone(),
                    e,
                ))
            })
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
        Err(Box::new(EngineError::new_cannot_restart_service(
            EventDetails::clone_changing_stage(
                self.event_details.clone(),
                Stage::Environment(EnvironmentStep::Restart),
            ),
            target.environment.namespace(),
            "",
            command_error,
        )))
    }
}

#[cfg(test)]
mod chart_preparation_tests {
    use super::{HelmChartValues, HelmDeployment};
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::helm::ChartInfo;
    use crate::io_models::QoveryIdentifier;
    use serde_json::json;
    use std::fs;
    use std::process::Command;
    use tera::Context;
    use uuid::Uuid;

    fn event_details() -> EventDetails {
        EventDetails::new(
            None,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            Uuid::new_v4().to_string(),
            Stage::Infrastructure(InfrastructureStep::RetrieveClusterConfig),
            Transmitter::TaskManager(Uuid::new_v4(), "engine".to_string()),
        )
    }

    #[test]
    fn serialized_context_supports_native_and_legacy_templates() {
        let source = tempfile::tempdir().expect("source directory");
        let destination = tempfile::tempdir().expect("destination directory");
        fs::create_dir(source.path().join("templates")).expect("templates directory");
        let template = "value: {{ .Values.service.value | quote }}\n";
        fs::write(source.path().join("templates/secret.yaml"), template).expect("write template");
        fs::write(
            source.path().join("templates/legacy.j2.yaml"),
            "enabled: {{ service.enabled }}\n",
        )
        .expect("write legacy template");
        fs::write(source.path().join("values.yaml"), "{}\n").expect("write default values");
        let values = json!({
            "service": {
                "value": "{{ fail \"must remain data\" }}\n{% if literal %}",
                "enabled": false,
                "replicas": 0,
                "optional": null,
                "arguments": ["sh", "-c", "echo 'hello, world'"],
            }
        });
        let deployment = HelmDeployment::new(
            event_details(),
            Context::from_value(values.clone()).expect("values context"),
            source.path().to_path_buf(),
            HelmChartValues::SerializedContext,
            ChartInfo {
                path: destination.path().to_string_lossy().into_owned(),
                ..Default::default()
            },
        );

        deployment.prepare_helm_chart().expect("prepare native chart");

        assert_eq!(
            fs::read_to_string(destination.path().join("templates/secret.yaml")).expect("copied template"),
            template,
        );
        assert_eq!(
            fs::read_to_string(destination.path().join("templates/legacy.yaml")).expect("rendered legacy template"),
            "enabled: false\n",
        );
        assert!(!destination.path().join("templates/legacy.j2.yaml").exists());
        let serialized_values = fs::read_to_string(destination.path().join("values.yaml")).expect("generated values");
        assert_eq!(
            serde_yaml::from_str::<serde_json::Value>(&serialized_values).expect("valid YAML"),
            values
        );
    }

    #[test]
    fn chart_values_preserve_defaults_and_apply_custom_overrides() {
        let source = tempfile::tempdir().expect("source directory");
        let overrides = tempfile::tempdir().expect("overrides directory");
        fs::create_dir(source.path().join("templates")).expect("templates directory");
        fs::write(
            source.path().join("Chart.yaml"),
            "apiVersion: v2\nname: legacy-chart\nversion: 0.1.0\n",
        )
        .expect("write chart metadata");
        let defaults = "image: default-image\nprimary:\n  persistence:\n    size: 8Gi\n";
        fs::write(source.path().join("values.yaml"), defaults).expect("write defaults");
        fs::write(
            source.path().join("templates/configmap.j2.yaml"),
            r#"apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ name | yaml_encode }}
data:
  image: {% raw %}{{ .Values.image | quote }}{% endraw %}
  size: {% raw %}{{ .Values.primary.persistence.size | quote }}{% endraw %}
"#,
        )
        .expect("write legacy template");
        let override_path = overrides.path().join("qovery-values.j2.yaml");
        fs::write(
            &override_path,
            "primary:\n  persistence:\n    size: {{ database_disk_size_in_gib }}Gi\n",
        )
        .expect("write values override");
        let context = Context::from_value(json!({
            "name": "legacy-config",
            "image": "context-must-not-replace-chart-defaults",
            "database_disk_size_in_gib": 10,
        }))
        .expect("legacy context");

        for (values, expected_size) in [
            (HelmChartValues::ChartDefaults, "8Gi"),
            (HelmChartValues::SerializedOverride, "10Gi"),
            (HelmChartValues::TeraFile(override_path), "10Gi"),
        ] {
            let destination = tempfile::tempdir().expect("destination directory");
            let values_files = if matches!(values, HelmChartValues::TeraFile(_) | HelmChartValues::SerializedOverride) {
                vec![
                    destination
                        .path()
                        .join("qovery-values.yaml")
                        .to_string_lossy()
                        .into_owned(),
                ]
            } else {
                vec![]
            };
            let values_context = if matches!(values, HelmChartValues::SerializedOverride) {
                Context::from_value(json!({
                    "name": "legacy-config",
                    "primary": {"persistence": {"size": "10Gi"}},
                }))
                .expect("override values context")
            } else {
                context.clone()
            };
            let deployment = HelmDeployment::new(
                event_details(),
                values_context,
                source.path().to_path_buf(),
                values,
                ChartInfo {
                    path: destination.path().to_string_lossy().into_owned(),
                    values_files,
                    ..Default::default()
                },
            );

            deployment.prepare_helm_chart().expect("prepare legacy chart");

            assert_eq!(
                fs::read_to_string(destination.path().join("values.yaml")).expect("chart defaults"),
                defaults,
            );
            assert!(!destination.path().join("templates/configmap.j2.yaml").exists());
            assert!(!destination.path().join("qovery-values.j2.yaml").exists());
            assert_eq!(
                destination.path().join("qovery-values.yaml").exists(),
                !deployment.helm_chart.values_files.is_empty(),
            );
            let mut helm = Command::new("helm");
            helm.args(["template", "test", &deployment.helm_chart.path]);
            for file in &deployment.helm_chart.values_files {
                helm.args(["--values", file]);
            }
            let output = helm.output().expect("helm must be installed to test chart rendering");
            assert!(
                output.status.success(),
                "helm template failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let manifest: serde_json::Value = serde_yaml::from_slice(&output.stdout).expect("rendered manifest");
            assert_eq!(manifest["metadata"]["name"], "legacy-config");
            assert_eq!(manifest["data"]["image"], "default-image");
            assert_eq!(manifest["data"]["size"], expected_size);
        }
    }
}

#[cfg(feature = "test-local-kube")]
#[cfg(test)]
mod tests {
    use crate::cmd::helm::Helm;
    use crate::environment::action::deploy_helm::{HelmChartValues, HelmDeployment, default_helm_timeout};
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::helm::ChartInfo;
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
            HelmChartValues::ChartDefaults,
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
