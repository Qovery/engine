use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::CloudProvider;
use crate::container_registry::ContainerRegistry;
use crate::io_models::application::Application;
use crate::io_models::container::Container;
use crate::io_models::context::Context;
use crate::io_models::database::Database;
use crate::io_models::helm_chart::HelmChart;
use crate::io_models::job::Job;
use crate::io_models::router::Router;
use crate::io_models::Action;
use crate::models::application::{ApplicationError, ApplicationService};
use crate::models::container::{ContainerError, ContainerService};
use crate::models::database::{DatabaseError, DatabaseService};
use crate::models::helm_chart::{HelmChartError, HelmChartService};
use crate::models::job::{JobError, JobService};
use crate::models::router::RouterError;
use crate::utilities::base64_replace_comma_to_new_line;
use crate::{cloud_provider::environment::Environment, models::router::RouterAdvancedSettings};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentRequest {
    pub execution_id: String,
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub project_long_id: Uuid,
    pub organization_long_id: Uuid,
    pub action: Action,
    #[serde(default = "default_max_parallel_build")]
    pub max_parallel_build: u32,
    #[serde(default = "default_max_parallel_deploy")]
    pub max_parallel_deploy: u32,
    pub applications: Vec<Application>,
    pub containers: Vec<Container>,
    pub jobs: Vec<Job>,
    pub routers: Vec<Router>,
    pub databases: Vec<Database>,
    #[serde(default)]
    pub helms: Vec<HelmChart>,
}

fn default_max_parallel_build() -> u32 {
    1u32
}

fn default_max_parallel_deploy() -> u32 {
    1u32
}

#[derive(thiserror::Error, Debug)]
pub enum DomainError {
    #[error("Invalid application: {0}")]
    ApplicationError(#[from] ApplicationError),
    #[error("Invalid container: {0}")]
    ContainerError(#[from] ContainerError),
    #[error("Invalid router: {0}")]
    RouterError(#[from] RouterError),
    #[error("Invalid database: {0}")]
    DatabaseError(#[from] DatabaseError),
    #[error("Invalid job: {0}")]
    JobError(#[from] JobError),
    #[error("Invalid helm chart: {0}")]
    HelmChartError(#[from] HelmChartError),
}

impl EnvironmentRequest {
    pub fn to_environment_domain(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        container_registry: &dyn ContainerRegistry,
        cluster: &dyn Kubernetes,
    ) -> Result<Environment, DomainError> {
        let applications: Result<Vec<Box<dyn ApplicationService>>, ApplicationError> = self
            .applications
            .iter()
            .cloned()
            .map(|srv| {
                let build = srv.to_build(
                    container_registry.registry_info(),
                    context.qovery_api.clone(),
                    cluster.cpu_architectures(),
                );
                srv.to_application_domain(context, build, cloud_provider)
            })
            .collect();
        let applications = applications?;

        let containers: Result<Vec<Box<dyn ContainerService>>, ContainerError> = self
            .containers
            .iter()
            .cloned()
            .map(|srv| srv.to_container_domain(context, cloud_provider, container_registry))
            .collect();
        let containers = containers?;

        let mut routers = Vec::with_capacity(self.routers.len());
        for router in &self.routers {
            let mut router_advanced_settings = RouterAdvancedSettings::default();

            for app in &self.applications {
                for route in &router.routes {
                    if route.service_long_id == app.long_id {
                        // disable custom domain check for this router
                        if !app.advanced_settings.deployment_custom_domain_check_enabled {
                            router_advanced_settings.custom_domain_check_enabled = false;
                        }
                        // whitelist source range
                        if app.advanced_settings.network_ingress_whitelist_source_range
                            != RouterAdvancedSettings::whitelist_source_range_default_value()
                        {
                            router_advanced_settings.whitelist_source_range =
                                Some(app.advanced_settings.network_ingress_whitelist_source_range.clone());
                        }
                        // denylist source range
                        if app.advanced_settings.network_ingress_denylist_source_range != *"" {
                            router_advanced_settings.denylist_source_range =
                                Some(app.advanced_settings.network_ingress_denylist_source_range.clone());
                        }
                        // basic auth
                        if app.advanced_settings.network_ingress_basic_auth_env_var != *"" {
                            match app
                                .environment_vars
                                .get(&app.advanced_settings.network_ingress_basic_auth_env_var)
                            {
                                Some(value) => {
                                    let secret = base64_replace_comma_to_new_line(value.clone()).map_err(|_| {
                                        DomainError::RouterError(RouterError::BasicAuthEnvVarBase64DecodeError {
                                            env_var_name: app
                                                .advanced_settings
                                                .network_ingress_basic_auth_env_var
                                                .to_string(),
                                            env_var_value: value.clone(),
                                        })
                                    })?;
                                    router_advanced_settings.basic_auth = Some(secret);
                                }
                                None => {
                                    return Err(DomainError::RouterError(RouterError::BasicAuthEnvVarNotFound {
                                        env_var_name: app
                                            .advanced_settings
                                            .network_ingress_basic_auth_env_var
                                            .to_string(),
                                    }))
                                }
                            }
                        }
                    }
                }
            }

            for container in &self.containers {
                for route in &router.routes {
                    if route.service_long_id == container.long_id {
                        // disable custom domain check for this router
                        if !container.advanced_settings.deployment_custom_domain_check_enabled {
                            router_advanced_settings.custom_domain_check_enabled = false;
                        }
                        // whitelist source range
                        if container.advanced_settings.network_ingress_whitelist_source_range
                            != RouterAdvancedSettings::whitelist_source_range_default_value()
                        {
                            router_advanced_settings.whitelist_source_range = Some(
                                container
                                    .advanced_settings
                                    .network_ingress_whitelist_source_range
                                    .clone(),
                            );
                        }
                        // denylist source range
                        if container.advanced_settings.network_ingress_denylist_source_range != *"" {
                            router_advanced_settings.denylist_source_range = Some(
                                container
                                    .advanced_settings
                                    .network_ingress_denylist_source_range
                                    .clone(),
                            );
                        }
                        // basic auth
                        if container.advanced_settings.network_ingress_basic_auth_env_var != *"" {
                            match container
                                .environment_vars
                                .get(&container.advanced_settings.network_ingress_basic_auth_env_var)
                            {
                                Some(value) => {
                                    let secret = base64_replace_comma_to_new_line(value.clone()).map_err(|_| {
                                        DomainError::RouterError(RouterError::BasicAuthEnvVarBase64DecodeError {
                                            env_var_name: container
                                                .advanced_settings
                                                .network_ingress_basic_auth_env_var
                                                .to_string(),
                                            env_var_value: value.clone(),
                                        })
                                    })?;
                                    router_advanced_settings.basic_auth = Some(secret);
                                }
                                None => {
                                    return Err(DomainError::RouterError(RouterError::BasicAuthEnvVarNotFound {
                                        env_var_name: container
                                            .advanced_settings
                                            .network_ingress_basic_auth_env_var
                                            .to_string(),
                                    }))
                                }
                            }
                        }
                    }
                }
            }

            match router.to_router_domain(context, router_advanced_settings, cloud_provider) {
                Ok(router) => routers.push(router),
                Err(err) => {
                    return Err(DomainError::RouterError(err));
                }
            }
        }

        let databases: Result<Vec<Box<dyn DatabaseService>>, DatabaseError> = self
            .databases
            .iter()
            .cloned()
            .map(|srv| srv.to_database_domain(context, cloud_provider))
            .collect();
        let databases = databases?;

        let jobs: Result<Vec<Box<dyn JobService>>, JobError> = self
            .jobs
            .iter()
            .cloned()
            .map(|srv| srv.to_job_domain(context, cloud_provider, container_registry, cluster))
            .collect();
        let jobs = jobs?;

        let helm_charts: Result<Vec<Box<dyn HelmChartService>>, HelmChartError> = self
            .helms
            .iter()
            .cloned()
            .map(|helm_chart| helm_chart.to_helm_chart_domain(context, cloud_provider))
            .collect();
        let helm_charts = helm_charts?;

        Ok(Environment::new(
            self.long_id,
            self.name.clone(),
            self.kube_name.clone(),
            self.project_long_id,
            self.organization_long_id,
            self.action.to_service_action(),
            context,
            self.max_parallel_build,
            self.max_parallel_deploy,
            applications,
            containers,
            routers,
            databases,
            jobs,
            helm_charts,
        ))
    }
}
