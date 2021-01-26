use crate::cloud_provider::service::{StatefulService, StatelessService};
use crate::error::EngineError;
use crate::unit_conversion::cpu_string_to_float;

pub struct Environment {
    namespace: String,
    pub kind: Kind,
    pub id: String,
    pub project_id: String,
    pub owner_id: String,
    pub organization_id: String,
    pub stateless_services: Vec<Box<dyn StatelessService>>,
    pub stateful_services: Vec<Box<dyn StatefulService>>,
}

impl Environment {
    pub fn new(
        kind: Kind,
        id: &str,
        project_id: &str,
        owner_id: &str,
        organization_id: &str,
        stateless_services: Vec<Box<dyn StatelessService>>,
        stateful_services: Vec<Box<dyn StatefulService>>,
    ) -> Self {
        Environment {
            namespace: format!("{}-{}", project_id, id),
            kind,
            id: id.to_string(),
            project_id: project_id.to_string(),
            owner_id: owner_id.to_string(),
            organization_id: organization_id.to_string(),
            stateless_services,
            stateful_services,
        }
    }

    pub fn namespace(&self) -> &str {
        self.namespace.as_str()
    }

    pub fn is_valid(&self) -> Result<(), EngineError> {
        for service in self.stateful_services.iter() {
            match service.is_valid() {
                Err(err) => return Err(err),
                _ => {}
            }
        }

        for service in self.stateless_services.iter() {
            match service.is_valid() {
                Err(err) => return Err(err),
                _ => {}
            }
        }

        Ok(())
    }

    /// compute the required resources for this environment from
    /// applications, external services, routers, and databases
    /// Note: Even if external services don't run on the targeted Kubernetes cluster, it requires CPU and memory resources to run the container(s)
    pub fn required_resources(&self) -> EnvironmentResources {
        let mut total_cpu_for_stateless_services: f32 = 0.0;
        let mut total_ram_in_mib_for_stateless_services: u32 = 0;
        let mut required_pods = self.stateless_services.len() as u16;

        for service in &self.stateless_services {
            total_cpu_for_stateless_services += cpu_string_to_float(&service.total_cpus());
            total_ram_in_mib_for_stateless_services += &service.total_ram_in_mib();
            required_pods += service.total_instances();
        }

        let mut total_cpu_for_stateful_services: f32 = 0.0;
        let mut total_ram_in_mib_for_stateful_services: u32 = 0;

        match self.kind {
            Kind::Development => {
                // development means stateful services are running on Kubernetes
                for service in &self.stateful_services {
                    total_cpu_for_stateful_services += cpu_string_to_float(&service.total_cpus());
                    total_ram_in_mib_for_stateful_services += &service.total_ram_in_mib();
                }
            }
            Kind::Production => {} // production means databases are running on managed services - so it consumes 0 cpu
        };

        match self.kind {
            crate::cloud_provider::environment::Kind::Production => {}
            crate::cloud_provider::environment::Kind::Development => {
                for service in &self.stateful_services {
                    required_pods += service.total_instances();
                }
            }
        }

        EnvironmentResources {
            pods: required_pods,
            cpu: total_cpu_for_stateless_services + total_cpu_for_stateful_services,
            ram_in_mib: total_ram_in_mib_for_stateless_services + total_ram_in_mib_for_stateless_services,
        }
    }
}

pub enum Kind {
    Production,
    Development,
}

pub struct EnvironmentResources {
    pub pods: u16,
    pub cpu: f32,
    pub ram_in_mib: u32,
}
