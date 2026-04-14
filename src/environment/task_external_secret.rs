use crate::cmd::command::CommandKiller;
use crate::environment::action::deploy_external_secrets::{
    deploy_helm_external_secrets, eso_companion_release_name, external_secrets_exist_for_service,
};
use crate::environment::models::abort::Abort;
use crate::environment::models::environment::Environment;
use crate::environment::report::logger::{EnvLogger, EnvProgressLogger};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::helm::ChartInfo;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::io_models::variable_utils::VariableInfo;
use std::collections::HashMap;
use std::sync::Arc;

/// Resolved values collected when deploying services external secrets.
pub struct EsoExternalSecretsPreBuild {
    pub app_external_secrets_values: Vec<HashMap<String, VariableInfo>>,
    pub job_external_secrets_values: Vec<HashMap<String, VariableInfo>>,
    pub helm_external_secrets_values: Vec<HashMap<String, VariableInfo>>,
    pub terraform_external_secrets_values: Vec<HashMap<String, VariableInfo>>,
    // For this vec:
    // * if new external secrets are empty => contains the existing external secrets' kube names to clean
    //   at the end of deployment. The purpose is to not uninstall them immediately in case of service rollback
    // * otherwise => it is empty (no clean to do)
    pub external_secrets_to_clean_after_environment_deployment: Vec<String>,
}

/// Deploy all Services ExternalSecrets defined in separate helm release
/// (we need to deploy them separately to compute the image tag needed for buildable services)
/// Returns the kube names of services that need to be cleaned up if any
pub fn deploy_services_external_secrets(
    environment: &mut Environment,
    infra_ctx: &InfrastructureContext,
    abort: &dyn Abort,
) -> Result<Vec<String>, Box<EngineError>> {
    let logger = Arc::new(infra_ctx.kubernetes().logger().clone_dyn());

    let EsoExternalSecretsPreBuild {
        app_external_secrets_values: app_resolved,
        job_external_secrets_values: job_resolved,
        helm_external_secrets_values: helm_resolved,
        terraform_external_secrets_values: terraform_resolved,
        external_secrets_to_clean_after_environment_deployment: orphaned_kube_names,
    } = {
        let target = DeploymentTarget::new(infra_ctx, environment, abort)?;
        let kube_client = target.kube.client();
        let namespace = target.environment.namespace();
        let mut external_secrets_to_delete_post_deployment: Vec<String> = Vec::new();

        let app_r = environment
            .applications
            .iter()
            .map(|app| {
                if app.external_secrets().is_empty() {
                    // Compute external secrets to delete after service deployment
                    if external_secrets_exist_for_service(&kube_client, namespace, *app.long_id()) {
                        external_secrets_to_delete_post_deployment.push(app.kube_name().to_string());
                    }
                    return Ok(HashMap::new());
                }
                let env_logger = EnvLogger::new(app.as_service(), EnvironmentStep::Deploy, logger.clone());
                let progress_logger = EnvProgressLogger::new(&env_logger);
                deploy_helm_external_secrets(
                    app.kube_name(),
                    app.name(),
                    *app.long_id(),
                    "application",
                    namespace,
                    app.external_secrets(),
                    target.environment.long_id,
                    target.environment.project_long_id,
                    app.workspace_directory_path(),
                    app.lib_root_directory(),
                    &target,
                    app.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
                    &progress_logger,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        for container in environment.containers.iter() {
            if container.external_secrets().is_empty() {
                // Compute external secrets to delete after service deployment
                if external_secrets_exist_for_service(&kube_client, namespace, *container.long_id()) {
                    external_secrets_to_delete_post_deployment.push(container.kube_name().to_string());
                }
                continue;
            }
            let env_logger = EnvLogger::new(container.as_service(), EnvironmentStep::Deploy, logger.clone());
            let progress_logger = EnvProgressLogger::new(&env_logger);
            deploy_helm_external_secrets(
                container.kube_name(),
                container.name(),
                *container.long_id(),
                "container",
                namespace,
                container.external_secrets(),
                target.environment.long_id,
                target.environment.project_long_id,
                container.workspace_directory_path(),
                container.lib_root_directory(),
                &target,
                container.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
                &progress_logger,
            )?;
        }

        let job_r = environment
            .jobs
            .iter()
            .map(|job| {
                if job.external_secrets().is_empty() {
                    // Compute external secrets to delete after service deployment
                    if external_secrets_exist_for_service(&kube_client, namespace, *job.long_id()) {
                        external_secrets_to_delete_post_deployment.push(job.kube_name().to_string());
                    }
                    return Ok(HashMap::new());
                }
                let service_type = if job.job_schedule().is_cronjob() {
                    "cronjob"
                } else {
                    "job"
                };
                let env_logger = EnvLogger::new(job.as_service(), EnvironmentStep::Deploy, logger.clone());
                let progress_logger = EnvProgressLogger::new(&env_logger);
                deploy_helm_external_secrets(
                    job.kube_name(),
                    job.name(),
                    *job.long_id(),
                    service_type,
                    namespace,
                    job.external_secrets(),
                    target.environment.long_id,
                    target.environment.project_long_id,
                    job.workspace_directory_path(),
                    job.lib_root_directory(),
                    &target,
                    job.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
                    &progress_logger,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let helm_r = environment
            .helm_charts
            .iter()
            .map(|chart| {
                if chart.external_secrets().is_empty() {
                    // Compute external secrets to delete after service deployment
                    if external_secrets_exist_for_service(&kube_client, namespace, *chart.long_id()) {
                        external_secrets_to_delete_post_deployment.push(chart.kube_name().to_string());
                    }
                    return Ok(HashMap::new());
                }
                let env_logger = EnvLogger::new(chart.as_service(), EnvironmentStep::Deploy, logger.clone());
                let progress_logger = EnvProgressLogger::new(&env_logger);
                deploy_helm_external_secrets(
                    chart.kube_name(),
                    chart.name(),
                    *chart.long_id(),
                    "helm",
                    namespace,
                    chart.external_secrets(),
                    target.environment.long_id,
                    target.environment.project_long_id,
                    chart.workspace_directory_path(),
                    chart.lib_root_directory(),
                    &target,
                    chart.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
                    &progress_logger,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let terraform_r = environment
            .terraform_services
            .iter()
            .map(|terraform| {
                if terraform.external_secrets().is_empty() {
                    // Compute external secrets to delete after service deployment
                    if external_secrets_exist_for_service(&kube_client, namespace, *terraform.long_id()) {
                        external_secrets_to_delete_post_deployment.push(terraform.kube_name().to_string());
                    }
                    return Ok(HashMap::new());
                }
                let env_logger = EnvLogger::new(terraform.as_service(), EnvironmentStep::Deploy, logger.clone());
                let progress_logger = EnvProgressLogger::new(&env_logger);
                deploy_helm_external_secrets(
                    terraform.kube_name(),
                    terraform.name(),
                    *terraform.long_id(),
                    "terraform",
                    namespace,
                    terraform.external_secrets(),
                    target.environment.long_id,
                    target.environment.project_long_id,
                    terraform.workspace_directory_path(),
                    terraform.lib_root_directory(),
                    &target,
                    terraform.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
                    &progress_logger,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        EsoExternalSecretsPreBuild {
            app_external_secrets_values: app_r,
            job_external_secrets_values: job_r,
            helm_external_secrets_values: helm_r,
            terraform_external_secrets_values: terraform_r,
            external_secrets_to_clean_after_environment_deployment: external_secrets_to_delete_post_deployment,
        }
    }; // DeploymentTarget dropped here — environment is mutable again

    // Inject resolved values into Application build environment variables
    for (app, resolved) in environment.applications.iter_mut().zip(app_resolved) {
        if let Some(build) = app.build_mut() {
            for (key, var_info) in resolved {
                build.environment_variables.insert(key, var_info.value);
            }
        }
    }

    // Inject resolved values into Job build environment variables
    for (job, resolved) in environment.jobs.iter_mut().zip(job_resolved) {
        if let Some(build) = job.build_mut() {
            for (key, var_info) in resolved {
                build.environment_variables.insert(key, var_info.value);
            }
        }
    }

    // Store resolved values in HelmChart for later use in chart preparation
    for (chart, resolved) in environment.helm_charts.iter_mut().zip(helm_resolved) {
        chart.set_resolved_eso_values(resolved);
    }

    // Inject resolved values into TerraformService build environment variables
    for (terraform, resolved) in environment.terraform_services.iter_mut().zip(terraform_resolved) {
        if let Some(build) = terraform.build_mut() {
            for (key, var_info) in resolved {
                build.environment_variables.insert(key, var_info.value);
            }
        }
    }

    Ok(orphaned_kube_names)
}

/// Called after a Delete deployment regardless of success or failure.
pub fn uninstall_external_secrets_after_delete_successful(deployment_target: &DeploymentTarget) {
    let namespace = deployment_target.environment.namespace();
    let environment = deployment_target.environment;

    let services_kube_names: Vec<&str> = std::iter::empty()
        .chain(environment.applications.iter().map(|s| s.kube_name()))
        .chain(environment.containers.iter().map(|s| s.kube_name()))
        .chain(environment.jobs.iter().map(|s| s.kube_name()))
        .chain(environment.helm_charts.iter().map(|s| s.kube_name()))
        .chain(environment.terraform_services.iter().map(|s| s.kube_name()))
        .collect();

    for kube_name in services_kube_names {
        let companion_release = eso_companion_release_name(kube_name);
        let companion_chart = ChartInfo::new_from_release_name(&companion_release, namespace);
        if let Err(e) =
            deployment_target
                .helm
                .uninstall(&companion_chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {})
        {
            warn!("Failed to uninstall external secrets for release '{companion_release}': {e}");
        }
    }
}

/// Uninstall ESO companion releases for the given kube names.
pub fn uninstall_external_secrets_orphans(kube_names: &[String], deployment_target: &DeploymentTarget) {
    if kube_names.is_empty() {
        return;
    }

    let namespace = deployment_target.environment.namespace();
    for kube_name in kube_names {
        let companion_release = eso_companion_release_name(kube_name);
        let companion_chart = ChartInfo::new_from_release_name(&companion_release, namespace);
        if let Err(e) =
            deployment_target
                .helm
                .uninstall(&companion_chart, &[], &CommandKiller::never(), &mut |_| {}, &mut |_| {})
        {
            warn!("Failed to uninstall external secrets for release '{companion_release}': {e}");
        }
    }
}
