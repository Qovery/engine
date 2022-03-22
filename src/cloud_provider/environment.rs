use crate::cloud_provider::service::{Action, Application, Database, Router, StatefulService, StatelessService};
use crate::unit_conversion::cpu_string_to_float;

pub struct Environment {
    namespace: String,
    pub id: String,
    pub project_id: String,
    pub owner_id: String,
    pub organization_id: String,
    pub action: Action,
    pub applications: Vec<Box<dyn Application>>,
    pub routers: Vec<Box<dyn Router>>,
    pub databases: Vec<Box<dyn Database>>,
}

impl Environment {
    pub fn new(
        id: &str,
        project_id: &str,
        owner_id: &str,
        organization_id: &str,
        action: Action,
        applications: Vec<Box<dyn Application>>,
        routers: Vec<Box<dyn Router>>,
        databases: Vec<Box<dyn Database>>,
    ) -> Self {
        Environment {
            namespace: format!("{}-{}", project_id, id),
            id: id.to_string(),
            project_id: project_id.to_string(),
            owner_id: owner_id.to_string(),
            organization_id: organization_id.to_string(),
            action,
            applications,
            routers,
            databases,
        }
    }

    pub fn stateless_services(&self) -> Vec<&dyn StatelessService> {
        let mut stateless_services: Vec<&dyn StatelessService> =
            Vec::with_capacity(self.applications.len() + self.routers.len());
        stateless_services.extend_from_slice(
            self.applications
                .iter()
                .map(|x| x.as_stateless_service())
                .collect::<Vec<_>>()
                .as_slice(),
        );
        stateless_services.extend_from_slice(
            self.routers
                .iter()
                .map(|x| x.as_stateless_service())
                .collect::<Vec<_>>()
                .as_slice(),
        );

        stateless_services
    }

    pub fn stateful_services(&self) -> Vec<&dyn StatefulService> {
        self.databases
            .iter()
            .map(|x| x.as_stateful_service())
            .collect::<Vec<_>>()
    }

    pub fn namespace(&self) -> &str {
        self.namespace.as_str()
    }

    /// compute the required resources for this environment from
    /// applications, external services, routers, and databases
    /// Note: Even if external services don't run on the targeted Kubernetes cluster, it requires CPU and memory resources to run the container(s)
    pub fn required_resources(&self) -> EnvironmentResources {
        let mut total_cpu_for_stateless_services: f32 = 0.0;
        let mut total_ram_in_mib_for_stateless_services: u32 = 0;
        let mut required_pods = self.stateless_services().len() as u32;

        for service in self.stateless_services() {
            match service.action() {
                Action::Create | Action::Nothing => {
                    total_cpu_for_stateless_services += cpu_string_to_float(&service.total_cpus());
                    total_ram_in_mib_for_stateless_services += &service.total_ram_in_mib();
                    required_pods += service.max_instances()
                }
                Action::Delete | Action::Pause => {}
            }
        }

        let mut total_cpu_for_stateful_services: f32 = 0.0;
        let mut total_ram_in_mib_for_stateful_services: u32 = 0;
        for service in self.stateful_services() {
            if service.is_managed_service() {
                // If it is a managed service, we don't care of its resources as it is not managed by us
                continue;
            }

            match service.action() {
                Action::Pause | Action::Delete => {
                    total_cpu_for_stateful_services += cpu_string_to_float(service.total_cpus());
                    total_ram_in_mib_for_stateful_services += service.total_ram_in_mib();
                    required_pods += service.max_instances()
                }
                Action::Create | Action::Nothing => {}
            }
        }

        EnvironmentResources {
            pods: required_pods,
            cpu: total_cpu_for_stateless_services + total_cpu_for_stateful_services,
            ram_in_mib: total_ram_in_mib_for_stateless_services + total_ram_in_mib_for_stateful_services,
        }
    }
}

pub struct EnvironmentResources {
    pub pods: u32,
    pub cpu: f32,
    pub ram_in_mib: u32,
}
