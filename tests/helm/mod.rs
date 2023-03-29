use crate::helpers::common::{Cluster, ClusterDomain};
use crate::helpers::dns::dns_provider_qoverydns;
use crate::helpers::utilities::{context_for_cluster, logger, FuncTestsSecrets};
use chrono::Utc;
use qovery_engine::build_platform::{Build, GitRepository, Image, SshKey};
use qovery_engine::cloud_provider::aws::{
    kubernetes::eks::EKS,
    regions::{AwsRegion, AwsZones},
    AWS,
};
use qovery_engine::cloud_provider::environment::Environment;
use qovery_engine::cloud_provider::io::ClusterAdvancedSettings;
use qovery_engine::cloud_provider::kubernetes::{Kind::Eks, Kubernetes, KubernetesVersion};
use qovery_engine::cloud_provider::models::{
    CpuArchitecture, CustomDomain, EnvironmentVariable, MountedFile, Route, Storage,
};
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::cloud_provider::service::{Action, Service};
use qovery_engine::cloud_provider::DeploymentTarget;
use qovery_engine::engine::InfrastructureContext;
use qovery_engine::events::{EnvironmentStep, EventDetails, Stage};
use qovery_engine::io_models::application::{AdvancedSettingsProbeType, ApplicationAdvancedSettings, Port, Protocol};
use qovery_engine::io_models::container::{ContainerAdvancedSettings, Registry};
use qovery_engine::io_models::database::{DatabaseMode, DatabaseOptions};
use qovery_engine::io_models::job::{JobAdvancedSettings, JobSchedule};
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::models::application::Application;
use qovery_engine::models::aws::{AwsAppExtraSettings, AwsRouterExtraSettings, AwsStorageType};
use qovery_engine::models::container::Container;
use qovery_engine::models::database::{Container as ContainerDB, Database, Managed, PostgresSQL};
use qovery_engine::models::job::{ImageSource, Job, RegistryImageSource};
use qovery_engine::models::router::{Router, RouterAdvancedSettings};
use qovery_engine::models::types::{VersionsNumber, AWS as AWSType};
use qovery_engine::utilities::to_short_id;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::string::ToString;
use std::sync::Arc;
use std::time::Duration;
use std::{env, fs};
use tera::Context as TeraContext;
use url::Url;
use uuid::Uuid;

mod cert_manager;
mod chart_testing;

fn lib_dir() -> String {
    env::var("LIB_ROOT_DIR").expect("Missing environment variable LIB_ROOT_DIR")
}

fn kubeconfig_path() -> PathBuf {
    let mut kube_config = dirs::home_dir().unwrap();
    kube_config.push(".kube/config");
    kube_config
}

pub fn chart_path(temp_dir: &str, service_type: &str, chart_id: &Uuid, chart_name: &str) -> String {
    format!("{temp_dir}/{service_type}/{chart_id}/{chart_name}")
}

pub struct TestInfo {
    context: TeraContext,
    event_details: EventDetails,
    temp_dir: String,
    service_folder_type: String,
    service_id: Uuid,
}

fn test_kubernetes() -> Box<dyn Kubernetes> {
    let cluster_id = Uuid::new_v4();
    let context = context_for_cluster(Uuid::new_v4(), cluster_id);
    Box::new(
        EKS::new(
            context.clone(),
            &to_short_id(&cluster_id),
            cluster_id,
            "my_cluster_name",
            KubernetesVersion::V1_23 {
                prefix: None,
                patch: None,
                suffix: None,
            },
            AwsRegion::UsEast2,
            vec![
                AwsZones::UsEast2A.to_string(),
                AwsZones::UsEast2B.to_string(),
                AwsZones::UsEast2C.to_string(),
            ],
            Arc::new(AWS::cloud_provider(&context, Eks, "us-east-2")),
            Arc::new(dns_provider_qoverydns(
                &context,
                &ClusterDomain::Default {
                    cluster_id: cluster_id.to_string(),
                },
            )),
            AWS::kubernetes_cluster_options(FuncTestsSecrets::default(), None, EngineLocation::ClientSide),
            AWS::kubernetes_nodes(3, 5, CpuArchitecture::AMD64),
            logger(),
            ClusterAdvancedSettings {
                load_balancer_size: "my_load_balancer_size".to_string(),
                registry_image_retention_time_sec: 1,
                pleco_resources_ttl: 2,
                loki_log_retention_in_week: 3,
                aws_iam_user_mapper_group_name: "my_aws_iam_user_mapper_group_name".to_string(),
                aws_vpc_enable_flow_logs: true,
                cloud_provider_container_registry_tags: HashMap::new(),
                aws_vpc_flow_logs_retention_days: 1,
                aws_cloudwatch_eks_logs_retention_days: 1,
                ..Default::default()
            },
        )
        .unwrap(),
    )
}

fn create_fake_kubeconfig(kube: &dyn Kubernetes, test_env: &Environment) {
    let temp_dir = kube
        .get_temp_dir(test_env.event_details().clone())
        .expect("Unable to get temp dir");
    let short_id = to_short_id(kube.long_id());
    let kubeconfig_dir_path = format!("{temp_dir}/qovery-kubeconfigs-{short_id}");
    fs::create_dir(&kubeconfig_dir_path)
        .unwrap_or_else(|e| panic!("Unable to create directory {}: {}", &kubeconfig_dir_path, e));
    let local_kubeconfig = format!("{temp_dir}/qovery-kubeconfigs-{short_id}/{short_id}.yaml");
    let _ = fs::copy(kubeconfig_path(), &local_kubeconfig)
        .unwrap_or_else(|e| panic!("Unable to create file {}: {}", &local_kubeconfig, e));
}

fn test_environment(kube: &dyn Kubernetes) -> Environment {
    let app = test_application(kube);
    let app_id = *app.long_id();
    Environment::new(
        Uuid::new_v4(),
        "my_test_environment".to_string(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Action::Create,
        kube.context(),
        1,
        vec![Box::new(app)],
        vec![Box::new(test_container(kube))],
        vec![Box::new(test_router(kube, app_id))],
        vec![
            Box::new(test_managed_database(kube)),
            Box::new(test_container_database(kube)),
        ],
        vec![Box::new(test_job(kube))],
    )
}

fn test_port() -> Port {
    Port {
        id: "my_port_id".to_string(),
        long_id: Uuid::new_v4(),
        port: 1234,
        is_default: true,
        name: Some("my_port_name".to_string()),
        publicly_accessible: true,
        protocol: Protocol::HTTP,
    }
}

fn test_storage() -> Storage<AwsStorageType> {
    Storage {
        id: "my_storage_id".to_string(),
        long_id: Uuid::new_v4(),
        name: "my_storage_name".to_string(),
        storage_type: AwsStorageType::GP2,
        size_in_gib: 1,
        mount_point: "my_mount_point".to_string(),
        snapshot_retention_in_days: 2,
    }
}

fn test_env_var() -> EnvironmentVariable {
    EnvironmentVariable {
        key: "my_env_var_key".to_string(),
        value: "my_env_var_value".to_string(),
    }
}

fn test_mounted_file() -> MountedFile {
    let mounted_file_identifier = QoveryIdentifier::new_random();
    MountedFile {
        id: mounted_file_identifier.short().to_string(),
        long_id: mounted_file_identifier.to_uuid(),
        mount_path: "/etc/mounted_file.json".to_string(),
        file_content_b64: base64::encode(r#"{"mounted_file_key": "hello"}"#),
    }
}

fn test_cmd_arg() -> String {
    "my_command_arg".to_string()
}

fn test_custom_domain() -> CustomDomain {
    CustomDomain {
        domain: "my_custom_domain".to_string(),
        target_domain: "my_target_domain".to_string(),
    }
}

fn test_route(uuid: Uuid) -> Route {
    Route {
        path: "my_route_path".to_string(),
        service_long_id: uuid,
    }
}

#[allow(deprecated)]
pub fn test_application(test_kube: &dyn Kubernetes) -> Application<AWSType> {
    let long_id = Uuid::new_v4();
    Application::new(
        test_kube.context(),
        long_id,
        Action::Create,
        "my_application_name",
        vec![test_port()],
        "1".to_string(),
        "2".to_string(),
        3,
        4,
        5,
        Build {
            git_repository: GitRepository {
                url: Url::parse("https://my_git_url.com").unwrap(),
                get_credentials: None,
                ssh_keys: vec![SshKey {
                    private_key: "my_private_ssh_key".to_string(),
                    passphrase: Some("my_ssh_passphrase".to_string()),
                    public_key: Some("my_public_ssh_key".to_string()),
                }],
                commit_id: "my_commit_id".to_string(),
                dockerfile_path: Some(PathBuf::from("my_dockerfile_path")),
                root_path: PathBuf::from("my_root_path"),
                buildpack_language: Some("my_language".to_string()),
            },
            image: Image {
                application_id: "my_application_id".to_string(),
                application_long_id: long_id,
                application_name: "my_application_name".to_string(),
                name: "my_image_name".to_string(),
                tag: "my_image_tag".to_string(),
                commit_id: "my_image_commit".to_string(),
                registry_name: "my_image_registry_name".to_string(),
                registry_docker_json_config: Some("my_image_registry_docker_json_config".to_string()),
                registry_url: Url::parse("https://my_image_registry_url.com").unwrap(),
                repository_name: "my_image_repository_name".to_string(),
            },
            environment_variables: BTreeMap::new(),
            disable_cache: false,
            timeout: Duration::from_secs(42),
            architectures: test_kube.cpu_architectures(),
        },
        vec![],
        None,
        vec![test_storage()],
        vec![test_env_var()],
        btreeset![test_mounted_file()],
        ApplicationAdvancedSettings {
            security_service_account_name: "".to_string(),
            deployment_delay_start_time_sec: 0,
            deployment_termination_grace_period_seconds: 60,
            deployment_custom_domain_check_enabled: true,
            build_timeout_max_sec: 2,
            network_ingress_proxy_body_size_mb: 3,
            network_ingress_cors_enable: true,
            network_ingress_sticky_session_enable: false,
            network_ingress_cors_allow_origin: "my_network_ingress_cors_allow_origin".to_string(),
            network_ingress_cors_allow_methods: "my_network_ingress_cors_allow_methods".to_string(),
            network_ingress_cors_allow_headers: "my_network_ingress_cors_allow_headers".to_string(),
            network_ingress_keepalive_time_seconds: 4,
            network_ingress_keepalive_timeout_seconds: 5,
            network_ingress_send_timeout_seconds: 6,
            network_ingress_proxy_connect_timeout_seconds: 7,
            network_ingress_proxy_send_timeout_seconds: 8,
            network_ingress_proxy_read_timeout_seconds: 9,
            network_ingress_proxy_buffer_size_kb: 10,
            network_ingress_whitelist_source_range: "my_network_ingress_whitelist_source_range".to_string(),
            network_ingress_denylist_source_range: "".to_string(),
            network_ingress_basic_auth_env_var: "".to_string(),
            readiness_probe_type: AdvancedSettingsProbeType::Tcp,
            readiness_probe_http_get_path: "my_useless_readiness_probe_http_get_path".to_string(),
            readiness_probe_initial_delay_seconds: 11,
            readiness_probe_period_seconds: 12,
            readiness_probe_timeout_seconds: 13,
            readiness_probe_success_threshold: 14,
            readiness_probe_failure_threshold: 15,
            liveness_probe_type: AdvancedSettingsProbeType::Http,
            liveness_probe_http_get_path: "my_liveness_probe_http_get_path".to_string(),
            liveness_probe_initial_delay_seconds: 21,
            liveness_probe_period_seconds: 22,
            liveness_probe_timeout_seconds: 23,
            liveness_probe_success_threshold: 24,
            liveness_probe_failure_threshold: 25,
            hpa_cpu_average_utilization_percent: 31,
        },
        AwsAppExtraSettings {},
        |transmitter| test_kube.context().get_event_details(transmitter),
    )
    .unwrap()
}

pub fn test_container(test_kube: &dyn Kubernetes) -> Container<AWSType> {
    Container::new(
        test_kube.context(),
        Uuid::new_v4(),
        "my_container_name".to_string(),
        Action::Create,
        Registry::DockerHub {
            long_id: Default::default(),
            url: Url::parse("https://my_registry_url.com").unwrap(),
            credentials: None,
        },
        "my_image".to_string(),
        "my_tag".to_string(),
        vec![test_cmd_arg()],
        Some("my_entrypoint".to_string()),
        1,
        2,
        3,
        4,
        5,
        6,
        vec![test_port()],
        vec![test_storage()],
        vec![test_env_var()],
        btreeset![test_mounted_file()],
        ContainerAdvancedSettings {
            deployment_custom_domain_check_enabled: true,
            deployment_termination_grace_period_seconds: 60,
            network_ingress_proxy_body_size_mb: 11,
            network_ingress_cors_enable: true,
            network_ingress_sticky_session_enable: false,
            network_ingress_cors_allow_origin: "my_network_ingress_cors_allow_origin".to_string(),
            network_ingress_cors_allow_methods: "my_network_ingress_cors_allow_methods".to_string(),
            network_ingress_cors_allow_headers: "my_network_ingress_cors_allow_headers".to_string(),
            network_ingress_keepalive_time_seconds: 12,
            network_ingress_keepalive_timeout_seconds: 13,
            network_ingress_send_timeout_seconds: 14,
            network_ingress_proxy_connect_timeout_seconds: 15,
            network_ingress_proxy_send_timeout_seconds: 16,
            network_ingress_proxy_read_timeout_seconds: 17,
            network_ingress_proxy_buffer_size_kb: 18,
            network_ingress_whitelist_source_range: "my_network_ingress_whitelist_source_range".to_string(),
            network_ingress_denylist_source_range: "".to_string(),
            network_ingress_basic_auth_env_var: "".to_string(),
            readiness_probe_type: AdvancedSettingsProbeType::Http,
            readiness_probe_http_get_path: "my_readiness_probe_http_get_path".to_string(),
            readiness_probe_initial_delay_seconds: 21,
            readiness_probe_period_seconds: 22,
            readiness_probe_timeout_seconds: 23,
            readiness_probe_success_threshold: 24,
            readiness_probe_failure_threshold: 25,
            liveness_probe_type: AdvancedSettingsProbeType::Tcp,
            liveness_probe_http_get_path: "my_useless_liveness_probe_http_get_path".to_string(),
            liveness_probe_initial_delay_seconds: 31,
            liveness_probe_period_seconds: 32,
            liveness_probe_timeout_seconds: 33,
            liveness_probe_success_threshold: 34,
            liveness_probe_failure_threshold: 35,
            hpa_cpu_average_utilization_percent: 41,
            security_service_account_name: "".to_string(),
        },
        AwsAppExtraSettings {},
        |transmitter| test_kube.context().get_event_details(transmitter),
    )
    .unwrap()
}

pub fn test_managed_database(test_kube: &dyn Kubernetes) -> Database<AWSType, Managed, PostgresSQL> {
    Database::new(
        test_kube.context(),
        Uuid::new_v4(),
        Action::Create,
        "my_managed_db_name",
        VersionsNumber::new("13".to_string(), None, None, None),
        Utc::now(),
        "my_managed_db_fqdn",
        "my_managed_db_fqdn_id",
        "my_managed_db_total_cpus".to_string(),
        1,
        42,
        "my_managed_db_total_instance_type",
        true,
        2,
        DatabaseOptions {
            login: "my_managed_db_login".to_string(),
            password: "my_managed_db_password".to_string(),
            host: "my_managed_db_host".to_string(),
            port: 11,
            mode: DatabaseMode::MANAGED,
            disk_size_in_gib: 12,
            database_disk_type: "my_managed_db_disk_type".to_string(),
            encrypt_disk: true,
            activate_high_availability: true,
            activate_backups: true,
            publicly_accessible: true,
        },
        |transmitter| test_kube.context().get_event_details(transmitter),
    )
    .unwrap()
}

pub fn test_container_database(test_kube: &dyn Kubernetes) -> Database<AWSType, ContainerDB, PostgresSQL> {
    Database::new(
        test_kube.context(),
        Uuid::new_v4(),
        Action::Create,
        "my_container_db_name",
        VersionsNumber::new("13".to_string(), None, None, None),
        Utc::now(),
        "my_container_db_fqdn",
        "my_container_db_fqdn_id",
        "my_container_db_total_cpus".to_string(),
        1,
        42,
        "my_container_db_instance_type",
        false,
        1234,
        DatabaseOptions {
            login: "my_container_db_login".to_string(),
            password: "my_container_db_password".to_string(),
            host: "my_container_db_host".to_string(),
            port: 11,
            mode: DatabaseMode::MANAGED,
            disk_size_in_gib: 12,
            database_disk_type: "my_container_db_disk_type".to_string(),
            encrypt_disk: true,
            activate_high_availability: true,
            activate_backups: true,
            publicly_accessible: true,
        },
        |transmitter| test_kube.context().get_event_details(transmitter),
    )
    .unwrap()
}

pub fn test_router(test_kube: &dyn Kubernetes, app_id: Uuid) -> Router<AWSType> {
    let long_id = Uuid::new_v4();
    Router::new(
        test_kube.context(),
        long_id,
        "my_router_name",
        Action::Create,
        "my_default_domain",
        vec![test_custom_domain()],
        vec![test_route(app_id)],
        AwsRouterExtraSettings {},
        RouterAdvancedSettings {
            custom_domain_check_enabled: true,
            whitelist_source_range: None,
            denylist_source_range: None,
            basic_auth: None,
        },
        |transmitter| test_kube.context().get_event_details(transmitter),
    )
    .unwrap()
}

fn test_job(test_kube: &dyn Kubernetes) -> Job<AWSType> {
    Job::new(
        test_kube.context(),
        Uuid::new_v4(),
        "my_job_name".to_string(),
        Action::Create,
        ImageSource::Registry {
            source: Box::new(RegistryImageSource {
                registry: Registry::DockerHub {
                    long_id: Default::default(),
                    url: Url::parse("https://my_registry_url.com").unwrap(),
                    credentials: None,
                },
                image: "my_image".to_string(),
                tag: "my_tag".to_string(),
            }),
        },
        JobSchedule::Cron {
            schedule: "my_schedule".to_string(),
        },
        1,
        Duration::from_secs(2),
        Some(3),
        vec![test_cmd_arg()],
        None,
        false,
        4,
        5,
        6,
        7,
        vec![test_env_var()],
        btreeset![test_mounted_file()],
        JobAdvancedSettings {
            job_delete_ttl_seconds_after_finished: Some(8),
            deployment_termination_grace_period_seconds: 60,
            cronjob_concurrency_policy: "my_cronjob_concurrency_policy".to_string(),
            cronjob_failed_jobs_history_limit: 9,
            cronjob_success_jobs_history_limit: 10,
            build_timeout_max_sec: 30 * 60,
            readiness_probe_type: AdvancedSettingsProbeType::Tcp,
            readiness_probe_http_get_path: "my_useless_readiness_probe_http_get_path".to_string(),
            readiness_probe_initial_delay_seconds: 11,
            readiness_probe_period_seconds: 12,
            readiness_probe_timeout_seconds: 13,
            readiness_probe_success_threshold: 14,
            readiness_probe_failure_threshold: 15,
            liveness_probe_type: AdvancedSettingsProbeType::Http,
            liveness_probe_http_get_path: "my_liveness_probe_http_get_path".to_string(),
            liveness_probe_initial_delay_seconds: 21,
            liveness_probe_period_seconds: 22,
            liveness_probe_timeout_seconds: 23,
            liveness_probe_success_threshold: 24,
            liveness_probe_failure_threshold: 25,
            security_service_account_name: "".to_string(),
        },
        AwsAppExtraSettings {},
        |transmitter| test_kube.context().get_event_details(transmitter),
    )
    .unwrap()
}

fn infra_ctx(test_kube: &dyn Kubernetes) -> InfrastructureContext {
    AWS::docker_cr_engine(
        test_kube.context(),
        logger(),
        test_kube.region(),
        test_kube.kind(),
        test_kube.version(),
        &ClusterDomain::Default {
            cluster_id: test_kube.long_id().to_string(),
        },
        None,
        3,
        5,
        CpuArchitecture::AMD64,
        EngineLocation::QoverySide,
    )
}

fn deployment_target<'a>(
    test_kube: &'a dyn Kubernetes,
    test_env: &'a Environment,
    infra_ctx: &'a InfrastructureContext,
) -> DeploymentTarget<'a> {
    create_fake_kubeconfig(test_kube, test_env);
    DeploymentTarget::new(infra_ctx, test_env, &|| false)
        .unwrap_or_else(|e| panic!("Unable to create deployment target: {e}"))
}

pub fn application_context() -> TestInfo {
    let test_kube = test_kubernetes();
    let test_env = test_environment(test_kube.as_ref());
    let infra_ctx = infra_ctx(test_kube.as_ref());
    let target = deployment_target(test_kube.as_ref(), &test_env, &infra_ctx);
    let temp_dir = format!(
        "{}/.qovery-workspace/{}",
        test_kube.context().workspace_root_dir(),
        test_kube.context().execution_id()
    );

    TestInfo {
        context: test_env.applications[0]
            .to_tera_context(&target)
            .expect("Unable to get application context"),
        event_details: test_kube.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration)),
        temp_dir,
        service_folder_type: "applications".to_string(),
        service_id: *test_env.applications[0].long_id(),
    }
}

pub fn container_context() -> TestInfo {
    let test_kube = test_kubernetes();
    let test_env = test_environment(test_kube.as_ref());
    let infra_ctx = infra_ctx(test_kube.as_ref());
    let target = deployment_target(test_kube.as_ref(), &test_env, &infra_ctx);
    let temp_dir = format!(
        "{}/.qovery-workspace/{}",
        test_kube.context().workspace_root_dir(),
        test_kube.context().execution_id()
    );

    TestInfo {
        context: test_env.containers[0]
            .to_tera_context(&target)
            .expect("Unable to get application context"),
        event_details: test_kube.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration)),
        temp_dir,
        service_folder_type: "containers".to_string(),
        service_id: *test_env.containers[0].long_id(),
    }
}

pub fn managed_database_context() -> TestInfo {
    let test_kube = test_kubernetes();
    let test_env = test_environment(test_kube.as_ref());
    let infra_ctx = infra_ctx(test_kube.as_ref());
    let target = deployment_target(test_kube.as_ref(), &test_env, &infra_ctx);
    let temp_dir = format!(
        "{}/.qovery-workspace/{}",
        test_kube.context().workspace_root_dir(),
        test_kube.context().execution_id()
    );

    TestInfo {
        context: test_env.databases[0]
            .to_tera_context(&target)
            .expect("Unable to get application context"),
        event_details: test_kube.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration)),
        temp_dir,
        service_folder_type: "databases".to_string(),
        service_id: *test_env.databases[0].long_id(),
    }
}

pub fn container_database_context() -> TestInfo {
    let test_kube = test_kubernetes();
    let test_env = test_environment(test_kube.as_ref());
    let infra_ctx = infra_ctx(test_kube.as_ref());
    let target = deployment_target(test_kube.as_ref(), &test_env, &infra_ctx);
    let temp_dir = format!(
        "{}/.qovery-workspace/{}",
        test_kube.context().workspace_root_dir(),
        test_kube.context().execution_id()
    );

    TestInfo {
        context: test_env.databases[1]
            .to_tera_context(&target)
            .expect("Unable to get application context"),
        event_details: test_kube.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration)),
        temp_dir,
        service_folder_type: "databases".to_string(),
        service_id: *test_env.databases[1].long_id(),
    }
}

pub fn router_context() -> TestInfo {
    let test_kube = test_kubernetes();
    let test_env = test_environment(test_kube.as_ref());
    let infra_ctx = infra_ctx(test_kube.as_ref());
    let target = deployment_target(test_kube.as_ref(), &test_env, &infra_ctx);
    let temp_dir = format!(
        "{}/.qovery-workspace/{}",
        test_kube.context().workspace_root_dir(),
        test_kube.context().execution_id()
    );

    TestInfo {
        context: test_env.routers[0]
            .to_tera_context(&target)
            .expect("Unable to get application context"),
        event_details: test_kube.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration)),
        temp_dir,
        service_folder_type: "routers".to_string(),
        service_id: *test_env.routers[0].long_id(),
    }
}

pub fn job_context() -> TestInfo {
    let test_kube = test_kubernetes();
    let test_env = test_environment(test_kube.as_ref());
    let infra_ctx = infra_ctx(test_kube.as_ref());
    let target = deployment_target(test_kube.as_ref(), &test_env, &infra_ctx);
    let temp_dir = format!(
        "{}/.qovery-workspace/{}",
        test_kube.context().workspace_root_dir(),
        test_kube.context().execution_id()
    );

    TestInfo {
        context: test_env.jobs[0]
            .to_tera_context(&target)
            .expect("Unable to get application context"),
        event_details: test_kube.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration)),
        temp_dir,
        service_folder_type: "jobs".to_string(),
        service_id: *test_env.jobs[0].long_id(),
    }
}
