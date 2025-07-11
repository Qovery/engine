use crate::infrastructure::models::build_platform::{Credentials, SshKey};

use crate::cmd::command::CommandKiller;
use crate::cmd::git;
use crate::environment::action::pause_service::PauseServiceAction;
use crate::environment::action::restart_service::RestartServiceAction;
use crate::environment::action::{DeploymentAction, K8sResourceType};
use crate::environment::models::helm_chart::{HelmChart, HelmChartSource, HelmValueSource};
use crate::environment::models::types::CloudProvider;
use crate::environment::report::helm_chart::reporter::HelmChartDeploymentReporter;
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::environment::report::{DeploymentTaskImpl, execute_long_deployment};
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::helm::{ChartInfo, HelmChartError};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::io_models::variable_utils::VariableInfo;
use anyhow::anyhow;
use git2::{Cred, CredentialType};
use itertools::Itertools;
use kube::api::{DeleteParams, PartialObjectMeta, Patch, PatchParams};
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

use crate::runtime::block_on;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

const HELM_CHART_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(9 * 60);

impl<T: CloudProvider> DeploymentAction for HelmChart<T> {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let pre_run = |logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            prepare_helm_chart_directory(self, target, event_details.clone(), logger)?;
            // Now the chart is ready at self.chart_workspace_directory()

            // Check users does not bypass restrictions (i.e: install cluster wide resources, or not in the correct namespace)
            check_resources_are_allowed_to_install(self, target, event_details.clone(), logger)?;

            // Create config map for qovery-webhook-admission-controller to inject labels / annotations
            create_config_map_for_webhook_admission_controller_if_not_exists(self, target, event_details.clone())?;
            Ok(())
        };

        let run = |logger: &EnvProgressLogger, _state: ()| {
            // unpause cron job if necessary
            let _ = PauseServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::CronJob,
                Duration::from_secs(5 * 60),
                event_details.clone(),
                self.is_cluster_wide_resources_allowed(),
                true,
            )
            .unpause_if_needed(target);

            // unpause daemonset if necessary
            let _ = PauseServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::DaemonSet,
                Duration::from_secs(5 * 60),
                event_details.clone(),
                self.is_cluster_wide_resources_allowed(),
                true,
            )
            .unpause_if_needed(target);

            let args = self.helm_upgrade_arguments().collect::<Vec<_>>();
            target
                .helm
                .upgrade_raw(
                    self.helm_release_name(),
                    self.chart_workspace_directory(),
                    target.environment.namespace(),
                    &args.iter().map(|x| x.as_ref()).collect::<Vec<_>>(),
                    &[],
                    &CommandKiller::from(self.helm_timeout(), target.abort),
                    &mut |line| logger.info(line),
                    &mut |line| logger.warning(line),
                )
                .map_err(|err| (event_details.clone(), HelmChartError::HelmError(err)))?;

            Ok(())
        };

        let post_run = |_logger: &EnvSuccessLogger, _state: ()| {};

        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };

        execute_long_deployment(HelmChartDeploymentReporter::new(self, target, Action::Create), task)
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let _event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));

        let task = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            let pause_cron_job = PauseServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::CronJob,
                Duration::from_secs(5 * 60),
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                self.is_cluster_wide_resources_allowed(),
                false,
            );
            pause_cron_job.on_pause(target)?;

            let pause_deployment = PauseServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::Deployment,
                Duration::from_secs(5 * 60),
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                self.is_cluster_wide_resources_allowed(),
                false,
            );
            pause_deployment.on_pause(target)?;

            let pause_statefulset = PauseServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::StateFulSet,
                Duration::from_secs(5 * 60),
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                self.is_cluster_wide_resources_allowed(),
                false,
            );
            pause_statefulset.on_pause(target)?;

            let pause_daemonset = PauseServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::DaemonSet,
                Duration::from_secs(5 * 60),
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                self.is_cluster_wide_resources_allowed(),
                true,
            );
            pause_daemonset.on_pause(target)
        };

        execute_long_deployment(HelmChartDeploymentReporter::new(self, target, Action::Pause), task)
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));

        let task = |logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            // delete admission controller config map
            let config_map_api: Api<ConfigMap> = Api::namespaced(target.kube.client(), target.environment.namespace());
            let admission_controller_config_map = self.admission_controller_config_map_name();
            if let Err(err) =
                block_on(config_map_api.delete(admission_controller_config_map.as_str(), &DeleteParams::default()))
            {
                // this should not be blocking, as some helm charts don't have this secret
                warn!(
                    "Cannot delete secret {}: {}",
                    admission_controller_config_map.as_str(),
                    err.to_string()
                );
            }

            let namespace_from_args = extract_namespace_from_helm_args(self.helm_upgrade_arguments());

            // uninstall chart
            let mut chart_info = ChartInfo::new_from_release_name(
                self.helm_release_name(),
                &namespace_from_args.unwrap_or_else(|| Cow::Borrowed(target.environment.namespace())), // take the namespace from the list of arguments if it exists
            );
            chart_info.timeout_in_seconds = self.helm_timeout().as_secs() as i64;

            target
                .helm
                .uninstall(
                    &chart_info,
                    &[],
                    &CommandKiller::from(self.helm_timeout(), target.abort),
                    &mut |line| logger.info(line),
                    &mut |line| logger.warning(line),
                )
                .map_err(|err| {
                    Box::new(EngineError::new_helm_chart_error(
                        event_details.clone(),
                        HelmChartError::HelmError(err),
                    ))
                })
        };

        execute_long_deployment(HelmChartDeploymentReporter::new(self, target, Action::Delete), task)
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let _event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Restart));

        let task = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            let restart_daemon_set = RestartServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::DaemonSet,
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                self.is_cluster_wide_resources_allowed(),
            );
            restart_daemon_set.on_restart(target)?;

            let restart_deployment = RestartServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::Deployment,
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                self.is_cluster_wide_resources_allowed(),
            );
            restart_deployment.on_restart(target)?;

            let restart_statefulset = RestartServiceAction::new_with_resource_type(
                self.kube_label_selector(),
                K8sResourceType::StateFulSet,
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                self.is_cluster_wide_resources_allowed(),
            );
            restart_statefulset.on_restart(target)?;
            Ok(())
        };

        execute_long_deployment(HelmChartDeploymentReporter::new(self, target, Action::Restart), task)
    }
}

fn write_helm_value_with_replacement<'a>(
    mut lines: impl Iterator<Item = Cow<'a, str>>,
    output_writer: &mut impl Write,
    service_id: Uuid,
    service_name: &str,
    service_version: &str,
    environment_id: Uuid,
    project_id: Uuid,
    env_vars: &HashMap<String, VariableInfo>,
    loadbalancer_l4_annotations: Vec<(String, String)>,
) -> Result<(), anyhow::Error> {
    let mut output_writer = BufWriter::new(output_writer);
    let mut lines = lines.try_fold(Vec::with_capacity(512), |mut acc, l| {
        replace_qovery_env_variable(l, env_vars).map(|ret| {
            acc.push(ret);
            acc
        })
    })?;

    let labels_replacements = vec![
        (
            "qovery.labels.service",
            vec![
                "qovery.com/service-type: helm".to_string(),
                format!("qovery.com/service-id: {}", service_id),
                format!("qovery.com/environment-id: {}", environment_id),
                format!("qovery.com/project-id: {}", project_id),
            ],
        ),
        (
            "qovery.annotations.loadbalancer",
            loadbalancer_l4_annotations
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect_vec(),
        ),
        (
            "qovery.annotations.service",
            vec![format!("qovery.com/service-version: {}", service_version)],
        ),
        ("qovery.service.id", vec![service_id.to_string()]),
        ("qovery.service.version", vec![service_version.to_string()]),
        ("qovery.service.type", vec!["helm".to_string()]),
        ("qovery.service.name", vec![service_name.to_string()]),
        ("qovery.environment.id", vec![environment_id.to_string()]),
        ("qovery.project.id", vec![project_id.to_string()]),
    ];

    // Replace all matching pattern by their respective replacements
    // 1 line can generate N lines in output
    for (pattern, replacements) in &labels_replacements {
        let nb_lines = lines.len();
        lines = lines
            .into_iter()
            .try_fold(Vec::with_capacity(nb_lines + replacements.len()), |acc, l| {
                replace_qovery_labels(acc, l, pattern, replacements)
            })?;
    }

    // Writes all lines into the files
    for line in lines {
        output_writer.write_all(line.as_bytes())?;
        output_writer.write_all(b"\n")?;
    }

    output_writer.flush()?;

    Ok(())
}

fn replace_qovery_labels<'a>(
    mut acc: Vec<Cow<'a, str>>,
    line: Cow<'a, str>,
    pattern: &str,
    replacements: &'a [String],
) -> Result<Vec<Cow<'a, str>>, anyhow::Error> {
    if line.contains(pattern) {
        replacements
            .iter()
            .map(move |replacement| Cow::Owned(line.replace(pattern, replacement)))
            .for_each(|item| acc.push(item))
    } else {
        acc.push(line)
    };

    Ok(acc)
}

fn replace_qovery_env_variable<'a>(
    mut line: Cow<'a, str>,
    envs: &HashMap<String, VariableInfo>,
) -> Result<Cow<'a, str>, anyhow::Error> {
    const PREFIX: &str = "qovery.env.";

    // Loop until we find all occurrences on the same line
    loop {
        // If no pattern matches, exit the loop to return our result
        let Some(beg_pos) = line.find(PREFIX) else {
            break;
        };

        let needle = &line[beg_pos..];
        // Built-in variable are not allowed because they contains ID in them
        // Which we will not be able to replace during a clone. So use must set an alias or use its own vars
        if needle[PREFIX.len()..].starts_with("QOVERY_") {
            return Err(anyhow!(
                "You cannot use Qovery built_in variable in your helm values file. Please create and use an alias. line: {}",
                line
            ));
        }

        let variable_name =
            if let Some(end_pos) = needle.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')) {
                &needle[..end_pos]
            } else {
                needle
            };

        let Some(variable_infos) = envs.get(&variable_name[PREFIX.len()..]) else {
            return Err(anyhow!(
                "Invalid variable, specified {:?} variable does not exist at line: {}",
                variable_name,
                line
            ));
        };

        line = Cow::Owned(line.replace(variable_name, &variable_infos.value))
    }

    Ok(line)
}

fn git_credentials_callback<'a>(
    git_credentials: &'a Option<Credentials>,
    ssh_keys: &'a [SshKey],
) -> impl Fn(&str) -> Vec<(CredentialType, Cred)> + 'a {
    move |user: &str| {
        let mut creds: Vec<(CredentialType, Cred)> = Vec::with_capacity(ssh_keys.len() + 1);
        for ssh_key in ssh_keys.iter() {
            let public_key = ssh_key.public_key.as_deref();
            let passphrase = ssh_key.passphrase.as_deref();
            if let Ok(cred) = Cred::ssh_key_from_memory(user, public_key, &ssh_key.private_key, passphrase) {
                creds.push((CredentialType::SSH_MEMORY, cred));
            }
        }

        if let Some(git_creds) = git_credentials {
            creds.push((
                CredentialType::USER_PASS_PLAINTEXT,
                Cred::userpass_plaintext(&git_creds.login, &git_creds.password).unwrap(),
            ));
        }

        creds
    }
}

// Goal is to download the chart in the workspace directory with everything ready to execute
// 1. Download the chart on disk
// 2. Copy the values files in the chart location with qovery replacements
fn prepare_helm_chart_directory<T: CloudProvider>(
    this: &HelmChart<T>,
    target: &DeploymentTarget,
    event_details: EventDetails,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>> {
    // Error Mapper.
    // There are a lot of small io error possible. We only want to return a meaningful error msg to user
    let to_error = |msg: String| -> Box<EngineError> {
        Box::new(EngineError::new_helm_chart_error(
            event_details.clone(),
            HelmChartError::CreateTemplateError {
                chart_name: this.name().to_string(),
                msg,
            },
        ))
    };

    // Prepare the chart with template folder
    match this.chart_source() {
        HelmChartSource::Repository {
            chart_name,
            engine_helm_registry,
            chart_version,
            skip_tls_verify,
            ..
        } => {
            fs::create_dir(this.chart_workspace_directory())
                .map_err(|e| to_error(format!("Cannot create destination directory for chart due to {e}")))?;

            let repository_url_with_credentials = match engine_helm_registry.get_url_with_credentials() {
                Ok(url) => url,
                Err(err) => {
                    error!("Cannot get URL with credentials, taking the url without credentials: {}", err);
                    engine_helm_registry.get_url()
                }
            };
            let url_without_password = {
                let mut url = repository_url_with_credentials.clone();
                let _ = url.set_password(None);
                url
            };
            logger.info(format!(
                "📥 Downloading Helm chart {chart_name} at version {chart_version} from {url_without_password}"
            ));

            target
                .helm
                .download_chart(
                    &repository_url_with_credentials,
                    engine_helm_registry,
                    chart_name,
                    chart_version,
                    this.chart_workspace_directory(),
                    *skip_tls_verify,
                    &[],
                    &CommandKiller::from(HELM_CHART_DOWNLOAD_TIMEOUT, target.abort),
                )
                .map_err(|e| (event_details.clone(), e))?;
        }
        HelmChartSource::Git {
            git_url,
            commit_id,
            root_path,
            get_credentials,
            ssh_keys,
        } => {
            logger.info(format!(
                "📥 Cloning Helm chart from git repository {git_url} at commit {commit_id}"
            ));

            let tmpdir = tempfile::tempdir_in(this.workspace_directory())
                .map_err(|e| to_error(format!("Cannot create tempdir {e}")))?;

            let git_creds =
                get_credentials().map_err(|e| to_error(format!("Cannot get git credentials due to {e}")))?;

            git::clone_at_commit(git_url, commit_id, &tmpdir, &git_credentials_callback(&git_creds, ssh_keys))
                .map_err(|e| to_error(format!("Cannot clone helm chart git repository due to {e}")))?;

            fs::rename(tmpdir.path().join(root_path), this.chart_workspace_directory())
                .map_err(|e| to_error(format!("Cannot move helm chart directory due to {e}")))?;
        }
    }

    // fetch the dependencies attached to this chart
    logger.info("🪤 Retrieving Helm chart dependencies".to_string());
    target
        .helm
        .dependency_build(
            this.helm_release_name(),
            this.workspace_directory(),
            this.chart_workspace_directory(),
            &[],
            &[],
            &CommandKiller::from(HELM_CHART_DOWNLOAD_TIMEOUT, target.abort),
            &mut |line| logger.info(line),
            &mut |line| logger.warning(line),
        )
        .map_err(|e| to_error(format!("Cannot fetch chart dependencies: {e:?}")))?;

    // Now we retrieve and prepare the chart values
    match this.chart_values() {
        HelmValueSource::Raw { values } => {
            for value in values {
                logger.info(format!("Preparing Helm values file {}", &value.name));

                let lines = value.content.lines().map(Cow::Borrowed);
                let mut output_path =
                    File::create(this.chart_workspace_directory().join(&value.name)).map_err(|e| {
                        to_error(format!("Cannot create output helm value file {} due to {}", value.name, e))
                    })?;
                write_helm_value_with_replacement(
                    lines,
                    &mut output_path,
                    *this.long_id(),
                    this.name(),
                    &this.service_version(),
                    target.environment.long_id,
                    target.environment.project_long_id,
                    this.environment_variables(),
                    target.kubernetes.loadbalancer_l4_annotations(Some(this.name())),
                )
                .map_err(|e| to_error(format!("Cannot prepare helm value file {} due to {}", value.name, e)))?;
            }
        }
        HelmValueSource::Git {
            git_url,
            get_credentials,
            commit_id,
            values_path,
            ssh_keys,
        } => {
            logger.info(format!(
                "🧲 Grabbing Helm values from git repository {git_url} at commit {commit_id}"
            ));

            let tmpdir = tempfile::tempdir_in(this.workspace_directory())
                .map_err(|e| to_error(format!("Cannot create tempdir {e}")))?;

            let git_creds =
                get_credentials().map_err(|e| to_error(format!("Cannot get git credentials due to {e}")))?;

            git::clone_at_commit(git_url, commit_id, &tmpdir, &git_credentials_callback(&git_creds, ssh_keys))
                .map_err(|e| to_error(format!("Cannot clone helm values git repository due to {e}")))?;

            for value in values_path {
                let Some(filename) = value.file_name() else {
                    logger.warning(format!("Invalid filename for {value:?}"));
                    continue;
                };

                logger.info(format!("Preparing Helm values file {filename:?}"));
                let input_file = File::open(tmpdir.path().join(value))
                    .map_err(|e| to_error(format!("Cannot open value file {filename:?} for helm value due to {e}")))?;

                let lines = BufReader::new(input_file)
                    .lines()
                    .map(|l| Cow::Owned(l.unwrap_or_default()));

                let mut output_path = File::create(this.chart_workspace_directory().join(filename))
                    .map_err(|e| to_error(format!("Cannot create output helm value file {filename:?} due to {e}")))?;
                write_helm_value_with_replacement(
                    lines,
                    &mut output_path,
                    *this.long_id(),
                    this.name(),
                    &this.service_version(),
                    target.environment.long_id,
                    target.environment.project_long_id,
                    this.environment_variables(),
                    target.kubernetes.loadbalancer_l4_annotations(Some(this.name())),
                )
                .map_err(|e| to_error(format!("Cannot prepare helm value file {filename:?} due to {e}")))?;
            }
        }
    }

    Ok(())
}

fn check_resources_are_allowed_to_install<T: CloudProvider>(
    this: &HelmChart<T>,
    target: &DeploymentTarget,
    event_details: EventDetails,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>> {
    if this.is_cluster_wide_resources_allowed() {
        return Ok(());
    }

    logger.info("🔬 Checking deployed resources do not cross namespace boundary".to_string());
    let template_args: Vec<_> = this.helm_template_arguments().collect();
    let template = target
        .helm
        .template_raw(
            this.helm_release_name(),
            this.chart_workspace_directory(),
            target.environment.namespace(),
            &template_args.iter().map(|x| x.as_ref()).collect::<Vec<_>>(),
            &[],
            &CommandKiller::from(HELM_CHART_DOWNLOAD_TIMEOUT, target.abort),
            &mut |line| logger.warning(line),
        )
        .map_err(|e| (event_details.clone(), e))?;

    for document in serde_yaml::Deserializer::from_str(&template) {
        let kube_obj: PartialObjectMeta<()> = PartialObjectMeta::deserialize(document).map_err(|err| {
            error!("Cannot deserialize yaml into kube resource {:?}", err);
            (
                event_details.clone(),
                HelmChartError::RenderingError {
                    chart_name: this.name().to_string(),
                    msg: format!("Cannot deserialize helm template into kube object: {err}"),
                },
            )
        })?;

        // Check that the user is allowed to deploy what he is requesting to install
        is_allowed_namespaced_resource(target.environment.namespace(), &kube_obj).map_err(|err| {
            error!("{err} {kube_obj:?}");
            (
                event_details.clone(),
                HelmChartError::RenderingError {
                    chart_name: this.name().to_string(),
                    msg: err,
                },
            )
        })?;
    }

    Ok(())
}

// * Cannot install CRDS
// * Cannot install in another namespaces
// * Cannot install cluster wide resources (i.e: ClusterIssuer)
fn is_allowed_namespaced_resource(namespace: &str, kube_obj: &PartialObjectMeta<()>) -> Result<(), String> {
    // To find them `kubectl api-resources --namespaced=true`
    const WHITELISTED_RESOURCES: &[&str] = &[
        "Alertmanager",
        "AlertmanagerConfig",
        "Binding",
        "Certificate",
        "CertificateRequest",
        "Challenge",
        "CiliumEndpoint",
        "CiliumNetworkPolicy",
        "CiliumNodeConfig",
        "ConfigMap",
        "ControllerRevision",
        "CronJob",
        "CSIStorageCapacity",
        "DaemonSet",
        "Deployment",
        "Endpoints",
        "EndpointSlice",
        "Event",
        "HorizontalPodAutoscaler",
        "Ingress",
        "Issuer",
        "Job",
        "Lease",
        "LimitRange",
        "LocalSubjectAccessReview",
        "NetworkPolicy",
        "NetworkSet",
        "Order",
        "PersistentVolumeClaim",
        "Pod",
        "PodDisruptionBudget",
        "PodMetrics",
        "PodMonitor",
        "PodTemplate",
        "PolicyEndpoint",
        "Probe",
        "Prometheus",
        "PrometheusAgent",
        "PrometheusRule",
        "ReplicaSet",
        "ReplicationController",
        "ResourceQuota",
        "Role",
        "RoleBinding",
        "ScrapeConfig",
        "Secret",
        "SecurityGroupPolicy",
        "Service",
        "ServiceAccount",
        "ServiceMonitor",
        "StatefulSet",
        "ThanosRuler",
        "VerticalPodAutoscaler",
        "VerticalPodAutoscalerCheckpoint",
    ];

    match (&kube_obj.metadata.namespace, &kube_obj.types) {
        // If object is a CRD it will not get any namespace, same if it is a cluster wide resource
        // helm template does not force the namespace to be set https://github.com/helm/helm/issues/3553
        // so we must whitelist the resources we allow to be installed
        (None, Some(obj)) => {
            if !WHITELISTED_RESOURCES.contains(&obj.kind.as_str()) {
                return Err(format!(
                    "Cannot deploy {} {} as it is a cluster wide resource",
                    &obj.kind, &obj.api_version
                ));
            }
        }
        (Some(requested_ns), _) => {
            if requested_ns != namespace {
                return Err(format!(
                    "Cannot deploy {} {} as it does not target correct namespace. Found {:?} expected {}. To deploy resources outside the environment namespace (or CRD etc..) cluster-wide resource should be allowed",
                    kube_obj.types.as_ref().map(|x| x.kind.as_str()).unwrap_or(""),
                    kube_obj.metadata.name.as_deref().unwrap_or(""),
                    &kube_obj.metadata.namespace,
                    namespace
                ));
            }
        }
        (None, None) => {
            return Err(format!(
                "To deploy resources outside the environment namespace (or CRD etc..) cluster-wide resource should be allowed. Got object {kube_obj:?}"
            ));
        }
    }

    Ok(())
}

fn create_config_map_for_webhook_admission_controller_if_not_exists<T: CloudProvider>(
    this: &HelmChart<T>,
    target: &DeploymentTarget,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    let kube_client = target.kube.client();
    let api_config_map: Api<ConfigMap> = Api::namespaced(kube_client, target.environment.namespace());

    let config_map_name = this.admission_controller_config_map_name();
    let project_id = target.environment.project_long_id;
    let environment_id = target.environment.long_id;
    let helm_chart_id = this.long_id();
    let helm_chart_version = this.version();

    let json_config_map = json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
          "name": config_map_name,
          "namespace": target.environment.namespace(),
          "labels": {
            "qovery.com/project-id": project_id,
            "qovery.com/environment-id": environment_id,
            "qovery.com/service-id": helm_chart_id,
            "qovery.com/service-type": "helm",
          },
        },
        "data": {
          "project-id": project_id,
          "environment-id": environment_id,
          "service-id": helm_chart_id,
          "service-version": helm_chart_version,
          "service-type": "helm",
        },
    });
    let config_map = match serde_json::from_value::<ConfigMap>(json_config_map) {
        Ok(config_map) => config_map,
        Err(err) => {
            return Err(Box::new(
                EngineError::new_k8s_cannot_create_helm_config_map_for_admission_controller(
                    event_details,
                    CommandError::from(err),
                ),
            ));
        }
    };

    if let Err(err) = block_on(api_config_map.patch(
        config_map_name.as_str(),
        &PatchParams::apply("qovery").force(),
        &Patch::Apply(config_map),
    )) {
        return Err(Box::new(
            EngineError::new_k8s_cannot_patch_helm_config_map_for_admission_controller(
                event_details,
                CommandError::new(
                    err.to_string(),
                    Some(
                        format!(
                            "Error while trying to patch config map '{config_map_name}' for helm admission controller"
                        )
                        .to_string(),
                    ),
                    None,
                ),
            ),
        ));
    }
    Ok(())
}

fn extract_namespace_from_helm_args<'a>(args: impl Iterator<Item = Cow<'a, str>>) -> Option<Cow<'a, str>> {
    let mut ns_iter = args.skip_while(|arg| {
        !(*arg == "-n" || *arg == "--namespace" || arg.starts_with("-n=") || arg.starts_with("--namespace="))
    });

    if let Some(arg) = ns_iter.next() {
        if let Some(value) = arg.strip_prefix("--namespace=") {
            return Some(Cow::Owned(value.to_string()));
        }

        if let Some(value) = arg.strip_prefix("-n=") {
            return Some(Cow::Owned(value.to_string()));
        }

        if let Some(next_arg) = ns_iter.next() {
            return Some(next_arg);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use maplit::hashmap;

    #[test]
    fn test_replace_qovery_env_variables() {
        let envs = hashmap! {
            "TOTO".to_string() => VariableInfo { value: "toto_var".to_string(), is_secret: false},
            "LABEL_NAME".to_string() => VariableInfo {value: "toto_label".to_string(), is_secret: false},
            "NGNIX_TAG".to_string() => VariableInfo {value: "42".to_string(), is_secret: false}
        };

        let ret = replace_qovery_env_variable(Cow::Borrowed("    tag: \"qovery.env.NGNIX_TAG\""), &envs);
        assert!(matches!(ret, Ok(line) if line == "    tag: \"42\""));

        let ret = replace_qovery_env_variable(Cow::Borrowed("toto: qovery.env.TOTO"), &envs);
        assert!(matches!(ret, Ok(line) if line == "toto: toto_var"));

        let ret = replace_qovery_env_variable(Cow::Borrowed("qovery.env.LABEL_NAME: qovery.env.TOTO"), &envs);
        assert!(matches!(ret, Ok(line) if line == "toto_label: toto_var"));

        let ret = replace_qovery_env_variable(Cow::Borrowed("wesh wesh"), &envs);
        assert!(matches!(ret, Ok(line) if line == "wesh wesh"));

        let ret = replace_qovery_env_variable(Cow::Borrowed("qovery.env.DO_NOT_EXIST"), &envs);
        assert!(ret.is_err());

        let ret = replace_qovery_env_variable(Cow::Borrowed("qovery.env.QOVERY_"), &envs);
        assert!(ret.is_err());
    }

    #[test]
    fn test_replace_qovery_labels() {
        let label_replacements = (
            "qovery.labels.service",
            vec![
                "qovery.com/service-type: helm".to_string(),
                format!(
                    "qovery.com/service-id: {}",
                    Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()
                ),
                format!(
                    "qovery.com/environment-id: {}",
                    Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
                ),
                format!(
                    "qovery.com/project-id: {}",
                    Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()
                ),
            ],
        );

        // Replacements should happen and produces multiple lines
        let lines = vec![];
        let ret = replace_qovery_labels(
            lines,
            Cow::Borrowed("  - qovery.labels.service "),
            label_replacements.0,
            &label_replacements.1,
        );
        let golden_rod: Vec<Cow<'static, str>> = vec![
            Cow::Owned("  - qovery.com/service-type: helm ".to_string()),
            Cow::Owned("  - qovery.com/service-id: 00000000-0000-0000-0000-000000000000 ".to_string()),
            Cow::Owned("  - qovery.com/environment-id: 11111111-1111-1111-1111-111111111111 ".to_string()),
            Cow::Owned("  - qovery.com/project-id: 22222222-2222-2222-2222-222222222222 ".to_string()),
        ];
        assert!(matches!(ret, Ok(rod) if rod == golden_rod));

        // Nothing match line should stay the same
        let lines = vec![];
        let ret = replace_qovery_labels(
            lines,
            Cow::Borrowed("  - qovery.labels.fake "),
            label_replacements.0,
            &label_replacements.1,
        );
        let golden_rod: Vec<Cow<'static, str>> = vec![Cow::Borrowed("  - qovery.labels.fake ")];
        assert!(matches!(ret, Ok(rod) if rod == golden_rod));
    }

    #[test]
    fn test_write_helm_value_with_replacement() {
        let value_file = r#"
controller:
  name: qovery.service.name
  image:
    repository: quay.io/kubernetes-ingress-controller/nginx-ingress-controller
    tag: "qovery.env.NGINX_TAG"
    pullPolicy: IfNotPresent
    runAsUser: 101
    allowPrivilegeEscalation: true

  # This will fix the issue of HPA not being able to read the metrics.
  # Note that if you enable it for existing deployments, it won't work as the labels are immutable.
  # We recommend setting this to true for new deployments.
  useComponentLabel: false
  labels:
    - qovery.labels.service

  loadBalancer:
    annotations:
      qovery.annotations.loadbalancer

  annotations:
    - qovery.annotations.service

  # Configures the ports the nginx-controller listens on
  containerPort:
    http: 80
    https: 443
"#
        .trim()
        .to_string();

        let service_id = Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
        let env_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let project_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let envs = hashmap! {
            "NGINX_TAG".to_string() => VariableInfo { value: "42".to_string(), is_secret: false},
            "LABEL_NAME".to_string() => VariableInfo {value: "toto_label".to_string(), is_secret: false}
        };
        let mut output: Vec<u8> = vec![];

        let ret = write_helm_value_with_replacement(
            value_file.lines().map(Cow::Borrowed),
            &mut output,
            service_id,
            "my_name",
            "42",
            env_id,
            project_id,
            &envs,
            vec![
                ("custom-annotation-1".to_string(), "custom-value-1".to_string()),
                ("custom-annotation-2".to_string(), "custom-value-2".to_string()),
            ],
        );
        assert!(ret.is_ok());

        let golden_rod = r#"
controller:
  name: my_name
  image:
    repository: quay.io/kubernetes-ingress-controller/nginx-ingress-controller
    tag: "42"
    pullPolicy: IfNotPresent
    runAsUser: 101
    allowPrivilegeEscalation: true

  # This will fix the issue of HPA not being able to read the metrics.
  # Note that if you enable it for existing deployments, it won't work as the labels are immutable.
  # We recommend setting this to true for new deployments.
  useComponentLabel: false
  labels:
    - qovery.com/service-type: helm
    - qovery.com/service-id: 00000000-0000-0000-0000-000000000000
    - qovery.com/environment-id: 11111111-1111-1111-1111-111111111111
    - qovery.com/project-id: 22222222-2222-2222-2222-222222222222

  loadBalancer:
    annotations:
      custom-annotation-1: custom-value-1
      custom-annotation-2: custom-value-2

  annotations:
    - qovery.com/service-version: 42

  # Configures the ports the nginx-controller listens on
  containerPort:
    http: 80
    https: 443
"#
        .trim()
        .to_string();

        let mut golden_rod = golden_rod.lines();
        for line in output.lines() {
            assert_eq!(line.unwrap(), golden_rod.next().unwrap())
        }
    }

    #[test]
    fn test_is_allowed_namespaced_resource() {
        let resource = r#"
apiVersion: v1
kind: Namespace
metadata:
  creationTimestamp: "2023-07-12T19:41:43Z"
  labels:
    kubernetes.io/metadata.name: ze27e5943-z8bb2cdcb
    qovery.com/environment-id: 8bb2cdcb-16d1-45ed-aef3-20436791c0a6
    qovery.com/project-id: e27e5943-04ac-4cb7-97ee-d772622e9f95
  name: ze27e5943-z8bb2cdcb
  resourceVersion: "159811376"
  uid: 597e0308-2a05-4b12-b2ad-6708a9bdb80a
spec: []
       "#;

        // Creating the namespace should not be nok
        let ns: PartialObjectMeta<()> =
            PartialObjectMeta::deserialize(serde_yaml::Deserializer::from_str(resource)).unwrap();
        assert!(is_allowed_namespaced_resource("tesotron", &ns).is_err());

        let resource = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  annotations:
    deployment.kubernetes.io/revision: "69"
    meta.helm.sh/release-name: grpc-gateway
    meta.helm.sh/release-namespace: qovery-dev
  creationTimestamp: "2022-12-21T15:25:09Z"
  generation: 71
  labels:
    app: grpc-gateway
    app.kubernetes.io/managed-by: Helm
  name: grpc-gateway
  namespace: qovery-dev
  resourceVersion: "177248910"
  uid: 61a22418-2d14-40e3-8148-34f612f65baf
spec: []
       "#;

        // Wrong namespace should be nok
        let ns: PartialObjectMeta<()> =
            PartialObjectMeta::deserialize(serde_yaml::Deserializer::from_str(resource)).unwrap();
        assert!(is_allowed_namespaced_resource("tesotron", &ns).is_err());

        let resource = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  annotations:
    deployment.kubernetes.io/revision: "69"
    meta.helm.sh/release-name: grpc-gateway
    meta.helm.sh/release-namespace: qovery-dev
  creationTimestamp: "2022-12-21T15:25:09Z"
  generation: 71
  labels:
    app: grpc-gateway
    app.kubernetes.io/managed-by: Helm
  name: grpc-gateway
  namespace: tesotron
  resourceVersion: "177248910"
  uid: 61a22418-2d14-40e3-8148-34f612f65baf
spec: []
       "#;

        // Wrong namespace should be nok
        let ns: PartialObjectMeta<()> =
            PartialObjectMeta::deserialize(serde_yaml::Deserializer::from_str(resource)).unwrap();
        assert!(is_allowed_namespaced_resource("tesotron", &ns).is_ok());

        let resource = r#"
apiVersion: cert-manager.io/v1
kind: ClusterIssuer
metadata:
  annotations:
    meta.helm.sh/release-name: cert-manager-configs
    meta.helm.sh/release-namespace: cert-manager
  creationTimestamp: "2023-07-07T08:57:41Z"
  generation: 1
  labels:
    app.kubernetes.io/managed-by: Helm
  name: letsencrypt-qovery
  resourceVersion: "154912174"
  uid: 2785c950-bd20-432d-b6bc-0eb680090362
spec:
       "#;

        // Cluster wide resources are NOK
        let ns: PartialObjectMeta<()> =
            PartialObjectMeta::deserialize(serde_yaml::Deserializer::from_str(resource)).unwrap();
        assert!(is_allowed_namespaced_resource("tesotron", &ns).is_err());
    }

    #[test]
    fn should_return_empty_when_no_namespace_args() {
        let args = vec![Cow::Borrowed("--atomic")];

        let namespace = extract_namespace_from_helm_args(args.into_iter());

        assert!(namespace.is_none());
    }

    #[test]
    fn should_extract_namespace_with_short_option_format() {
        let args = vec![
            Cow::Borrowed("--atomic"),
            Cow::Borrowed("-n"),
            Cow::Borrowed("my-namespace"),
        ];

        let namespace = extract_namespace_from_helm_args(args.into_iter());

        assert_eq!(namespace, Some(Cow::Borrowed("my-namespace")));
    }

    #[test]
    fn should_ignore_incomplete_namespace_flag() {
        let args = vec![Cow::Borrowed("--atomic"), Cow::Borrowed("-n")];

        let namespace = extract_namespace_from_helm_args(args.into_iter());

        assert!(namespace.is_none());
    }

    #[test]
    fn should_extract_namespace_with_equals_syntax() {
        let args = vec![Cow::Borrowed("--atomic"), Cow::Borrowed("--namespace=production")];

        let namespace = extract_namespace_from_helm_args(args.into_iter());

        assert_eq!(namespace, Some(Cow::Borrowed("production")));
    }

    #[test]
    fn should_extract_namespace_with_long_option_format() {
        let args = vec![
            Cow::Borrowed("--atomic"),
            Cow::Borrowed("--namespace"),
            Cow::Borrowed("staging"),
        ];

        let namespace = extract_namespace_from_helm_args(args.into_iter());

        assert_eq!(namespace, Some(Cow::Borrowed("staging")));
    }

    #[test]
    fn should_extract_namespace_with_several_namespaces() {
        let args = vec![
            Cow::Borrowed("--atomic"),
            Cow::Borrowed("--namespace"),
            Cow::Borrowed("staging"),
            Cow::Borrowed("-n"),
            Cow::Borrowed("dev"),
        ];

        let namespace = extract_namespace_from_helm_args(args.into_iter());

        // take the first one, but it should not be allowed to have args with several namespaces
        assert_eq!(namespace, Some(Cow::Borrowed("staging")));
    }
}
