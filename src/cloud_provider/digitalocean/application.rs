use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::digitalocean::{common, DO};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::{
    Action, Application as CApplication, Create, Delete, Pause, Service, ServiceType,
    StatelessService,
};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::constants::DIGITAL_OCEAN_TOKEN;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope,
};
use crate::models::Context;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize)]
struct EnvironmentVariableDataTemplate {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Application {
    context: Context,
    id: String,
    action: Action,
    name: String,
    private_port: Option<u16>,
    total_cpus: String,
    cpu_burst: String,
    total_ram_in_mib: u32,
    total_instances: u16,
    start_timeout_in_seconds: u32,
    image: Image,
    environment_variables: Vec<EnvironmentVariable>,
}

impl Application {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        private_port: Option<u16>,
        total_cpus: String,
        cpu_burst: String,
        total_ram_in_mib: u32,
        total_instances: u16,
        start_timeout_in_seconds: u32,
        image: Image,
        environment_variables: Vec<EnvironmentVariable>,
    ) -> Self {
        Application {
            context,
            id: id.to_string(),
            action,
            name: name.to_string(),
            private_port,
            total_cpus,
            cpu_burst,
            total_ram_in_mib,
            total_instances,
            start_timeout_in_seconds,
            image,
            environment_variables,
        }
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.name, self.id), 50)
    }

    fn workspace_directory(&self) -> String {
        crate::fs::workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("applications/{}", self.name),
        )
    }

    /*    fn context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> TeraContext {
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

        //TODO: no storage for the moment
        context.insert("clone", &false);
        context.insert("start_timeout_in_seconds", &self.start_timeout_in_seconds);

        context
    }*/
}

impl Create for Application {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        info!(
            "DigitalOcean.application.on_create() called for {}",
            self.name
        );
        /*
        let (kubernetes, environment) = match target {
            DeploymentTarget::ManagedServices(k, env) => (*k, *env),
            DeploymentTarget::SelfHosted(k, env) => (*k, *env),
        };

        let digitalocean = kubernetes
            .cloud_provider()
            .as_any()
            .downcast_ref::<DO>()
            .unwrap();

        let context = self.context(kubernetes, environment);
        let workspace_dir = self.workspace_directory();

        let from_dir = format!(
            "{}/digitalocean/charts/q-application",
            self.context.lib_root_dir()
        );

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
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
        let do_credentials_envs = vec![(DIGITAL_OCEAN_TOKEN, digitalocean.token.as_str())];

        let kubernetes_config_file_path = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            common::kubernetes_config_path(
                workspace_dir.as_str(),
                environment.organization_id.as_str(),
                kubernetes.id(),
                digitalocean.token.as_str(),
            ),
        )?;

        // do exec helm upgrade and return the last deployment status
        let helm_history_row = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::cmd::helm::helm_exec_with_upgrade_history(
                kubernetes_config_file_path.as_str(),
                environment.namespace(),
                helm_release_name.as_str(),
                workspace_dir.as_str(),
                Timeout::Value(self.start_timeout_in_seconds),
            ),
        )?;

        // check deployment status
        if helm_history_row.is_none() || !helm_history_row.unwrap().is_successfully_deployed() {
            return Err(self.engine_error(
                EngineErrorCause::User(
                    "Your application didn't start for some reason. \
                Are you sure your application is correctly running? You can give a try by running \
                locally `qovery run`. You can also check the application log from the web \
                interface or the CLI with `qovery log`",
                ),
                format!("Application {} has failed to start ⤬", self.name_with_id()),
            ));
        }

        // TODO: check app status

               let selector = format!("app={}", self.name);


               let _ = cast_simple_error_to_engine_error(
                   self.engine_error_scope(),
                   self.context.execution_id(),
                   crate::cmd::kubectl::kubectl_exec_is_pod_ready_with_retry(
                       kubernetes_config_file_path.as_str(),
                       environment.namespace(),
                       selector.as_str(),
                       do_credentials_envs,
                   ),
               )?;
        */
        Ok(())
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}
