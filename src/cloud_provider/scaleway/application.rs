use std::fmt;
use std::str::FromStr;

use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::models::{
    EnvironmentVariable, EnvironmentVariableDataTemplate, Storage, StorageDataTemplate,
};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, deploy_stateless_service_error, deploy_user_stateless_service,
    scale_down_application, send_progress_on_long_task, Action, Application as CApplication, Create, Delete, Helm,
    Pause, Service, ServiceType, StatelessService,
};
use crate::cloud_provider::utilities::{print_action, sanitize_name, validate_k8s_required_cpu_and_burstable};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl::ScalingKind::{Deployment, Statefulset};
use crate::error::EngineErrorCause::Internal;
use crate::error::{EngineError, EngineErrorScope};
use crate::models::{Context, Listen, Listener, Listeners, ListenersHelper, Port};
use ::function_name::named;

pub struct Application {
    context: Context,
    id: String,
    action: Action,
    name: String,
    ports: Vec<Port>,
    total_cpus: String,
    cpu_burst: String,
    total_ram_in_mib: u32,
    min_instances: u32,
    max_instances: u32,
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
        ports: Vec<Port>,
        total_cpus: String,
        cpu_burst: String,
        total_ram_in_mib: u32,
        min_instances: u32,
        max_instances: u32,
        start_timeout_in_seconds: u32,
        image: Image,
        storage: Vec<Storage<StorageType>>,
        environment_variables: Vec<EnvironmentVariable>,
        listeners: Listeners,
    ) -> Application {
        Application {
            context,
            id: id.to_string(),
            action,
            name: name.to_string(),
            ports,
            total_cpus,
            cpu_burst,
            total_ram_in_mib,
            min_instances,
            max_instances,
            start_timeout_in_seconds,
            image,
            storage,
            environment_variables,
            listeners,
        }
    }

    fn is_stateful(&self) -> bool {
        !self.storage.is_empty()
    }

    fn cloud_provider_name(&self) -> &str {
        "scaleway"
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
    fn helm_selector(&self) -> Option<String> {
        self.selector()
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.name(), self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/scaleway/charts/q-application", self.context.lib_root_dir())
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
        self.ports
            .iter()
            .find(|port| port.publicly_accessible)
            .map(|port| port.port)
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

    fn min_instances(&self) -> u32 {
        self.min_instances
    }

    fn max_instances(&self) -> u32 {
        self.max_instances
    }

    fn publicly_accessible(&self) -> bool {
        self.private_port().is_some()
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);
        let commit_id = self.image().commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);

        match &self.image().registry_url {
            Some(registry_url) => context.insert(
                "image_name_with_tag",
                format!("{}/{}", registry_url.as_str(), self.image().name_with_tag()).as_str(),
            ),
            None => {
                let image_name_with_tag = self.image().name_with_tag();
                warn!(
                    "there is no registry url, use image name with tag with the default container registry: {}",
                    image_name_with_tag.as_str()
                );
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
        context.insert("ports", &self.ports);

        match self.image.registry_name.as_ref() {
            Some(_) => {
                context.insert("is_registry_secret", &true);
                context.insert("registry_secret_name", &format!("registry-token-{}", &self.id));
            }
            None => {
                context.insert("is_registry_secret", &false);
            }
        };

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

        let storage = self
            .storage
            .iter()
            .map(|s| StorageDataTemplate {
                id: s.id.clone(),
                name: s.name.clone(),
                storage_type: match s.storage_type {
                    // TODO(benjaminch): Switch to proper storage class
                    // Note: Seems volume storage type are not supported, only blocked storage for the time being
                    // https://github.com/scaleway/scaleway-csi/tree/master/examples/kubernetes#different-storageclass
                    StorageType::BlockSsd => "scw-sbv-ssd-0", // "b_ssd",
                    StorageType::LocalSsd => "l_ssd",
                }
                .to_string(),
                size_in_gib: s.size_in_gib,
                mount_point: s.mount_point.clone(),
                snapshot_retention_in_days: s.snapshot_retention_in_days,
            })
            .collect::<Vec<_>>();

        let is_storage = !storage.is_empty();

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

        // container registry credentials
        context.insert(
            "container_registry_docker_json_config",
            self.image
                .clone()
                .registry_docker_json_config
                .unwrap_or("".to_string())
                .as_str(),
        );

        Ok(context)
    }

    fn selector(&self) -> Option<String> {
        Some(format!("appId={}", self.id))
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

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde_derive::Serialize, serde_derive::Deserialize)]
pub enum StorageType {
    #[serde(rename = "b_ssd")]
    BlockSsd,
    #[serde(rename = "l_ssd")]
    LocalSsd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Region {
    Paris,
    Amsterdam,
    Warsaw,
}

impl Region {
    // TODO(benjaminch): improve / refactor this!
    pub fn as_str(&self) -> &str {
        match self {
            Region::Paris => "fr-par",
            Region::Amsterdam => "nl-ams",
            Region::Warsaw => "pl-waw",
        }
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Region::Paris => write!(f, "fr-par"),
            Region::Amsterdam => write!(f, "nl-ams"),
            Region::Warsaw => write!(f, "pl-waw"),
        }
    }
}

impl FromStr for Region {
    type Err = ();

    fn from_str(s: &str) -> Result<Region, ()> {
        match s {
            "fr-par" => Ok(Region::Paris),
            "nl-ams" => Ok(Region::Amsterdam),
            "pl-waw" => Ok(Region::Warsaw),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Zone {
    Paris1,
    Paris2,
    Amsterdam1,
    Warsaw1,
}

impl Zone {
    // TODO(benjaminch): improve / refactor this!
    pub fn as_str(&self) -> &str {
        match self {
            Zone::Paris1 => "fr-par-1",
            Zone::Paris2 => "fr-par-2",
            Zone::Amsterdam1 => "nl-ams-1",
            Zone::Warsaw1 => "pl-waw-1",
        }
    }

    pub fn region(&self) -> Region {
        match self {
            Zone::Paris1 => Region::Paris,
            Zone::Paris2 => Region::Paris,
            Zone::Amsterdam1 => Region::Amsterdam,
            Zone::Warsaw1 => Region::Warsaw,
        }
    }

    // TODO(benjaminch): improve / refactor this!
    pub fn region_str(&self) -> &str {
        match self {
            Zone::Paris1 => "fr-par",
            Zone::Paris2 => "fr-par",
            Zone::Amsterdam1 => "nl-ams",
            Zone::Warsaw1 => "pl-waw",
        }
    }
}

impl fmt::Display for Zone {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Zone::Paris1 => write!(f, "fr-par-1"),
            Zone::Paris2 => write!(f, "fr-par-2"),
            Zone::Amsterdam1 => write!(f, "nl-ams-1"),
            Zone::Warsaw1 => write!(f, "pl-waw-1"),
        }
    }
}

impl FromStr for Zone {
    type Err = ();

    fn from_str(s: &str) -> Result<Zone, ()> {
        match s {
            "fr-par-1" => Ok(Zone::Paris1),
            "fr-par-2" => Ok(Zone::Paris2),
            "nl-ams-1" => Ok(Zone::Amsterdam1),
            "pl-waw-1" => Ok(Zone::Warsaw1),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Region, Zone};
    use std::str::FromStr;

    #[test]
    fn test_region_to_str() {
        assert_eq!("fr-par", Region::Paris.as_str());
        assert_eq!("nl-ams", Region::Amsterdam.as_str());
        assert_eq!("pl-waw", Region::Warsaw.as_str());
    }

    #[test]
    fn test_region_from_str() {
        assert_eq!(Region::from_str("fr-par"), Ok(Region::Paris));
        assert_eq!(Region::from_str("nl-ams"), Ok(Region::Amsterdam));
        assert_eq!(Region::from_str("pl-waw"), Ok(Region::Warsaw));
    }

    #[test]
    fn test_zone_to_str() {
        assert_eq!("fr-par-1", Zone::Paris1.as_str());
        assert_eq!("fr-par-2", Zone::Paris2.as_str());
        assert_eq!("nl-ams-1", Zone::Amsterdam1.as_str());
        assert_eq!("pl-waw-1", Zone::Warsaw1.as_str());
    }

    #[test]
    fn test_zone_from_str() {
        assert_eq!(Zone::from_str("fr-par-1"), Ok(Zone::Paris1));
        assert_eq!(Zone::from_str("fr-par-2"), Ok(Zone::Paris2));
        assert_eq!(Zone::from_str("nl-ams-1"), Ok(Zone::Amsterdam1));
        assert_eq!(Zone::from_str("pl-waw-1"), Ok(Zone::Warsaw1));
    }

    #[test]
    fn test_zone_region() {
        assert_eq!(Zone::Paris1.region(), Region::Paris);
        assert_eq!(Zone::Paris2.region(), Region::Paris);
        assert_eq!(Zone::Amsterdam1.region(), Region::Amsterdam);
        assert_eq!(Zone::Warsaw1.region(), Region::Warsaw);
    }
}
