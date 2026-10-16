use crate::environment::models::agentic_workflow::{AgenticWorkflowError, AgenticWorkflowService};
use crate::environment::models::application::{ApplicationError, ApplicationService};
use crate::environment::models::container::{ContainerError, ContainerService};
use crate::environment::models::database::{DatabaseError, DatabaseService};
use crate::environment::models::environment::Environment;
use crate::environment::models::helm_chart::{HelmChartError, HelmChartService};
use crate::environment::models::job::{JobError, JobService};
use crate::environment::models::router::{RouterAdvancedSettings, RouterError};
use crate::environment::models::router_listener_set::normalize_hostname;
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
    // Requests sent before core knew about routing modes have no field and keep the legacy behavior.
    #[serde(default)]
    pub routing_mode: EnvironmentRoutingMode,
    #[serde(default)]
    pub hostname_mapping: HostnameMapping,
}

/// Routers sharing each custom hostname, serialized as `{ "hostname": ["router UUID", ...] }`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, Eq, PartialEq, Hash)]
#[serde(
    from = "BTreeMap<String, BTreeSet<Uuid>>",
    into = "BTreeMap<String, BTreeSet<Uuid>>"
)]
pub struct HostnameMapping(BTreeMap<String, BTreeSet<Uuid>>);

/// Keys are normalized once here, so a lookup is a plain `get`. Spellings of the same hostname are merged.
impl From<BTreeMap<String, BTreeSet<Uuid>>> for HostnameMapping {
    fn from(entries: BTreeMap<String, BTreeSet<Uuid>>) -> Self {
        let mut mapping: BTreeMap<String, BTreeSet<Uuid>> = BTreeMap::new();
        for (hostname, routers) in entries {
            mapping
                .entry(normalize_hostname(&hostname))
                .or_default()
                .extend(routers);
        }
        Self(mapping)
    }
}

impl From<HostnameMapping> for BTreeMap<String, BTreeSet<Uuid>> {
    fn from(mapping: HostnameMapping) -> Self {
        mapping.0
    }
}

impl HostnameMapping {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn contains_router(&self, router_id: &Uuid) -> bool {
        self.0.values().any(|routers| routers.contains(router_id))
    }

    pub(crate) fn routers_for_hostname(&self, hostname: &str) -> Option<&BTreeSet<Uuid>> {
        self.0.get(&normalize_hostname(hostname))
    }
}

/// How the Gateway API listeners of the environment's routers are grouped.
/// Core sends these exact names, so each variant name is pinned instead of derived.
///
/// `PerHostnameListenerSet` renders one ListenerSet per custom hostname, named after the hostname only.
/// `hostname_mapping` identifies the routers that can share each hostname's ListenerSet.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum EnvironmentRoutingMode {
    #[default]
    #[serde(rename = "PER_ROUTER_LISTENER_SET")]
    PerRouterListenerSet,
    #[serde(rename = "PER_HOSTNAME_LISTENER_SET")]
    PerHostnameListenerSet,
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
            self.routing_mode,
            self.hostname_mapping.clone(),
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
    use super::{EnvironmentRequest, EnvironmentRoutingMode, HostnameMapping, resolve_basic_auth_secret};
    use crate::environment::models::router::RouterError;
    use crate::io_models::environment::DomainError;
    use crate::io_models::variable_utils::VariableInfo;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use std::collections::{BTreeMap, BTreeSet};

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

    fn environment_request_json(routing_mode: Option<&str>) -> serde_json::Value {
        let mut json = serde_json::json!({
            "execution_id": "exec-123",
            "long_id": "11111111-1111-1111-1111-111111111111",
            "name": "production",
            "kube_name": "z99999999-production",
            "project_long_id": "22222222-2222-2222-2222-222222222222",
            "organization_long_id": "33333333-3333-3333-3333-333333333333",
            "action": "CREATE",
            "applications": [],
            "containers": [],
            "jobs": [],
            "routers": [],
            "databases": []
        });
        if let Some(routing_mode) = routing_mode {
            json["routing_mode"] = serde_json::Value::String(routing_mode.to_string());
        }
        json
    }

    #[test]
    fn environment_request_without_routing_mode_uses_one_listener_set_per_router() {
        let request: EnvironmentRequest =
            serde_json::from_value(environment_request_json(None)).expect("request without routing_mode should parse");

        assert_eq!(request.routing_mode, EnvironmentRoutingMode::PerRouterListenerSet);
    }

    #[test]
    fn environment_request_reads_the_routing_mode_sent_by_core() {
        let request: EnvironmentRequest =
            serde_json::from_value(environment_request_json(Some("PER_HOSTNAME_LISTENER_SET")))
                .expect("request with routing_mode should parse");

        assert_eq!(request.routing_mode, EnvironmentRoutingMode::PerHostnameListenerSet);
        let serialized = serde_json::to_value(&request).expect("request should serialize");
        assert_eq!(serialized["routing_mode"], "PER_HOSTNAME_LISTENER_SET");
    }

    #[test]
    fn environment_request_preserves_hostname_mapping_sent_by_core() {
        let mut json = environment_request_json(Some("PER_HOSTNAME_LISTENER_SET"));
        let mapping = serde_json::json!({
            "api.example.com": [
                "44444444-4444-4444-4444-444444444444",
                "55555555-5555-5555-5555-555555555555"
            ],
            "www.example.com": ["44444444-4444-4444-4444-444444444444"]
        });
        json["hostname_mapping"] = mapping.clone();

        let request: EnvironmentRequest = serde_json::from_value(json).expect("request should parse");
        let serialized = serde_json::to_value(request).expect("request should serialize");

        assert_eq!(serialized["hostname_mapping"], mapping);
    }

    #[test]
    fn hostname_mapping_normalizes_domains_and_combines_duplicate_entries() {
        let mapping: HostnameMapping = serde_json::from_value(serde_json::json!({
            " API.Example.com. ": ["44444444-4444-4444-4444-444444444444"],
            "api.example.com": ["55555555-5555-5555-5555-555555555555"]
        }))
        .unwrap();

        let routers = mapping.routers_for_hostname("Api.Example.COM.").unwrap();
        assert_eq!(routers.len(), 2);
        assert!(routers.contains(&uuid::uuid!("44444444-4444-4444-4444-444444444444")));
        assert!(routers.contains(&uuid::uuid!("55555555-5555-5555-5555-555555555555")));
        assert!(mapping.routers_for_hostname("p8000.api.example.com").is_none());
        assert!(mapping.routers_for_hostname("other.example.com").is_none());
    }

    #[test]
    fn hostname_mapping_prefers_an_explicit_hostname_over_its_parent() {
        let mapping: HostnameMapping = serde_json::from_value(serde_json::json!({
            "example.com": ["44444444-4444-4444-4444-444444444444"],
            "api.example.com": ["55555555-5555-5555-5555-555555555555"]
        }))
        .unwrap();

        assert_eq!(
            mapping.routers_for_hostname("api.example.com"),
            Some(&BTreeSet::from([uuid::uuid!("55555555-5555-5555-5555-555555555555")]))
        );
    }

    #[test]
    fn environment_request_defaults_missing_hostname_mapping_and_rejects_invalid_router_ids() {
        let json = environment_request_json(None);
        let request: EnvironmentRequest = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(request.hostname_mapping, HostnameMapping::default());

        let mut invalid = json;
        invalid["hostname_mapping"] = serde_json::json!({ "api.example.com": ["not-a-router-uuid"] });
        assert!(serde_json::from_value::<EnvironmentRequest>(invalid).is_err());
    }

    #[test]
    fn environment_routing_mode_wire_names_are_pinned() {
        assert_eq!(
            serde_json::to_string(&EnvironmentRoutingMode::PerRouterListenerSet).unwrap(),
            "\"PER_ROUTER_LISTENER_SET\""
        );
        assert_eq!(
            serde_json::to_string(&EnvironmentRoutingMode::PerHostnameListenerSet).unwrap(),
            "\"PER_HOSTNAME_LISTENER_SET\""
        );
        assert_eq!(
            serde_json::from_value::<EnvironmentRoutingMode>(serde_json::json!("PER_ROUTER_LISTENER_SET")).unwrap(),
            EnvironmentRoutingMode::PerRouterListenerSet
        );
        assert!(serde_json::from_value::<EnvironmentRoutingMode>(serde_json::json!(42)).is_err());
    }

    #[test]
    fn environment_request_with_unknown_routing_mode_is_rejected() {
        let result =
            serde_json::from_value::<EnvironmentRequest>(environment_request_json(Some("PER_SERVICE_LISTENER_SET")));

        assert!(result.is_err());
    }
}
