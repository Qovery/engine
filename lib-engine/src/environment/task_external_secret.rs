use crate::cmd::command::CommandKiller;
use crate::environment::action::deploy_external_secrets::{
    DeployExternalSecretsResult, deploy_helm_external_secrets, eso_companion_release_name,
};
use crate::environment::models::abort::Abort;
use crate::environment::models::environment::Environment;
use crate::environment::report::logger::{EnvLogger, EnvProgressLogger};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::helm::ChartInfo;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Action;
use std::collections::HashMap;
use std::sync::Arc;

/// Resolved values collected when deploying services external secrets.
pub struct EsoExternalSecretsPreBuild {
    pub app_external_secrets_values: Vec<DeployExternalSecretsResult>,
    pub container_external_secrets_values: Vec<DeployExternalSecretsResult>,
    pub job_external_secrets_values: Vec<DeployExternalSecretsResult>,
    pub helm_external_secrets_values: Vec<DeployExternalSecretsResult>,
    pub terraform_external_secrets_values: Vec<DeployExternalSecretsResult>,
}

/// Deploy all Services ExternalSecrets defined in separate helm release
/// (we need to deploy them separately to compute the image tag needed for buildable services)
/// Returns the kube names of services that need to be cleaned up if any
pub fn handle_service_external_secrets(
    environment: &mut Environment,
    infra_ctx: &InfrastructureContext,
    abort: &dyn Abort,
) -> Result<(), Box<EngineError>> {
    let logger = Arc::new(infra_ctx.kubernetes().logger().clone_dyn());

    let EsoExternalSecretsPreBuild {
        app_external_secrets_values,
        container_external_secrets_values,
        job_external_secrets_values,
        helm_external_secrets_values,
        terraform_external_secrets_values,
    } = {
        let target = DeploymentTarget::new(infra_ctx, environment, abort)?;
        let namespace = target.environment.namespace();

        let app_external_secrets_values = environment
            .applications
            .iter()
            .map(|app| {
                // Apply external secrets only:
                // * to applications to be deployed
                // * if external secrets non empty
                if app.action() != &Action::Create || app.external_secrets().is_empty() {
                    return Ok(DeployExternalSecretsResult {
                        external_secrets_groups_with_values: vec![],
                    });
                }

                let env_logger = EnvLogger::new(app.as_service(), EnvironmentStep::Deploy, logger.clone());
                let progress_logger = EnvProgressLogger::new(&env_logger);
                deploy_helm_external_secrets(
                    app.kube_name(),
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
                .inspect_err(|err| env_logger.send_error(*err.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let container_external_secrets_values = environment
            .containers
            .iter()
            .map(|container| {
                // Apply external secrets only:
                // * to applications to be deployed
                // * if external secrets non empty
                if container.action() != &Action::Create || container.external_secrets().is_empty() {
                    return Ok(DeployExternalSecretsResult {
                        external_secrets_groups_with_values: vec![],
                    });
                }

                let env_logger = EnvLogger::new(container.as_service(), EnvironmentStep::Deploy, logger.clone());
                let progress_logger = EnvProgressLogger::new(&env_logger);
                deploy_helm_external_secrets(
                    container.kube_name(),
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
                )
                .inspect_err(|err| env_logger.send_error(*err.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let job_external_secrets_values = environment
            .jobs
            .iter()
            // INFO (qov-1569) We need to deploy external secrets regardless of the job action
            .map(|job| {
                // Apply external secrets only:
                // * to applications to be deployed
                // * if external secrets non empty
                if job.external_secrets().is_empty() {
                    return Ok(DeployExternalSecretsResult {
                        external_secrets_groups_with_values: vec![],
                    });
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
                .inspect_err(|err| env_logger.send_error(*err.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let helm_external_secrets_values = environment
            .helm_charts
            .iter()
            // Apply external secrets only to applications to be deployed
            .filter(|helm_chart| helm_chart.action() == &Action::Create)
            // Apply external secrets only if non empty
            .filter(|app| !app.external_secrets().is_empty())
            .map(|helm_chart| {
                // Apply external secrets only:
                // * to applications to be deployed
                // * if external secrets non empty
                if helm_chart.action() != &Action::Create || helm_chart.external_secrets().is_empty() {
                    return Ok(DeployExternalSecretsResult {
                        external_secrets_groups_with_values: vec![],
                    });
                }

                let env_logger = EnvLogger::new(helm_chart.as_service(), EnvironmentStep::Deploy, logger.clone());
                let progress_logger = EnvProgressLogger::new(&env_logger);
                deploy_helm_external_secrets(
                    helm_chart.kube_name(),
                    *helm_chart.long_id(),
                    "helm",
                    namespace,
                    helm_chart.external_secrets(),
                    target.environment.long_id,
                    target.environment.project_long_id,
                    helm_chart.workspace_directory_path(),
                    helm_chart.lib_root_directory(),
                    &target,
                    helm_chart.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
                    &progress_logger,
                )
                .inspect_err(|err| env_logger.send_error(*err.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let terraform_external_secrets_values = environment
            .terraform_services
            .iter()
            // Apply external secrets only if non empty
            .filter(|app| !app.external_secrets().is_empty())
            // INFO (qov-1569) We need to deploy external secrets regardless of the terraform service action
            .map(|terraform_service| {
                let env_logger =
                    EnvLogger::new(terraform_service.as_service(), EnvironmentStep::Deploy, logger.clone());
                let progress_logger = EnvProgressLogger::new(&env_logger);
                deploy_helm_external_secrets(
                    terraform_service.kube_name(),
                    *terraform_service.long_id(),
                    "terraform",
                    namespace,
                    terraform_service.external_secrets(),
                    target.environment.long_id,
                    target.environment.project_long_id,
                    terraform_service.workspace_directory_path(),
                    terraform_service.lib_root_directory(),
                    &target,
                    terraform_service.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
                    &progress_logger,
                )
                .inspect_err(|err| env_logger.send_error(*err.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        EsoExternalSecretsPreBuild {
            app_external_secrets_values,
            container_external_secrets_values,
            job_external_secrets_values,
            helm_external_secrets_values,
            terraform_external_secrets_values,
        }
    }; // DeploymentTarget dropped here — environment is mutable again

    // And inject resolved values into Application build environment variables
    for (app, deploy_external_secret_result) in environment.applications.iter_mut().zip(app_external_secrets_values) {
        for external_secret_values in deploy_external_secret_result.external_secrets_groups_with_values {
            if let Some(group) = app
                .external_secrets_mut()
                .iter_mut()
                .find(|it| it.external_secret_kube_name == external_secret_values.external_secret_name)
            {
                group.secret_name = external_secret_values.target_secret_name.to_string();
            }

            if let Some(build) = app.build_mut() {
                for (key, var_info) in external_secret_values.group_values {
                    build.environment_variables.insert(key, var_info.value);
                }
            }
        }
    }

    // No secret value to inject into build
    // Check only if target secret name needs to be changed
    for (container, deploy_external_secret_result) in
        environment.containers.iter_mut().zip(container_external_secrets_values)
    {
        for external_secret_values in deploy_external_secret_result.external_secrets_groups_with_values {
            if let Some(group) = container
                .external_secrets_mut()
                .iter_mut()
                .find(|it| it.external_secret_kube_name == external_secret_values.external_secret_name)
            {
                group.secret_name = external_secret_values.target_secret_name.to_string();
            }
        }
    }

    // Inject resolved values into Job build environment variables
    for (job, deploy_external_secret_result) in environment.jobs.iter_mut().zip(job_external_secrets_values) {
        for external_secret_values in deploy_external_secret_result.external_secrets_groups_with_values {
            if let Some(group) = job
                .external_secrets_mut()
                .iter_mut()
                .find(|it| it.external_secret_kube_name == external_secret_values.external_secret_name)
            {
                group.secret_name = external_secret_values.target_secret_name.to_string();
            }
            if let Some(build) = job.build_mut() {
                for (key, var_info) in external_secret_values.group_values {
                    build.environment_variables.insert(key, var_info.value);
                }
            }
        }
    }

    // Store resolved values in HelmChart for later use in chart preparation
    for (helm_chart, deploy_external_secret_result) in
        environment.helm_charts.iter_mut().zip(helm_external_secrets_values)
    {
        for external_secret_values in &deploy_external_secret_result.external_secrets_groups_with_values {
            if let Some(group) = helm_chart
                .external_secrets_mut()
                .iter_mut()
                .find(|it| it.external_secret_kube_name == external_secret_values.external_secret_name)
            {
                group.secret_name = external_secret_values.target_secret_name.to_string();
            }
        }
        let all_secret_values = deploy_external_secret_result
            .external_secrets_groups_with_values
            .into_iter()
            .flat_map(|g| g.group_values)
            .collect::<HashMap<_, _>>();
        helm_chart.set_resolved_eso_values(all_secret_values);
    }

    // Inject resolved values into TerraformService build environment variables
    for (terraform_service, deploy_external_secret_result) in environment
        .terraform_services
        .iter_mut()
        .zip(terraform_external_secrets_values)
    {
        for external_secret_values in deploy_external_secret_result.external_secrets_groups_with_values {
            if let Some(group) = terraform_service
                .external_secrets_mut()
                .iter_mut()
                .find(|it| it.external_secret_kube_name == external_secret_values.external_secret_name)
            {
                group.secret_name = external_secret_values.target_secret_name.to_string();
            }
            if let Some(build) = terraform_service.build_mut() {
                for (key, var_info) in external_secret_values.group_values {
                    build.environment_variables.insert(key, var_info.value);
                }
            }
        }
    }

    Ok(())
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
