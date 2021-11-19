use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::models::{
    EnvironmentVariable, EnvironmentVariableDataTemplate, Storage, StorageDataTemplate,
};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, deploy_stateless_service_error, deploy_user_stateless_service,
    scale_down_application, send_progress_on_long_task, Action, Create, Delete, Helm, Pause, Service, ServiceType,
    StatelessService,
};
use crate::cloud_provider::utilities::{print_action, sanitize_name, validate_k8s_required_cpu_and_burstable};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl::ScalingKind::{Deployment, Statefulset};
use crate::error::EngineErrorCause::Internal;
use crate::error::{EngineError, EngineErrorScope};
use crate::models::{Context, Listen, Listener, Listeners, ListenersHelper};
use ::function_name::named;
use std::fmt;
use std::str::FromStr;

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
    storage: Vec<Storage<StorageType>>,
    environment_variables: Vec<EnvironmentVariable>,
    listeners: Listeners,
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
        storage: Vec<Storage<StorageType>>,
        environment_variables: Vec<EnvironmentVariable>,
        listeners: Listeners,
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
            storage,
            environment_variables,
            listeners,
        }
    }

    fn is_stateful(&self) -> bool {
        self.storage.len() > 0
    }

    fn cloud_provider_name(&self) -> &str {
        "digitalocean"
    }

    fn struct_name(&self) -> &str {
        "application"
    }
}

impl crate::cloud_provider::service::Application for Application {
    fn image(&self) -> &Image {
        &self.image
    }

    fn set_image(&mut self, image: Image) {
        self.image = image;
    }
}

impl Helm for Application {
    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.name, self.id), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/digitalocean/charts/q-application", self.context.lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        String::new()
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        String::new()
    }
}

impl StatelessService for Application {}

impl Service for Application {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Application
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn sanitized_name(&self) -> String {
        sanitize_name("app", self.name())
    }

    fn version(&self) -> String {
        self.image.commit_id.clone()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn private_port(&self) -> Option<u16> {
        self.private_port
    }

    fn start_timeout(&self) -> Timeout<u32> {
        Timeout::Value((self.start_timeout_in_seconds + 10) * 4)
    }

    fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    fn cpu_burst(&self) -> String {
        self.cpu_burst.to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    fn total_instances(&self) -> u16 {
        self.total_instances
    }

    fn publicly_accessible(&self) -> bool {
        self.private_port.is_some()
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);
        let commit_id = self.image.commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);

        match &self.image.registry_url {
            Some(registry_url) => context.insert("image_name_with_tag", registry_url.as_str()),
            None => {
                let image_name_with_tag = self.image.name_with_tag();
                warn!(
                    "there is no registry url, use image name with tag with the default container registry: {}",
                    image_name_with_tag.as_str()
                );
                context.insert("image_name_with_tag", image_name_with_tag.as_str());
            }
        }

        let cpu_limits = match validate_k8s_required_cpu_and_burstable(
            &ListenersHelper::new(&self.listeners),
            &self.context.execution_id(),
            &self.id,
            self.total_cpus(),
            self.cpu_burst(),
        ) {
            Ok(l) => l,
            Err(e) => {
                return Err(EngineError::new(
                    Internal,
                    EngineErrorScope::Application(self.id().to_string(), self.name().to_string()),
                    self.context.execution_id(),
                    Some(e.to_string()),
                ));
            }
        };
        context.insert("cpu_burst", &cpu_limits.cpu_limit);

        let environment_variables = self
            .environment_variables
            .iter()
            .map(|ev| EnvironmentVariableDataTemplate {
                key: ev.key.clone(),
                value: ev.value.clone(),
            })
            .collect::<Vec<_>>();

        context.insert("environment_variables", &environment_variables);

        if self.image.registry_name.is_some() {
            context.insert("is_registry_secret", &true);
            context.insert(
                "registry_secret",
                &"do-container-registry-secret-for-cluster".to_string(),
            );
        } else {
            context.insert("is_registry_secret", &false);
        };

        let storage = self
            .storage
            .iter()
            .map(|s| StorageDataTemplate {
                id: s.id.clone(),
                name: s.name.clone(),
                storage_type: match s.storage_type {
                    StorageType::Standard => "do-block-storage",
                }
                .to_string(),
                size_in_gib: s.size_in_gib,
                mount_point: s.mount_point.clone(),
                snapshot_retention_in_days: s.snapshot_retention_in_days,
            })
            .collect::<Vec<_>>();

        let is_storage = storage.len() > 0;

        context.insert("storage", &storage);
        context.insert("is_storage", &is_storage);
        context.insert("clone", &false);
        context.insert("start_timeout_in_seconds", &self.start_timeout_in_seconds);

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }

        Ok(context)
    }

    fn selector(&self) -> String {
        format!("appId={}", self.id)
    }

    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::Application(self.id().to_string(), self.name().to_string())
    }
}

impl Create for Application {
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_user_stateless_service(target, self)
        })
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_stateless_service_error(target, self)
        })
    }
}

impl Pause for Application {
    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Pause, || {
            scale_down_application(
                target,
                self,
                0,
                if self.is_stateful() { Statefulset } else { Deployment },
            )
        })
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        Ok(())
    }
}

impl Delete for Application {
    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
            delete_stateless_service(target, self, false)
        })
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
            delete_stateless_service(target, self, true)
        })
    }
}

impl Listen for Application {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum StorageType {
    Standard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Region {
    NewYorkCity1,
    NewYorkCity2,
    NewYorkCity3,
    Amsterdam2,
    Amsterdam3,
    SanFrancisco1,
    SanFrancisco2,
    SanFrancisco3,
    Singapore,
    London,
    Frankfurt,
    Toronto,
    Bangalore,
}

impl Region {
    pub fn as_str(&self) -> &str {
        match self {
            Region::NewYorkCity1 => "nyc1",
            Region::NewYorkCity2 => "nyc2",
            Region::NewYorkCity3 => "nyc3",
            Region::Amsterdam2 => "ams2",
            Region::Amsterdam3 => "ams3",
            Region::SanFrancisco1 => "sfo1",
            Region::SanFrancisco2 => "sfo2",
            Region::SanFrancisco3 => "sfo3",
            Region::Singapore => "sgp1",
            Region::London => "lon1",
            Region::Frankfurt => "fra1",
            Region::Toronto => "tor1",
            Region::Bangalore => "blr1",
        }
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Region::NewYorkCity1 => write!(f, "nyc1"),
            Region::NewYorkCity2 => write!(f, "nyc2"),
            Region::NewYorkCity3 => write!(f, "nyc3"),
            Region::Amsterdam2 => write!(f, "ams2"),
            Region::Amsterdam3 => write!(f, "ams3"),
            Region::SanFrancisco1 => write!(f, "sfo1"),
            Region::SanFrancisco2 => write!(f, "sfo2"),
            Region::SanFrancisco3 => write!(f, "sfo3"),
            Region::Singapore => write!(f, "sgp1"),
            Region::London => write!(f, "lon1"),
            Region::Frankfurt => write!(f, "fra1"),
            Region::Toronto => write!(f, "tor1"),
            Region::Bangalore => write!(f, "blr1"),
        }
    }
}

impl FromStr for Region {
    type Err = ();

    fn from_str(s: &str) -> Result<Region, ()> {
        match s {
            "nyc1" => Ok(Region::NewYorkCity1),
            "nyc2" => Ok(Region::NewYorkCity2),
            "nyc3" => Ok(Region::NewYorkCity3),
            "ams2" => Ok(Region::Amsterdam2),
            "ams3" => Ok(Region::Amsterdam3),
            "sfo1" => Ok(Region::SanFrancisco1),
            "sfo2" => Ok(Region::SanFrancisco2),
            "sfo3" => Ok(Region::SanFrancisco3),
            "sgp1" => Ok(Region::Singapore),
            "lon1" => Ok(Region::London),
            "fra1" => Ok(Region::Frankfurt),
            "tor1" => Ok(Region::Toronto),
            "blr1" => Ok(Region::Bangalore),
            _ => Err(()),
        }
    }
}
