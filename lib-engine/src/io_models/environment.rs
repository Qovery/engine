use crate::environment::models::agentic_workflow::{AgenticWorkflowError, AgenticWorkflowService};
use crate::environment::models::application::{ApplicationError, ApplicationService};
use crate::environment::models::container::{ContainerError, ContainerService};
use crate::environment::models::database::{DatabaseError, DatabaseService};
use crate::environment::models::environment::Environment;
use crate::environment::models::helm_chart::{HelmChartError, HelmChartService};
use crate::environment::models::job::{JobError, JobService};
use crate::environment::models::router::{RouterAdvancedSettings, RouterError};
use crate::environment::models::terraform_service::{TerraformServiceError, TerraformServiceTrait};
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::container_registry::InteractWithRegistry;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::agentic_workflow::AgenticWorkflow;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::application::Application;
use crate::io_models::container::Container;
use crate::io_models::context::Context;
use crate::io_models::database::Database;
use crate::io_models::helm_chart::HelmChart;
use crate::io_models::job::Job;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::router::Router;
use crate::io_models::terraform::TerraformService;
use crate::io_models::variable_utils::VariableInfo;
use crate::io_models::{Action, QoveryIdentifier};
use crate::utilities::base64_replace_comma_to_new_line;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
    #[serde(default)]
    pub terraform_services: Vec<TerraformService>,
    #[serde(default)]
    pub agentic_workflows: Vec<AgenticWorkflow>,
    #[serde(default = "default_annotations_groups")]
    pub annotations_groups: BTreeMap<Uuid, AnnotationsGroup>,
    #[serde(default = "default_labels_groups")]
    pub labels_groups: BTreeMap<Uuid, LabelsGroup>,
}

fn default_max_parallel_build() -> u32 {
    1u32
}

fn default_max_parallel_deploy() -> u32 {
    1u32
}

fn default_annotations_groups() -> BTreeMap<Uuid, AnnotationsGroup> {
    BTreeMap::new()
}

fn default_labels_groups() -> BTreeMap<Uuid, LabelsGroup> {
    BTreeMap::new()
}

fn resolve_basic_auth_secret(
    gateway_api_basic_auth_env_var: &str,
    ingress_basic_auth_env_var: &str,
    environment_vars_with_infos: &BTreeMap<String, VariableInfo>,
) -> Result<Option<String>, DomainError> {
    // Temporary migration fallback:
    // Prefer Gateway API basic auth env var, but fallback to legacy ingress env var
    // while both stacks coexist. This should be removed when NGINX ingress is sunset.
    if let Some(variable_infos) = environment_vars_with_infos.get(gateway_api_basic_auth_env_var) {
        let secret = base64_replace_comma_to_new_line(variable_infos.value.clone()).map_err(|_| {
            DomainError::RouterError(RouterError::BasicAuthEnvVarBase64DecodeError {
                env_var_name: gateway_api_basic_auth_env_var.to_string(),
                env_var_value: variable_infos.value.clone(),
            })
        })?;
        return Ok(Some(secret));
    }

    if let Some(variable_infos) = environment_vars_with_infos.get(ingress_basic_auth_env_var) {
        let secret = base64_replace_comma_to_new_line(variable_infos.value.clone()).map_err(|_| {
            DomainError::RouterError(RouterError::BasicAuthEnvVarBase64DecodeError {
                env_var_name: ingress_basic_auth_env_var.to_string(),
                env_var_value: variable_infos.value.clone(),
            })
        })?;
        return Ok(Some(secret));
    }

    if !gateway_api_basic_auth_env_var.is_empty() || !ingress_basic_auth_env_var.is_empty() {
        return Err(DomainError::RouterError(RouterError::BasicAuthEnvVarNotFound {
            env_var_name: if !gateway_api_basic_auth_env_var.is_empty() {
                gateway_api_basic_auth_env_var.to_string()
            } else {
                ingress_basic_auth_env_var.to_string()
            },
        }));
    }

    Ok(None)
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
    #[error("Invalid terraform service: {0}")]
    TerraformServiceError(#[from] TerraformServiceError),
    #[error("Invalid agentic workflow: {0}")]
    AgenticWorkflowError(#[from] AgenticWorkflowError),
}

impl EnvironmentRequest {
    pub fn to_environment_domain(
        &self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        container_registry: &dyn InteractWithRegistry,
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
                    &QoveryIdentifier::new(*cluster.long_id()),
                );
                srv.to_application_domain(context, build, cloud_provider, &self.annotations_groups, &self.labels_groups)
            })
            .collect();
        let applications = applications?;

        let containers: Result<Vec<Box<dyn ContainerService>>, ContainerError> = self
            .containers
            .iter()
            .cloned()
            .map(|srv| {
                srv.to_container_domain(
                    context,
                    cloud_provider,
                    container_registry,
                    cluster,
                    &self.annotations_groups,
                    &self.labels_groups,
                )
            })
            .collect();
        let containers = containers?;

        let mut routers = Vec::with_capacity(self.routers.len());
        for router in &self.routers {
            let mut router_advanced_settings = RouterAdvancedSettings::default();
            let mut annotations_groups_ids = BTreeSet::new();
            let mut labels_groups_ids = BTreeSet::new();

            for app in &self.applications {
                for route in &router.routes {
                    if route.service_long_id == app.long_id {
                        annotations_groups_ids.clone_from(&app.annotations_group_ids);
                        labels_groups_ids.clone_from(&app.labels_group_ids);

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
                        if let Some(secret) = resolve_basic_auth_secret(
                            &app.advanced_settings.network_gateway_api_basic_auth_env_var,
                            &app.advanced_settings.network_ingress_basic_auth_env_var,
                            &app.environment_vars_with_infos,
                        )? {
                            router_advanced_settings.basic_auth = Some(secret);
                        }
                    }
                }
            }

            for container in &self.containers {
                for route in &router.routes {
                    if route.service_long_id == container.long_id {
                        annotations_groups_ids.clone_from(&container.annotations_group_ids);
                        labels_groups_ids.clone_from(&container.labels_group_ids);

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
                        if let Some(secret) = resolve_basic_auth_secret(
                            &container.advanced_settings.network_gateway_api_basic_auth_env_var,
                            &container.advanced_settings.network_ingress_basic_auth_env_var,
                            &container.environment_vars_with_infos,
                        )? {
                            router_advanced_settings.basic_auth = Some(secret);
                        }
                    }
                }
            }

            for helm in &self.helms {
                for route in &router.routes {
                    if route.service_long_id == helm.long_id {
                        // whitelist source range
                        if helm.advanced_settings.network_ingress_whitelist_source_range
                            != RouterAdvancedSettings::whitelist_source_range_default_value()
                        {
                            router_advanced_settings.whitelist_source_range =
                                Some(helm.advanced_settings.network_ingress_whitelist_source_range.clone());
                        }
                        // denylist source range
                        if helm.advanced_settings.network_ingress_denylist_source_range != *"" {
                            router_advanced_settings.denylist_source_range =
                                Some(helm.advanced_settings.network_ingress_denylist_source_range.clone());
                        }
                        // basic auth
                        if let Some(secret) = resolve_basic_auth_secret(
                            &helm.advanced_settings.network_gateway_api_basic_auth_env_var,
                            &helm.advanced_settings.network_ingress_basic_auth_env_var,
                            &helm.environment_vars_with_infos,
                        )? {
                            router_advanced_settings.basic_auth = Some(secret);
                        }
                    }
                }
            }

            let annotations_groups = annotations_groups_ids
                .iter()
                .flat_map(|annotations_group_id| self.annotations_groups.get(annotations_group_id))
                .cloned()
                .collect_vec();
            let labels_groups = labels_groups_ids
                .iter()
                .flat_map(|labels_group_id| self.labels_groups.get(labels_group_id))
                .cloned()
                .collect_vec();

            match router.to_router_domain(
                context,
                router_advanced_settings,
                cloud_provider,
                annotations_groups,
                labels_groups,
            ) {
                Ok(router) => routers.push(router),
                Err(err) => {
                    return Err(DomainError::RouterError(err));
                }
            }
        }

        let databases: Result<Vec<Box<dyn DatabaseService>>, DatabaseError> = self
            .databases
            .iter()
            .map(|srv| srv.to_database_domain(context, cloud_provider, &self.annotations_groups, &self.labels_groups))
            .collect();
        let databases = databases?;

        let jobs: Result<Vec<Box<dyn JobService>>, JobError> = self
            .jobs
            .iter()
            .cloned()
            .map(|srv| {
                srv.to_job_domain(
                    context,
                    cloud_provider,
                    container_registry,
                    cluster,
                    &self.annotations_groups,
                    &self.labels_groups,
                )
            })
            .collect();
        let jobs = jobs?;

        let helm_charts: Result<Vec<Box<dyn HelmChartService>>, HelmChartError> = self
            .helms
            .iter()
            .cloned()
            .map(|helm_chart| helm_chart.to_helm_chart_domain(context, cloud_provider))
            .collect();
        let helm_charts = helm_charts?;

        let terraform_services: Result<Vec<Box<dyn TerraformServiceTrait>>, TerraformServiceError> = self
            .terraform_services
            .iter()
            .cloned()
            .map(|terraform_service| {
                terraform_service.to_terraform_service_domain(
                    context,
                    cloud_provider,
                    container_registry,
                    cluster,
                    &self.kube_name,
                    self.long_id,
                    &self.annotations_groups,
                    &self.labels_groups,
                )
            })
            .collect();
        let terraform_services = terraform_services?;

        let agentic_workflows: Result<Vec<Box<dyn AgenticWorkflowService>>, AgenticWorkflowError> = self
            .agentic_workflows
            .iter()
            .cloned()
            .map(|agentic_workflow| agentic_workflow.to_agentic_workflow_domain(context, container_registry, cluster))
            .collect();
        let agentic_workflows = agentic_workflows?;

        Ok(Environment::new(
            self.long_id,
            self.name.clone(),
            self.kube_name.clone(),
            self.project_long_id,
            self.organization_long_id,
            self.action.into(),
            context,
            self.max_parallel_build,
            self.max_parallel_deploy,
            applications,
            containers,
            routers,
            databases,
            jobs,
            helm_charts,
            terraform_services,
            agentic_workflows,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_basic_auth_secret;
    use crate::environment::models::router::RouterError;
    use crate::io_models::environment::DomainError;
    use crate::io_models::variable_utils::VariableInfo;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use std::collections::BTreeMap;

    fn var(value: String) -> VariableInfo {
        VariableInfo { value, is_secret: true }
    }

    #[test]
    fn resolve_basic_auth_secret_prefers_gateway_api_env_var() {
        let mut env_vars = BTreeMap::new();
        env_vars.insert("GW_BASIC_AUTH".to_string(), var(STANDARD.encode("gw-user:gw-pass")));
        env_vars.insert("ING_BASIC_AUTH".to_string(), var(STANDARD.encode("ing-user:ing-pass")));

        let secret = resolve_basic_auth_secret("GW_BASIC_AUTH", "ING_BASIC_AUTH", &env_vars)
            .expect("gateway var should be resolved")
            .expect("secret should be present");

        let decoded = STANDARD
            .decode(secret)
            .expect("resolved secret should stay base64-encoded for k8s secret data");
        assert_eq!(
            String::from_utf8(decoded).expect("decoded secret should be valid utf-8"),
            "gw-user:gw-pass"
        );
    }

    #[test]
    fn resolve_basic_auth_secret_falls_back_to_ingress_env_var() {
        let mut env_vars = BTreeMap::new();
        env_vars.insert("ING_BASIC_AUTH".to_string(), var(STANDARD.encode("ing-user:ing-pass")));

        let secret = resolve_basic_auth_secret("GW_BASIC_AUTH", "ING_BASIC_AUTH", &env_vars)
            .expect("ingress fallback should be resolved")
            .expect("secret should be present");

        let decoded = STANDARD
            .decode(secret)
            .expect("resolved secret should stay base64-encoded for k8s secret data");
        assert_eq!(
            String::from_utf8(decoded).expect("decoded secret should be valid utf-8"),
            "ing-user:ing-pass"
        );
    }

    #[test]
    fn resolve_basic_auth_secret_returns_not_found_when_referenced_env_var_is_missing() {
        let err = resolve_basic_auth_secret("GW_BASIC_AUTH", "", &BTreeMap::new()).expect_err("should fail");

        match err {
            DomainError::RouterError(RouterError::BasicAuthEnvVarNotFound { env_var_name }) => {
                assert_eq!(env_var_name, "GW_BASIC_AUTH");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn resolve_basic_auth_secret_returns_decode_error_for_invalid_base64() {
        let mut env_vars = BTreeMap::new();
        env_vars.insert("GW_BASIC_AUTH".to_string(), var("not-base64".to_string()));

        let err = resolve_basic_auth_secret("GW_BASIC_AUTH", "", &env_vars).expect_err("should fail");

        match err {
            DomainError::RouterError(RouterError::BasicAuthEnvVarBase64DecodeError {
                env_var_name,
                env_var_value,
            }) => {
                assert_eq!(env_var_name, "GW_BASIC_AUTH");
                assert_eq!(env_var_value, "not-base64");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }
}
