use crate::helpers::common::Infrastructure;
use crate::helpers::utilities::{engine_run_test, get_pods};
use crate::services::utilities::{CloudProvider, TestInfra};
use function_name::named;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use qovery_engine::io_models::Action;
use qovery_engine::io_models::application::{PortIo, Protocol};
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::helm_chart::{HelmChart, HelmChartSource, HelmRawValues, HelmValueSource};
use qovery_engine::io_models::router::{Route, Router};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::runtime::block_on;
use qovery_engine::utilities::to_short_id;
use std::path::{Path, PathBuf};
use tracing::{Level, span};
use url::Url;
use uuid::Uuid;

// Builder for creating HelmChart test instances
struct HelmChartBuilder {
    service_id: Uuid,
    kube_name: String,
    name: String,
    commit_id: String,
    root_path: String,
    allow_cluster_wide_resources: bool,
    set_values: Vec<(String, String)>,
    set_string_values: Vec<(String, String)>,
    set_json_values: Vec<(String, String)>,
    command_args: Vec<String>,
    ports: Vec<PortIo>,
}

impl HelmChartBuilder {
    fn new(service_id: Uuid, kube_name: &str) -> Self {
        Self {
            service_id,
            kube_name: kube_name.to_string(),
            name: "my little chart ****".to_string(),
            commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
            root_path: "/simple_app".to_string(),
            allow_cluster_wide_resources: false,
            set_values: vec![("toto".to_string(), "tata".to_string())],
            set_string_values: vec![("my-string".to_string(), "1".to_string())],
            set_json_values: vec![("my-json".to_string(), "{\"json\": \"value\"}".to_string())],
            command_args: vec!["--install".to_string()],
            ports: vec![],
        }
    }

    fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    fn with_commit_id(mut self, commit_id: &str) -> Self {
        self.commit_id = commit_id.to_string();
        self
    }

    fn with_root_path(mut self, root_path: &str) -> Self {
        self.root_path = root_path.to_string();
        self
    }

    fn with_cluster_wide_resources(mut self, allow: bool) -> Self {
        self.allow_cluster_wide_resources = allow;
        self
    }

    fn with_set_values(mut self, values: Vec<(String, String)>) -> Self {
        self.set_values = values;
        self
    }

    fn with_set_string_values(mut self, values: Vec<(String, String)>) -> Self {
        self.set_string_values = values;
        self
    }

    fn with_command_args(mut self, args: Vec<String>) -> Self {
        self.command_args = args;
        self
    }

    fn with_ports(mut self, ports: Vec<PortIo>) -> Self {
        self.ports = ports;
        self
    }

    fn build(&self) -> HelmChart {
        let mut set_values = self.set_values.clone();
        set_values.push(("serviceId".to_string(), self.service_id.to_string()));

        let mut set_string_values = self.set_string_values.clone();
        if !set_string_values.iter().any(|(k, _)| k == "serviceId") {
            set_string_values.push(("serviceId".to_string(), self.service_id.to_string()));
        }

        HelmChart {
            long_id: self.service_id,
            name: self.name.clone(),
            kube_name: self.kube_name.clone(),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: self.commit_id.clone(),
                root_path: PathBuf::from(&self.root_path),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "toto.yaml".to_string(),
                    content: "nameOverride: tata".to_string(),
                }],
            },
            set_values,
            set_string_values,
            set_json_values: self.set_json_values.clone(),
            command_args: self.command_args.clone(),
            timeout_sec: 60,
            allow_cluster_wide_resources: self.allow_cluster_wide_resources,
            environment_vars_with_infos: btreemap! {
                "TOTO".to_string() => VariableInfo {
                    value: "Salut".to_string(),
                    is_secret: false
                }
            },
            advanced_settings: Default::default(),
            ports: self.ports.clone(),
        }
    }
}

// Test helper functions

fn run_simple_lifecycle_test(cloud_provider: CloudProvider, service_id: Uuid) {
    let infra = TestInfra::new(cloud_provider);
    let mut environment = infra.create_environment();
    let helm_chart = HelmChartBuilder::new(service_id, "my-little-chart").build();
    environment.helms = vec![helm_chart];

    let mut environment_for_delete = environment.clone();
    environment_for_delete.action = Action::Delete;

    assert!(environment.deploy_environment(&environment, &infra.infra_ctx).is_ok());
    assert!(
        environment_for_delete
            .delete_environment(&environment_for_delete, &infra.infra_ctx_for_delete)
            .is_ok()
    );

    infra.cleanup(environment);
}

fn verify_config_map_values(
    api: &Api<ConfigMap>,
    config_map_name: &str,
    service_id: &Uuid,
    environment: &EnvironmentRequest,
    expected_version: &str,
) {
    let config_map: ConfigMap = block_on(api.get(config_map_name)).unwrap();
    let data = config_map.data.unwrap();
    assert_eq!(data.len(), 5);
    assert_eq!(data.get("project-id").unwrap(), &environment.project_long_id.to_string());
    assert_eq!(data.get("environment-id").unwrap(), &environment.long_id.to_string());
    assert_eq!(data.get("service-id").unwrap(), &service_id.to_string());
    assert_eq!(data.get("service-version").unwrap(), expected_version);
}

fn run_admission_controller_test(cloud_provider: CloudProvider) {
    let infra = TestInfra::new(cloud_provider);
    let service_id = Uuid::new_v4();
    let mut environment = infra.create_environment();

    let helm_chart = HelmChartBuilder::new(service_id, "my-little-chart").build();
    environment.helms = vec![helm_chart];
    let mut environment_for_delete = environment.clone();
    environment_for_delete.action = Action::Delete;

    // First deployment
    assert!(environment.deploy_environment(&environment, &infra.infra_ctx).is_ok());

    let kube_client = infra
        .infra_ctx
        .mk_kube_client()
        .expect("kube client is not set")
        .client();
    let api_config_map: Api<ConfigMap> = Api::namespaced(kube_client, &environment.kube_name);
    let short_id = to_short_id(&service_id);
    let config_map_name = format!("{short_id}-admission-controller-config-map");

    verify_config_map_values(
        &api_config_map,
        &config_map_name,
        &service_id,
        &environment,
        "18679eb4acf787470d4e3bdd4aa369c7dcea90a0",
    );

    // Second deployment with different version
    let updated_chart = HelmChartBuilder::new(service_id, "my-little-chart")
        .with_commit_id("b93c8d1b9c0bea63f7ce6a669c758cd6b9c9ece2")
        .build();
    environment.helms = vec![updated_chart];

    // Delete helm chart dir
    let chart_directory = qovery_engine::fs::workspace_directory(
        infra.context.workspace_root_dir(),
        infra.context.execution_id(),
        format!("helm_charts/{service_id}"),
    )
    .unwrap();
    std::fs::remove_dir_all(Path::new(chart_directory.to_str().unwrap())).unwrap();

    assert!(environment.deploy_environment(&environment, &infra.infra_ctx).is_ok());
    verify_config_map_values(
        &api_config_map,
        &config_map_name,
        &service_id,
        &environment,
        "b93c8d1b9c0bea63f7ce6a669c758cd6b9c9ece2",
    );

    assert!(
        environment_for_delete
            .delete_environment(&environment_for_delete, &infra.infra_ctx_for_delete)
            .is_ok()
    );
    infra.cleanup(environment);
}

fn run_pause_resume_test(cloud_provider: CloudProvider, allow_cluster_wide_resources: bool) {
    let infra = TestInfra::new(cloud_provider);
    let service_id = Uuid::new_v4();
    let mut environment = infra.create_environment();

    println!("service id {service_id}");
    let helm_chart = HelmChartBuilder::new(service_id, "my-little-chart")
        .with_commit_id("214310971046bc28db8c03b068248ed11b68315b")
        .with_cluster_wide_resources(allow_cluster_wide_resources)
        .with_set_values(vec![("toto".to_string(), "tata".to_string())])
        .with_set_string_values(vec![("my-string".to_string(), "1".to_string())])
        .build();
    environment.helms = vec![helm_chart];

    let mut environment_for_delete = environment.clone();
    environment_for_delete.action = Action::Delete;

    // Deploy
    assert!(environment.deploy_environment(&environment, &infra.infra_ctx).is_ok());
    let pods = get_pods(&infra.infra_ctx, cloud_provider.kind(), &environment, &service_id).unwrap();
    assert!(!pods.items.is_empty());

    // Pause
    assert!(
        environment
            .pause_environment(&environment, &infra.infra_ctx_for_delete)
            .is_ok()
    );
    let pods = get_pods(&infra.infra_ctx, cloud_provider.kind(), &environment, &service_id).unwrap();
    assert!(pods.items.is_empty());

    // Resume
    let infra_ctx_resume = infra.create_resume_context();
    assert!(environment.deploy_environment(&environment, &infra_ctx_resume).is_ok());
    let pods = get_pods(&infra.infra_ctx, cloud_provider.kind(), &environment, &service_id).unwrap();
    assert!(!pods.items.is_empty());

    // Cleanup
    assert!(
        environment
            .delete_environment(&environment, &infra.infra_ctx_for_delete)
            .is_ok()
    );
    infra.cleanup(environment);
}

fn run_restart_test(cloud_provider: CloudProvider, allow_cluster_wide_resources: bool) {
    let infra = TestInfra::new(cloud_provider);
    let service_id = Uuid::new_v4();
    let mut environment = infra.create_environment();

    println!("service id {service_id}");
    let helm_chart = HelmChartBuilder::new(service_id, "my-little-chart")
        .with_commit_id("214310971046bc28db8c03b068248ed11b68315b")
        .with_cluster_wide_resources(allow_cluster_wide_resources)
        .with_set_values(vec![("toto".to_string(), "tata".to_string())])
        .with_set_string_values(vec![("my-string".to_string(), "1".to_string())])
        .build();
    environment.helms = vec![helm_chart];

    // Deploy
    assert!(environment.deploy_environment(&environment, &infra.infra_ctx).is_ok());
    let pods = get_pods(&infra.infra_ctx, cloud_provider.kind(), &environment, &service_id).unwrap();
    assert!(!pods.items.is_empty());

    // Restart
    assert!(
        environment
            .restart_environment(&environment, &infra.infra_ctx_for_delete)
            .is_ok()
    );

    infra.cleanup(environment);
}

fn run_router_test(cloud_provider: CloudProvider) {
    let infra = TestInfra::new(cloud_provider);
    let extra_namespace = format!("extra-env-{}", Uuid::new_v4());
    let host_suffix = Uuid::new_v4();
    let service_id = Uuid::new_v4();
    let mut environment = infra.create_environment();

    let ports = vec![
        PortIo {
            long_id: Uuid::new_v4(),
            port: 8080,
            is_default: false,
            name: format!("service1-p8080-{host_suffix}"),
            publicly_accessible: true,
            protocol: Protocol::HTTP,
            namespace: None,
            service_name: Some("inner-namespace-service1".to_string()),
            path: None,
            path_rewrite: None,
        },
        PortIo {
            long_id: Uuid::new_v4(),
            port: 8080,
            is_default: false,
            name: format!("service2-p8080-{host_suffix}"),
            publicly_accessible: true,
            protocol: Protocol::HTTP,
            namespace: Some(extra_namespace.clone()),
            service_name: Some("outside-namespace-service2".to_string()),
            path: None,
            path_rewrite: None,
        },
    ];

    let helm_chart = HelmChartBuilder::new(service_id, "my-special-chart")
        .with_name("my special chart ****")
        .with_commit_id("8acb6e06d98c0c1b8f2285d5c5bc7f1a837a782a")
        .with_root_path("/several_services")
        .with_cluster_wide_resources(true)
        .with_set_values(vec![("service2.namespace".to_string(), extra_namespace)])
        .with_set_string_values(vec![])
        .with_command_args(vec![])
        .with_ports(ports)
        .build();

    environment.helms = vec![helm_chart];
    environment.routers = vec![Router {
        long_id: Uuid::new_v4(),
        name: "default-router".to_string(),
        kube_name: "default-router".to_string(),
        action: Action::Create,
        default_domain: "main".to_string(),
        public_port: 443,
        custom_domains: vec![],
        routes: vec![Route {
            path: "/".to_string(),
            service_long_id: environment.helms[0].long_id,
        }],
    }];

    let mut environment_for_delete = environment.clone();
    environment_for_delete.action = Action::Delete;

    assert!(environment.deploy_environment(&environment, &infra.infra_ctx).is_ok());
    assert!(
        environment_for_delete
            .delete_environment(&environment_for_delete, &infra.infra_ctx_for_delete)
            .is_ok()
    );

    infra.cleanup(environment);
}

// AWS Tests

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_helm_chart() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();
        run_simple_lifecycle_test(CloudProvider::Aws, Uuid::new_v4());
        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_helm_chart_twice_to_check_admission_controller_config_map_is_well_created_and_updated() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();
        run_admission_controller_test(CloudProvider::Aws);
        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_helm_chart_and_pause_it() {
    let test = |allow_cluster_wide_resources| {
        engine_run_test(|| {
            let span = span!(Level::INFO, "test", name = function_name!());
            let _enter = span.enter();
            run_pause_resume_test(CloudProvider::Aws, allow_cluster_wide_resources);
            "".to_string()
        })
    };
    test(true);
    test(false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_helm_chart_and_restart_it() {
    let test = |allow_cluster_wide_resources| {
        engine_run_test(|| {
            let span = span!(Level::INFO, "test", name = function_name!());
            let _enter = span.enter();
            run_restart_test(CloudProvider::Aws, allow_cluster_wide_resources);
            "".to_string()
        })
    };
    test(true);
    test(false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn deploy_helm_chart_with_router() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = "deploy_helm_chart");
        let _enter = span.enter();
        run_router_test(CloudProvider::Aws);
        "".to_string()
    })
}

// Azure Tests

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_helm_chart() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();
        run_simple_lifecycle_test(CloudProvider::Azure, Uuid::new_v4());
        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_helm_chart_twice_to_check_admission_controller_config_map_is_well_created_and_updated() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();
        run_admission_controller_test(CloudProvider::Azure);
        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_helm_chart_and_pause_it() {
    let test = |allow_cluster_wide_resources| {
        let test_name = function_name!();
        engine_run_test(|| {
            let span = span!(Level::INFO, "test", name = function_name!());
            let _enter = span.enter();
            run_pause_resume_test(CloudProvider::Azure, allow_cluster_wide_resources);
            test_name.to_string()
        })
    };
    test(true);
    test(false);
}

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_helm_chart_and_restart_it() {
    let test = |allow_cluster_wide_resources| {
        let test_name = function_name!();
        engine_run_test(|| {
            let span = span!(Level::INFO, "test", name = function_name!());
            let _enter = span.enter();
            run_restart_test(CloudProvider::Azure, allow_cluster_wide_resources);
            test_name.to_string()
        })
    };
    test(true);
    test(false);
}

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_helm_chart_with_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = "deploy_helm_chart");
        let _enter = span.enter();
        run_router_test(CloudProvider::Azure);
        test_name.to_string()
    })
}
