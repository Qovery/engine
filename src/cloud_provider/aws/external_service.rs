use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::common::kubernetes::{
    do_stateless_service_cleanup, get_stateless_resource_information,
};
use crate::cloud_provider::common::models::{EnvironmentVariable, EnvironmentVariableDataTemplate};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Action, Application as AApplication, Create, Delete, Pause, Service, ServiceType,
    StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause};
use crate::models::Context;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct ExternalService {
    context: Context,
    id: String,
    action: Action,
    name: String,
    total_cpus: String,
    total_ram_in_mib: u32,
    image: Image,
    environment_variables: Vec<EnvironmentVariable>,
}

impl ExternalService {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        total_cpus: String,
        total_ram_in_mib: u32,
        image: Image,
        environment_variables: Vec<EnvironmentVariable>,
    ) -> Self {
        ExternalService {
            context,
            id: id.to_string(),
            action,
            name: name.to_string(),
            total_cpus,
            total_ram_in_mib,
            image,
            environment_variables,
        }
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(
            format!("external-service-{}-{}", self.name(), self.id()),
            50,
        )
    }

    fn context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> TeraContext {
        let mut context = self.default_tera_context(kubernetes, environment);
        let commit_id = self.image().commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);

        match &self.image().registry_url {
            Some(registry_url) => context.insert("image_name_with_tag", registry_url.as_str()),
            None => {
                let image_name_with_tag = self.image().name_with_tag();
                warn!("there is no registry url, use image name with tag with the default container registry: {}", image_name_with_tag.as_str());
                context.insert("image_name_with_tag", image_name_with_tag.as_str());
            }
        }

        let environment_variables = self
            .environment_variables
            .iter()
            .map(|ev| EnvironmentVariableDataTemplate {
                key: ev.key.clone(),
                value: ev.value.clone(),
            })
            .collect::<Vec<_>>();

        context.insert("environment_variables", &environment_variables);

        context
    }

    fn delete(&self, target: &DeploymentTarget, is_error: bool) -> Result<(), EngineError> {
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let helm_release_name = self.helm_release_name();
        let selector = format!("app={}", self.name());

        if is_error {
            let _ = get_stateless_resource_information(kubernetes, environment, selector.as_str())?;
        }

        // clean the resource
        let _ = do_stateless_service_cleanup(kubernetes, environment, helm_release_name.as_str())?;

        Ok(())
    }
}

impl crate::cloud_provider::service::ExternalService for ExternalService {}

impl crate::cloud_provider::service::Application for ExternalService {
    fn image(&self) -> &Image {
        &self.image
    }

    fn set_image(&mut self, image: Image) {
        self.image = image;
    }
}

impl StatelessService for ExternalService {}

impl Service for ExternalService {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::ExternalService
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.image.commit_id.as_str()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn private_port(&self) -> Option<u16> {
        None
    }

    fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    fn total_instances(&self) -> u16 {
        1
    }
}

impl Create for ExternalService {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!(
            "AWS.external_service.on_create() called for {}",
            self.name()
        );
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let context = self.context(kubernetes, environment);
        let workspace_dir = self.workspace_directory();

        let from_dir = format!("{}/common/services/q-job", self.context.lib_root_dir());
        let _ = cast_simple_error_to_engine_error(
            crate::cloud_provider::service::ExternalService::engine_error_scope(self),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                from_dir.as_str(),
                workspace_dir.as_str(),
                &context,
            ),
        )?;

        // render
        // TODO check the rendered files?
        let helm_release_name = self.helm_release_name();
        let kubernetes_config_file_path = kubernetes.config_file_path()?;

        // do exec helm upgrade and return the last deployment status
        let helm_history_row = cast_simple_error_to_engine_error(
            crate::cloud_provider::service::ExternalService::engine_error_scope(self),
            self.context.execution_id(),
            crate::cmd::helm::helm_exec_with_upgrade_history(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name.as_str(),
                workspace_dir.as_str(),
                Timeout::Default,
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables(),
            ),
        )?;

        // check deployment status
        if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
            return Err(crate::cloud_provider::service::ExternalService::engine_error(self, EngineErrorCause::User(
                "Your External Service didn't start for some reason. \
                Are you sure your External Service is correctly running? You can give a try by running \
                locally `docker run`. You can also check the External Service log from the web \
                interface or the CLI with `qovery log`",
            ), format!("External Service {} has failed to start ⤬", self.name_with_id()),
            ));
        }

        // check job status
        match crate::cmd::kubectl::kubectl_exec_is_job_ready_with_retry(
            kubernetes_config_file_path.as_str(),
            environment.namespace(),
            self.name.as_str(),
            kubernetes
                .cloud_provider()
                .credentials_environment_variables(),
        ) {
            Ok(Some(true)) => {}
            _ => {
                return Err(
                    crate::cloud_provider::service::ExternalService::engine_error(
                        self,
                        EngineErrorCause::Internal,
                        format!(
                            "External Service {} with id {} failed to start after several retries",
                            self.name(),
                            self.id()
                        ),
                    ),
                );
            }
        }

        Ok(())
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "AWS.external_service.on_create_error() called for {}",
            self.name()
        );
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let kubernetes_config_file_path = kubernetes.config_file_path()?;
        let helm_release_name = self.helm_release_name();

        let history_rows = cast_simple_error_to_engine_error(
            crate::cloud_provider::service::ExternalService::engine_error_scope(self),
            self.context.execution_id(),
            crate::cmd::helm::helm_exec_history(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name.as_str(),
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables(),
            ),
        )?;

        if history_rows.len() == 1 {
            cast_simple_error_to_engine_error(
                crate::cloud_provider::service::ExternalService::engine_error_scope(self),
                self.context.execution_id(),
                crate::cmd::helm::helm_exec_uninstall(
                    kubernetes_config_file_path.as_str(),
                    environment.namespace(),
                    helm_release_name.as_str(),
                    kubernetes
                        .cloud_provider()
                        .credentials_environment_variables(),
                ),
            )?;
        }

        Ok(())
    }
}

impl Pause for ExternalService {
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!("AWS.external_service.on_pause() called for {}", self.name());
        self.delete(target, false)
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "AWS.external_service.on_pause_error() called for {}",
            self.name()
        );
        self.delete(target, true)
    }
}

impl Delete for ExternalService {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!(
            "AWS.external_service.on_delete() called for {}",
            self.name()
        );
        self.delete(target, false)
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        warn!(
            "AWS.external_service.on_delete_error() called for {}",
            self.name()
        );
        self.delete(target, true)
    }
}
