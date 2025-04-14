use crate::helpers::aws::AWS_KUBERNETES_VERSION;
use crate::helpers::common::{Cluster, ClusterDomain, Infrastructure, NodeManager, compute_test_cluster_endpoint};
use crate::helpers::kubernetes::{KUBERNETES_MAX_NODES, KUBERNETES_MIN_NODES};
use crate::helpers::scaleway::SCW_KUBERNETES_VERSION;
use crate::helpers::utilities::{
    FuncTestsSecrets, context_for_resource, db_disk_type, db_infos, db_instance_type, engine_run_test, generate_id,
    generate_password, get_pvc, get_svc, get_svc_name, init, logger, metrics_registry,
};
use chrono::Utc;
use core::default::Default;
use core::option::Option;
use core::option::Option::{None, Some};
use core::result::Result::{Err, Ok};
use qovery_engine::cmd::structs::SVCItem;
use qovery_engine::environment::models::environment::Environment;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::aws::AWS;
use qovery_engine::infrastructure::models::cloud_provider::scaleway::Scaleway;
use qovery_engine::infrastructure::models::kubernetes::Kind as KubernetesKind;
use qovery_engine::io_models::engine_location::EngineLocation;

use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::io_models::application::{Application, Port, Protocol};
use qovery_engine::io_models::context::{CloneForTest, Context};
use qovery_engine::io_models::database::DatabaseMode::{CONTAINER, MANAGED};
use qovery_engine::io_models::database::{Database, DatabaseKind, DatabaseMode};

use qovery_engine::environment::models::database::DatabaseInstanceType;
use qovery_engine::environment::report::logger::EnvLogger;
use qovery_engine::environment::task::{DeploymentOption, EnvironmentTask};
use qovery_engine::events::EnvironmentStep;
use qovery_engine::infrastructure::models::cloud_provider::service::Service;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::models::CpuArchitecture;
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, QoveryIdentifier};
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;

use crate::helpers::azure::{AZURE_KUBERNETES_VERSION, AZURE_LOCATION};
use crate::helpers::gcp::GCP_KUBERNETES_VERSION;
use crate::helpers::on_premise::ON_PREMISE_KUBERNETES_VERSION;
use base64::Engine;
use base64::engine::general_purpose;
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::environment::models::abort::AbortStatus;
use qovery_engine::environment::models::types::VersionsNumber;
use qovery_engine::errors::EngineError;
use qovery_engine::infrastructure::models::cloud_provider::azure::Azure;
use qovery_engine::infrastructure::models::kubernetes::gcp::Gke;
use qovery_engine::utilities::to_short_id;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{Level, span};
use uuid::Uuid;

impl Infrastructure for EnvironmentRequest {
    fn build_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> (Environment, Result<(), Box<EngineError>>) {
        let mut env = environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        let deployment_option = DeploymentOption {
            force_build: true,
            force_push: true,
        };
        let logger = Arc::new(infra_ctx.kubernetes().logger().clone_dyn());
        let services_to_build: Vec<&mut dyn Service> = env
            .applications
            .iter_mut()
            .map(|app| app.as_service_mut())
            .chain(env.jobs.iter_mut().map(|job| job.as_service_mut()))
            .collect();

        let ret = EnvironmentTask::build_and_push_services(
            environment.long_id,
            environment.project_long_id,
            services_to_build,
            &deployment_option,
            infra_ctx,
            1,
            |srv: &dyn Service| EnvLogger::new(srv, EnvironmentStep::Build, logger.clone()),
            &|| AbortStatus::None,
        );

        (env, ret)
    }

    fn deploy_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>> {
        let mut env = environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        env.action = qovery_engine::infrastructure::models::cloud_provider::service::Action::Create;
        EnvironmentTask::deploy_environment(env, infra_ctx, &|| AbortStatus::None)
    }

    fn pause_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>> {
        let mut env = environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        env.action = qovery_engine::infrastructure::models::cloud_provider::service::Action::Pause;
        EnvironmentTask::deploy_environment(env, infra_ctx, &|| AbortStatus::None)
    }

    fn delete_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>> {
        let mut env = environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        env.action = qovery_engine::infrastructure::models::cloud_provider::service::Action::Delete;
        EnvironmentTask::deploy_environment(env, infra_ctx, &|| AbortStatus::None)
    }

    fn restart_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>> {
        let mut env = environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        env.action = qovery_engine::infrastructure::models::cloud_provider::service::Action::Restart;
        EnvironmentTask::deploy_environment(env, infra_ctx, &|| AbortStatus::None)
    }
}

pub enum StorageSize {
    NormalSize,
    OverSize,
    Resize,
}

impl StorageSize {
    pub fn size(&self) -> u32 {
        match *self {
            StorageSize::NormalSize => 10,
            StorageSize::OverSize => 200000,
            StorageSize::Resize => 20,
        }
    }
}

pub fn environment_3_apps_3_databases(
    context: &Context,
    database_instance_type: Option<Box<dyn DatabaseInstanceType>>,
    database_disk_type: &str,
    provider_kind: Kind,
) -> EnvironmentRequest {
    let app_name_1 = QoveryIdentifier::new_random().short().to_string();
    let app_name_2 = QoveryIdentifier::new_random().short().to_string();
    let app_name_3 = QoveryIdentifier::new_random().short().to_string();

    // mongoDB management part
    let database_host_mongo = get_svc_name(DatabaseKind::Mongodb, provider_kind.clone()).to_string();
    let database_port_mongo = 27017;
    let database_db_name_mongo = "mongodb".to_string();
    let database_username_mongo = "superuser".to_string();
    let database_password_mongo = generate_password(CONTAINER);
    let database_uri_mongo = format!(
        "mongodb://{database_username_mongo}:{database_password_mongo}@{database_host_mongo}:{database_port_mongo}/{database_db_name_mongo}"
    );
    let version_mongo = "4.4";

    // pSQL 1 management part
    let fqdn = get_svc_name(DatabaseKind::Postgresql, provider_kind.clone()).to_string();
    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_password(CONTAINER);
    let database_name = "pg".to_string();

    // pSQL 2 management part
    let fqdn_2 = format!("{}2", get_svc_name(DatabaseKind::Postgresql, provider_kind));
    let database_username_2 = "superuser2".to_string();
    let database_name_2 = "pg2".to_string();

    let env_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: env_id,
        name: "env".to_string(),
        kube_name: format!("env-{}-my-env", to_short_id(&env_id)),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        max_parallel_build: 1,
        max_parallel_deploy: 1,
        applications: vec![
            Application {
                long_id: app_id,
                name: app_name_1.clone(),
                kube_name: app_name_1,
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                branch: "postgres-app".to_string(),
                commit_id: "71990e977a60c87034530614607494a96dee2254".to_string(),
                dockerfile_path: Some("Dockerfile-11".to_string()),
                command_args: vec![],
                entrypoint: None,
                root_path: "/".to_string(),
                action: Action::Create,
                git_credentials: None,
                storage: vec![],
                environment_vars_with_infos: btreemap! {
                     "PG_DBNAME".to_string() => VariableInfo{value: general_purpose::STANDARD.encode(database_name.clone()), is_secret: false},
                     "PG_HOST".to_string() => VariableInfo{value: general_purpose::STANDARD.encode(fqdn.clone()),is_secret: false},
                     "PG_PORT".to_string() => VariableInfo{value: general_purpose::STANDARD.encode(database_port.to_string()), is_secret: false},
                     "PG_USERNAME".to_string() => VariableInfo{value: general_purpose::STANDARD.encode(database_username.clone()), is_secret: false},
                     "PG_PASSWORD".to_string() => VariableInfo{value: general_purpose::STANDARD.encode(database_password.clone()), is_secret: false},
                },
                mounted_files: vec![],
                public_domain: format!("{}.example.com", app_id),
                ports: vec![Port {
                    long_id: Default::default(),
                    port: 1234,
                    is_default: true,
                    name: "p1234".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                }],
                cpu_request_in_milli: 100,
                cpu_limit_in_milli: 100,
                ram_request_in_mib: 256,
                ram_limit_in_mib: 256,
                min_instances: 1,
                max_instances: 1,
                advanced_settings: Default::default(),
                readiness_probe: Some(Probe {
                    r#type: ProbeType::Http {
                        path: "/".to_string(),
                        scheme: "HTTP".to_string(),
                    },
                    port: 1234,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                liveness_probe: Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 1234,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                container_registries: Vec::new(),
                annotations_group_ids: BTreeSet::new(),
                labels_group_ids: BTreeSet::new(),
                should_delete_shared_registry: false,
                shared_image_feature_enabled: false,
                docker_target_build_stage: None,
            },
            Application {
                long_id: Uuid::new_v4(),
                name: app_name_2.clone(),
                kube_name: app_name_2,
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                branch: "postgres-app".to_string(),
                commit_id: "71990e977a60c87034530614607494a96dee2254".to_string(),
                dockerfile_path: Some("Dockerfile-11".to_string()),
                command_args: vec![],
                entrypoint: None,
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: None,
                storage: vec![],
                environment_vars_with_infos: btreemap! {
                     "PG_DBNAME".to_string() => VariableInfo {value: general_purpose::STANDARD.encode(database_name_2.clone()), is_secret: false },
                     "PG_HOST".to_string() =>VariableInfo {value: general_purpose::STANDARD.encode(fqdn_2.clone()), is_secret: false },
                     "PG_PORT".to_string() => VariableInfo {value:general_purpose::STANDARD.encode(database_port.to_string()), is_secret: false },
                     "PG_USERNAME".to_string() =>VariableInfo {value: general_purpose::STANDARD.encode(database_username_2.clone()), is_secret: false },
                     "PG_PASSWORD".to_string() => VariableInfo {value:general_purpose::STANDARD.encode(database_password.clone()), is_secret: false },
                },
                mounted_files: vec![],
                ports: vec![Port {
                    long_id: Default::default(),
                    port: 1234,
                    is_default: true,
                    name: "p1234".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                }],
                cpu_request_in_milli: 100,
                cpu_limit_in_milli: 100,
                ram_request_in_mib: 256,
                ram_limit_in_mib: 256,
                min_instances: 1,
                max_instances: 1,
                advanced_settings: Default::default(),
                readiness_probe: Some(Probe {
                    r#type: ProbeType::Http {
                        path: "/".to_string(),
                        scheme: "HTTP".to_string(),
                    },
                    port: 1234,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                liveness_probe: Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 1234,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                public_domain: format!("{}.example.com", app_id),
                container_registries: Vec::new(),
                annotations_group_ids: BTreeSet::new(),
                labels_group_ids: BTreeSet::new(),
                should_delete_shared_registry: false,
                shared_image_feature_enabled: false,
                docker_target_build_stage: None,
            },
            Application {
                long_id: Uuid::new_v4(),
                name: app_name_3.clone(),
                kube_name: app_name_3,
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                branch: "mongo-app".to_string(),
                commit_id: "c5da00d2463061787e5fc2e31e7cd67877fd9881".to_string(),
                dockerfile_path: Some(format!("Dockerfile-{version_mongo}")),
                command_args: vec![],
                entrypoint: None,
                action: Action::Create,
                root_path: String::from("/"),
                git_credentials: None,
                storage: vec![],
                environment_vars_with_infos: btreemap! {
                    "IS_DOCUMENTDB".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(false.to_string()), is_secret:false},
                    "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(&database_host_mongo), is_secret:false},
                    "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_uri_mongo), is_secret:false},
                    "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_port_mongo.to_string()), is_secret:false},
                    "MONGODB_DBNAME".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(&database_db_name_mongo), is_secret:false},
                    "QOVERY_DATABASE_TESTING_DATABASE_USERNAME".to_string() =>VariableInfo { value:  general_purpose::STANDARD.encode(&database_username_mongo), is_secret:false},
                    "QOVERY_DATABASE_TESTING_DATABASE_PASSWORD".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(&database_password_mongo), is_secret:false},
                },
                mounted_files: vec![],
                public_domain: format!("{}.example.com", app_id),
                ports: vec![Port {
                    long_id: Default::default(),
                    port: 1234,
                    is_default: true,
                    name: "p1234".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                }],
                cpu_request_in_milli: 100,
                cpu_limit_in_milli: 100,
                ram_request_in_mib: 256,
                ram_limit_in_mib: 256,
                min_instances: 1,
                max_instances: 1,
                advanced_settings: Default::default(),
                readiness_probe: Some(Probe {
                    r#type: ProbeType::Http {
                        path: "/".to_string(),
                        scheme: "HTTP".to_string(),
                    },
                    port: 1234,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                liveness_probe: Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 1234,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                container_registries: Vec::new(),
                annotations_group_ids: BTreeSet::new(),
                labels_group_ids: BTreeSet::new(),
                should_delete_shared_registry: false,
                shared_image_feature_enabled: false,
                docker_target_build_stage: None,
            },
        ],
        containers: vec![],
        jobs: vec![],
        routers: vec![],
        databases: vec![
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                long_id: Uuid::new_v4(),
                name: database_name.clone(),
                kube_name: database_name,
                created_at: Utc::now(),
                version: "11.22.0".to_string(),
                fqdn_id: fqdn.clone(),
                fqdn,
                port: database_port,
                username: database_username,
                password: database_password.clone(),
                cpu_request_in_milli: 100,
                cpu_limit_in_milli: 100,
                ram_request_in_mib: 512,
                ram_limit_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: database_instance_type.as_ref().map(|i| i.to_cloud_provider_format()),
                database_disk_type: database_disk_type.to_string(),
                database_disk_iops: None,
                encrypt_disk: true,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
                mode: CONTAINER,
                annotations_group_ids: btreeset! {},
                labels_group_ids: btreeset! {},
            },
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                long_id: Uuid::new_v4(),
                name: database_name_2.clone(),
                kube_name: database_name_2,
                created_at: Utc::now(),
                version: "11.22.0".to_string(),
                fqdn_id: fqdn_2.clone(),
                fqdn: fqdn_2,
                port: database_port,
                username: database_username_2,
                password: database_password,
                cpu_request_in_milli: 100,
                cpu_limit_in_milli: 100,
                ram_request_in_mib: 512,
                ram_limit_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: database_instance_type.as_ref().map(|i| i.to_cloud_provider_format()),
                database_disk_type: database_disk_type.to_string(),
                database_disk_iops: None,
                encrypt_disk: true,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
                mode: CONTAINER,
                annotations_group_ids: btreeset! {},
                labels_group_ids: btreeset! {},
            },
            Database {
                kind: DatabaseKind::Mongodb,
                action: Action::Create,
                long_id: Uuid::new_v4(),
                name: database_db_name_mongo.clone(),
                kube_name: database_db_name_mongo.to_string(),
                created_at: Utc::now(),
                version: version_mongo.to_string(),
                fqdn_id: database_host_mongo.clone(),
                fqdn: database_host_mongo,
                port: database_port_mongo,
                username: database_username_mongo,
                password: database_password_mongo,
                cpu_request_in_milli: 500,
                cpu_limit_in_milli: 500,
                ram_request_in_mib: 512,
                ram_limit_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: database_instance_type.as_ref().map(|i| i.to_cloud_provider_format()),
                database_disk_type: database_disk_type.to_string(),
                database_disk_iops: None,
                encrypt_disk: true,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
                mode: CONTAINER,
                annotations_group_ids: btreeset! {},
                labels_group_ids: btreeset! {},
            },
        ],
        helms: vec![],
        terraform_services: vec![],
        annotations_groups: btreemap! {},
        labels_groups: btreemap! {},
    }
}

pub fn database_test_environment(context: &Context) -> EnvironmentRequest {
    let suffix = generate_id();
    let application_name = format!("{}-{}", "simple-app", &suffix);

    let env_id = Uuid::new_v4();
    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: env_id,
        name: "env".to_string(),
        kube_name: format!("env-{}-my-env", to_short_id(&env_id)),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        max_parallel_build: 1,
        max_parallel_deploy: 1,
        applications: vec![Application {
            long_id: Uuid::new_v4(),
            name: application_name.clone(),
            kube_name: application_name,
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "4bc6a902e83129a118185660b3c9e13dfd0ffc27".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            branch: "basic-app-deploy".to_string(),
            command_args: vec![],
            entrypoint: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: None,
            storage: vec![],
            environment_vars_with_infos: BTreeMap::default(),
            mounted_files: vec![],
            ports: vec![],
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 256,
            ram_limit_in_mib: 256,
            min_instances: 1,
            max_instances: 1,
            advanced_settings: Default::default(),
            readiness_probe: None,
            liveness_probe: None,
            public_domain: format!("{}.example.com", Uuid::new_v4()),
            container_registries: Vec::new(),
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
            docker_target_build_stage: None,
        }],
        containers: vec![],
        jobs: vec![],
        routers: vec![],
        databases: vec![],
        helms: vec![],
        terraform_services: vec![],
        annotations_groups: btreemap! {},
        labels_groups: btreemap! {},
    }
}

pub fn database_test_environment_on_upgrade(context: &Context) -> EnvironmentRequest {
    let suffix = Uuid::new_v4();
    let application_name = format!("{}-{}", "simple-app", to_short_id(&suffix));

    let env_id = Uuid::new_v4();
    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: env_id,
        name: "env".to_string(),
        kube_name: format!("env-{}-my-env", to_short_id(&env_id)),
        project_long_id: suffix,
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        max_parallel_build: 1,
        max_parallel_deploy: 1,
        applications: vec![Application {
            long_id: Uuid::from_str("9d0158db-b783-4bc2-a23b-c7d9228cbe90").unwrap(),
            name: application_name.clone(),
            kube_name: application_name,
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            command_args: vec![],
            entrypoint: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: None,
            storage: vec![],
            environment_vars_with_infos: BTreeMap::default(),
            mounted_files: vec![],
            branch: "basic-app-deploy".to_string(),
            public_domain: format!("{}.example.com", Uuid::new_v4()),
            ports: vec![],
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 256,
            ram_limit_in_mib: 256,
            min_instances: 1,
            max_instances: 1,
            advanced_settings: Default::default(),
            readiness_probe: None,
            liveness_probe: None,
            container_registries: Vec::new(),
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
            docker_target_build_stage: None,
        }],
        containers: vec![],
        jobs: vec![],
        routers: vec![],
        databases: vec![],
        terraform_services: vec![],
        helms: vec![],
        annotations_groups: btreemap! {},
        labels_groups: btreemap! {},
    }
}

pub fn test_db(
    context: Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    mut environment: EnvironmentRequest,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    db_kind: DatabaseKind,
    kubernetes_kind: KubernetesKind,
    database_mode: DatabaseMode,
    localisation: String,
    is_public: bool,
    cluster_domain: ClusterDomain,
    existing_infra_ctx: Option<&InfrastructureContext>,
    storage_size: StorageSize,
) -> String {
    let sem_ver = match VersionsNumber::from_str(version) {
        Ok(v) => v,
        Err(e) => panic!("Database version has a wrong format: `{}`, error: {}", version, e),
    };
    let context_for_delete = context.clone_not_same_execution_id();
    let provider_kind = kubernetes_kind.get_cloud_provider_kind();
    let app_id = Uuid::new_v4();
    let database_username = match db_kind {
        DatabaseKind::Redis => match database_mode {
            MANAGED => match sem_ver.to_major_version_string().as_str() {
                "7" => "default".to_string(),
                "6" => "default".to_string(),
                _ => "superuser".to_string(),
            },
            CONTAINER => "".to_string(),
        },
        DatabaseKind::Mysql => match database_mode {
            CONTAINER => "qovery".to_string(),
            _ => "superuser".to_string(),
        },
        _ => "superuser".to_string(),
    };
    let database_password = generate_password(database_mode.clone());
    let db_kind_str = db_kind.name().to_string();
    let db_id = generate_id();
    let database_host = format!("{}-{}", to_short_id(&db_id), db_kind_str);
    let database_fqdn = format!(
        "{}.{}",
        database_host,
        compute_test_cluster_endpoint(
            &cluster_domain,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN must be set")
                .to_string()
        )
    );

    let db_infos = db_infos(
        db_kind.clone(),
        to_short_id(&db_id),
        database_mode.clone(),
        database_username.clone(),
        database_password.clone(),
        if is_public {
            database_fqdn.clone()
        } else {
            database_host.clone()
        },
    );
    let database_port = db_infos.db_port;
    let disk_size = match storage_size {
        StorageSize::Resize => StorageSize::NormalSize.size(),
        _ => storage_size.size(),
    };
    let db_disk_type = db_disk_type(provider_kind.clone(), database_mode.clone());
    let db_instance_type = db_instance_type(provider_kind.clone(), db_kind.clone(), database_mode.clone());
    let db = Database {
        kind: db_kind.clone(),
        action: Action::Create,
        long_id: Uuid::new_v4(),
        name: to_short_id(&db_id),
        kube_name: to_short_id(&db_id),
        created_at: Utc::now(),
        version: version.to_string(),
        fqdn_id: database_host.clone(),
        fqdn: database_fqdn.clone(),
        port: database_port,
        username: database_username,
        password: database_password,
        cpu_request_in_milli: 500,
        cpu_limit_in_milli: 500,
        ram_request_in_mib: 512,
        ram_limit_in_mib: 512,
        disk_size_in_gib: disk_size,
        database_instance_type: db_instance_type.map(|i| i.to_cloud_provider_format()),
        database_disk_type: db_disk_type,
        database_disk_iops: None,
        encrypt_disk: true,
        activate_high_availability: false,
        activate_backups: false,
        publicly_accessible: is_public,
        mode: database_mode.clone(),
        annotations_group_ids: btreeset! {},
        labels_group_ids: btreeset! {},
    };

    environment.databases = vec![db.clone()];

    let app_name = format!("{}-app-{}", db_kind_str, QoveryIdentifier::new_random().short());
    let branch = format!(
        "{}-app",
        match db_kind {
            DatabaseKind::Postgresql => "postgres",
            DatabaseKind::Mysql => "mysql",
            DatabaseKind::Mongodb => "mongo",
            DatabaseKind::Redis => "redis",
        }
    );
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.long_id = app_id;
            app.name = app_name.to_string();
            app.branch.clone_from(&branch);
            app.commit_id.clone_from(&db_infos.app_commit);
            app.ports = vec![Port {
                long_id: Default::default(),
                port: 1234,
                is_default: true,
                name: "p1234".to_string(),
                publicly_accessible: true,
                protocol: Protocol::HTTP,
                service_name: None,
                namespace: None,
                additional_service: None,
            }];
            app.dockerfile_path = match db_kind {
                // to be able to support outdated container image versions, we jump to a higher version
                DatabaseKind::Mongodb if version.contains("4.0") => Some("Dockerfile-4.4".to_string()),
                _ => Some(format!("Dockerfile-{version}")),
            };
            app.command_args = vec![];
            app.entrypoint = None;
            app.environment_vars_with_infos = db_infos.app_env_vars.clone();
            app
        })
        .collect::<Vec<Application>>();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = environment.clone();
    let ea_delete = environment_delete.clone();

    let kubernetes_version = match kubernetes_kind {
        KubernetesKind::Aks | KubernetesKind::AksSelfManaged => AZURE_KUBERNETES_VERSION,
        KubernetesKind::Eks | KubernetesKind::EksSelfManaged => AWS_KUBERNETES_VERSION,
        KubernetesKind::ScwKapsule | KubernetesKind::ScwSelfManaged => SCW_KUBERNETES_VERSION,
        KubernetesKind::Gke | KubernetesKind::GkeSelfManaged => GCP_KUBERNETES_VERSION,
        KubernetesKind::OnPremiseSelfManaged => ON_PREMISE_KUBERNETES_VERSION,
    };

    let computed_infra_ctx: InfrastructureContext;
    let infra_ctx = match existing_infra_ctx {
        Some(c) => c,
        None => {
            computed_infra_ctx = match kubernetes_kind {
                KubernetesKind::Eks | KubernetesKind::EksSelfManaged => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Eks,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.AWS_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
                    NodeManager::Default,
                ),
                KubernetesKind::Aks | KubernetesKind::AksSelfManaged => Azure::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Aks,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.AZURE_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
                    NodeManager::Default,
                ),
                KubernetesKind::ScwKapsule | KubernetesKind::ScwSelfManaged => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.SCALEWAY_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
                    NodeManager::Default,
                ),
                KubernetesKind::Gke | KubernetesKind::GkeSelfManaged => Gke::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Gke,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::QoverySide,
                    secrets.GCP_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
                    NodeManager::Default,
                ),
                KubernetesKind::OnPremiseSelfManaged => todo!(), // TODO how to test on-premise clusers ?
            };
            &computed_infra_ctx
        }
    };

    let ret = environment.deploy_environment(&ea, infra_ctx);
    match storage_size {
        StorageSize::NormalSize => assert!(ret.is_ok()),
        StorageSize::OverSize => assert!(ret.is_err()),
        StorageSize::Resize => {
            let mut resized_env = environment.clone();
            resized_env.databases[0].disk_size_in_gib = StorageSize::Resize.size();
            assert!(resized_env.deploy_environment(&resized_env, infra_ctx).is_ok())
        }
    }

    match database_mode {
        CONTAINER => {
            match get_pvc(infra_ctx, provider_kind.clone(), &environment) {
                Ok(pvc) => assert_eq!(
                    pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                    format!("{}Gi", storage_size.size())
                ),
                Err(e) => panic!("Error: {e}"),
            };

            match get_svc(infra_ctx, provider_kind, environment) {
                Ok(svc) => assert_eq!(
                    svc.items
                        .expect("No items in svc")
                        .into_iter()
                        .filter(|svc| svc.metadata.name == database_host && &svc.spec.svc_type == "LoadBalancer")
                        .count(),
                    match is_public {
                        true => 1,
                        false => 0,
                    }
                ),
                Err(e) => panic!("Error: {e}"),
            };
        }
        MANAGED => {
            match get_svc(infra_ctx, provider_kind, environment) {
                Ok(svc) => {
                    let service = svc
                        .items
                        .expect("No items in svc")
                        .into_iter()
                        .filter(|svc| svc.metadata.name == database_host && svc.spec.svc_type == "ExternalName")
                        .collect::<Vec<SVCItem>>();
                    let annotations = &service[0].metadata.annotations;
                    assert_eq!(service.len(), 1);
                    match is_public {
                        true => {
                            if db.kind == DatabaseKind::Postgresql || db.kind == DatabaseKind::Mysql {
                                assert!(annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                                assert_eq!(annotations["external-dns.alpha.kubernetes.io/hostname"], database_fqdn);
                            } else {
                                assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                            }
                        }
                        false => assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname")),
                    }
                }
                Err(e) => panic!("Error: {e}"),
            };
        }
    }

    let computed_infra_ctx_for_delete: InfrastructureContext;
    let infra_ctx_for_delete = match existing_infra_ctx {
        Some(c) => c,
        None => {
            computed_infra_ctx_for_delete = match kubernetes_kind {
                KubernetesKind::Eks => AWS::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Eks,
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.AWS_TEST_KUBECONFIG_b64,
                    NodeManager::Default,
                ),
                KubernetesKind::Aks => Azure::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Aks,
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.AZURE_TEST_KUBECONFIG_b64,
                    NodeManager::Default,
                ),
                KubernetesKind::ScwKapsule => Scaleway::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.SCALEWAY_TEST_KUBECONFIG_b64,
                    NodeManager::Default,
                ),
                KubernetesKind::Gke => Gke::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.GCP_TEST_KUBECONFIG_b64,
                    NodeManager::Default,
                ),
                KubernetesKind::EksSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::AksSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::GkeSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::ScwSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::OnPremiseSelfManaged => todo!(), // TODO how to test on-premise clusers ?
            };
            &computed_infra_ctx_for_delete
        }
    };

    let ret = environment_delete.delete_environment(&ea_delete, infra_ctx_for_delete);
    assert!(ret.is_ok());

    test_name.to_string()
}

pub fn test_pause_managed_db(
    context: Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    mut environment: EnvironmentRequest,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    db_kind: DatabaseKind,
    kubernetes_kind: KubernetesKind,
    database_mode: DatabaseMode,
    is_public: bool,
    cluster_domain: ClusterDomain,
    existing_infra_ctx: Option<&InfrastructureContext>,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();

    let context_for_delete = context.clone_not_same_execution_id();

    let provider_kind = kubernetes_kind.get_cloud_provider_kind();
    let database_username = "superuser".to_string();
    let database_password = generate_password(database_mode.clone());
    let db_kind_str = db_kind.name().to_string();
    let db_id = generate_id();
    let database_host = format!("{}-{}", to_short_id(&db_id), db_kind_str);
    let database_fqdn = format!(
        "{}.{}",
        database_host,
        compute_test_cluster_endpoint(
            &cluster_domain,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN must be set")
                .to_string(),
        )
    );

    let db_infos = db_infos(
        db_kind.clone(),
        to_short_id(&db_id),
        database_mode.clone(),
        database_username.clone(),
        database_password.clone(),
        if is_public {
            database_fqdn.clone()
        } else {
            database_host.clone()
        },
    );
    let database_port = db_infos.db_port;
    let storage_size = 10;
    let db_disk_type = db_disk_type(provider_kind.clone(), database_mode.clone());
    let db_instance_type = db_instance_type(provider_kind.clone(), db_kind.clone(), database_mode.clone());
    let db = Database {
        kind: db_kind,
        action: Action::Create,
        long_id: Uuid::new_v4(),
        name: to_short_id(&db_id),
        kube_name: to_short_id(&db_id),
        created_at: Utc::now(),
        version: version.to_string(),
        fqdn_id: database_host.clone(),
        fqdn: database_fqdn.clone(),
        port: database_port,
        username: database_username,
        password: database_password,
        cpu_request_in_milli: 250,
        cpu_limit_in_milli: 250,
        ram_request_in_mib: 512,
        ram_limit_in_mib: 512,
        disk_size_in_gib: storage_size,
        database_instance_type: db_instance_type.map(|i| i.to_cloud_provider_format()),
        database_disk_type: db_disk_type,
        database_disk_iops: None,
        encrypt_disk: true,
        activate_high_availability: false,
        activate_backups: false,
        publicly_accessible: is_public,
        mode: database_mode.clone(),
        annotations_group_ids: btreeset! {},
        labels_group_ids: btreeset! {},
    };

    environment.databases = vec![db];
    environment.applications = vec![];

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let mut environment_pause = environment.clone();
    environment_pause.action = Action::Pause;

    let (localisation, kubernetes_version) = match provider_kind {
        Kind::Aws => (
            secrets
                .AWS_TEST_CLUSTER_REGION
                .as_ref()
                .expect("AWS_TEST_CLUSTER_REGION is not set")
                .to_string(),
            AWS_KUBERNETES_VERSION,
        ),
        Kind::Azure => (AZURE_LOCATION.to_cloud_provider_format().to_string(), AZURE_KUBERNETES_VERSION),
        Kind::Scw => (
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string(),
            SCW_KUBERNETES_VERSION,
        ),
        Kind::Gcp => todo!(),
        Kind::OnPremise => todo!(),
    };

    let computed_infra_ctx: InfrastructureContext;
    let infra_ctx = match existing_infra_ctx {
        Some(c) => c,
        None => {
            computed_infra_ctx = match kubernetes_kind {
                KubernetesKind::Eks => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Eks,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.AWS_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
                    NodeManager::Default,
                ),
                KubernetesKind::Aks => Azure::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Aks,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.AZURE_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
                    NodeManager::Default,
                ),
                KubernetesKind::ScwKapsule => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.SCALEWAY_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
                    NodeManager::Default,
                ),
                KubernetesKind::Gke => todo!(), // TODO(benjaminch): GKE integration
                KubernetesKind::EksSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::AksSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::GkeSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::ScwSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::OnPremiseSelfManaged => todo!(), // TODO how to test on-premise clusers ?
            };
            &computed_infra_ctx
        }
    };

    let ret = environment.deploy_environment(&environment, infra_ctx);
    assert!(ret.is_ok());

    match database_mode {
        CONTAINER => {
            match get_pvc(infra_ctx, provider_kind.clone(), &environment) {
                Ok(pvc) => assert_eq!(
                    pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                    format!("{storage_size}Gi")
                ),
                Err(e) => panic!("Error: {e}"),
            };

            match get_svc(infra_ctx, provider_kind, environment) {
                Ok(svc) => {
                    assert!(svc.items.is_some());
                    assert_eq!(
                        svc.items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| svc.metadata.name == database_host && &svc.spec.svc_type == "LoadBalancer")
                            .count(),
                        match is_public {
                            true => 1,
                            false => 0,
                        }
                    );
                }
                Err(e) => panic!("Error: {e}"),
            };
        }
        MANAGED => {
            match get_svc(infra_ctx, provider_kind, environment) {
                Ok(svc) => {
                    assert!(svc.items.is_some());
                    let service = svc
                        .items
                        .expect("No items in svc")
                        .into_iter()
                        .filter(|svc| svc.metadata.name == database_host && svc.spec.svc_type == "ExternalName")
                        .collect::<Vec<SVCItem>>();
                    let annotations = &service[0].metadata.annotations;
                    assert_eq!(service.len(), 1);
                    match is_public {
                        true => {
                            assert!(annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                            assert_eq!(annotations["external-dns.alpha.kubernetes.io/hostname"], database_fqdn);
                        }
                        false => assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname")),
                    }
                }
                Err(e) => panic!("Error: {e}"),
            };
        }
    }

    let computed_infra_ctx_for_delete: InfrastructureContext;
    let infra_ctx_for_delete = match existing_infra_ctx {
        Some(c) => c,
        None => {
            computed_infra_ctx_for_delete = match kubernetes_kind {
                KubernetesKind::Eks => AWS::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Eks,
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.AWS_TEST_KUBECONFIG_b64,
                    NodeManager::Default,
                ),
                KubernetesKind::Aks => Azure::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::Aks,
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.AZURE_TEST_KUBECONFIG_b64,
                    NodeManager::Default,
                ),
                KubernetesKind::ScwKapsule => Scaleway::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    metrics_registry.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                    secrets.SCALEWAY_TEST_KUBECONFIG_b64,
                    NodeManager::Default,
                ),
                KubernetesKind::Gke => todo!(), // TODO(benjaminch): GKE integration
                KubernetesKind::EksSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::AksSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::GkeSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::ScwSelfManaged => todo!(), // TODO byok integration
                KubernetesKind::OnPremiseSelfManaged => todo!(), // TODO how to test on-premise clusers ?
            };
            &computed_infra_ctx_for_delete
        }
    };

    let ret = environment_pause.pause_environment(&environment_pause, infra_ctx);
    assert!(ret.is_ok());

    let ret = environment_delete.delete_environment(&environment_delete, infra_ctx_for_delete);
    assert!(ret.is_ok());

    test_name.to_string()
}

pub fn test_db_on_upgrade(
    context: Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    mut environment: EnvironmentRequest,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    db_kind: DatabaseKind,
    provider_kind: Kind,
    database_mode: DatabaseMode,
    is_public: bool,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();

    let context_for_delete = context.clone_not_same_execution_id();

    let app_id = Uuid::from_str("8d0158db-b783-4bc2-a23b-c7d9228cbe90").unwrap();
    let database_username = "superuser".to_string();
    let database_password = "uxoyf358jojkemj".to_string();
    let db_kind_str = db_kind.name().to_string();
    let db_id = "c2dn5so3dltod3s".to_string();
    let database_host = format!("{db_id}-{db_kind_str}");
    let database_fqdn = format!(
        "{}.{}.{}",
        database_host,
        context.cluster_short_id(),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
    );

    let db_infos = db_infos(
        db_kind.clone(),
        db_id.clone(),
        database_mode.clone(),
        database_username.clone(),
        database_password.clone(),
        if is_public {
            database_fqdn.clone()
        } else {
            database_host.clone()
        },
    );
    let database_port = db_infos.db_port;
    let storage_size = 10;
    let db_disk_type = db_disk_type(provider_kind.clone(), database_mode.clone());
    let db_instance_type = db_instance_type(provider_kind.clone(), db_kind.clone(), database_mode.clone());
    let db = Database {
        kind: db_kind,
        action: Action::Create,
        long_id: Uuid::from_str("7d0158db-b783-4bc2-a23b-c7d9228cbe90").unwrap(),
        name: db_id.clone(),
        kube_name: db_id,
        created_at: Utc::now(),
        version: version.to_string(),
        fqdn_id: database_host.clone(),
        fqdn: database_fqdn.clone(),
        port: database_port,
        username: database_username,
        password: database_password,
        cpu_request_in_milli: 50,
        cpu_limit_in_milli: 50,
        ram_request_in_mib: 256,
        ram_limit_in_mib: 256,
        disk_size_in_gib: storage_size,
        database_instance_type: db_instance_type.map(|i| i.to_cloud_provider_format()),
        database_disk_type: db_disk_type,
        database_disk_iops: None,
        encrypt_disk: true,
        activate_high_availability: false,
        activate_backups: false,
        publicly_accessible: is_public,
        mode: database_mode.clone(),
        annotations_group_ids: btreeset! {},
        labels_group_ids: btreeset! {},
    };

    environment.databases = vec![db];

    let app_name = format!("{}-app-{}", db_kind_str, generate_id());
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.long_id = app_id;
            app.name = to_short_id(&app_id);
            app.branch.clone_from(&app_name);
            app.commit_id.clone_from(&db_infos.app_commit);
            app.ports = vec![Port {
                long_id: Default::default(),
                port: 1234,
                is_default: true,
                name: "p1234".to_string(),
                publicly_accessible: true,
                protocol: Protocol::HTTP,
                service_name: None,
                namespace: None,
                additional_service: None,
            }];
            app.dockerfile_path = Some(format!("Dockerfile-{version}"));
            app.environment_vars_with_infos = db_infos.app_env_vars.clone();
            app
        })
        .collect::<Vec<Application>>();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = environment.clone();
    let ea_delete = environment_delete.clone();

    let (localisation, kubernetes_version) = match provider_kind {
        Kind::Aws => (
            secrets
                .AWS_TEST_CLUSTER_REGION
                .as_ref()
                .expect("AWS_TEST_CLUSTER_REGION is not set")
                .to_string(),
            AWS_KUBERNETES_VERSION,
        ),
        Kind::Azure => (AZURE_LOCATION.to_cloud_provider_format().to_string(), AZURE_KUBERNETES_VERSION),
        Kind::Scw => (
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string(),
            SCW_KUBERNETES_VERSION,
        ),
        Kind::Gcp => todo!(), // TODO(benjaminch): GKE integration
        Kind::OnPremise => todo!(),
    };

    let infra_ctx = match provider_kind {
        Kind::Aws => AWS::docker_cr_engine(
            &context,
            logger.clone(),
            metrics_registry.clone(),
            localisation.as_str(),
            KubernetesKind::Eks,
            kubernetes_version.clone(),
            &ClusterDomain::Default {
                cluster_id: context.cluster_short_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            CpuArchitecture::AMD64,
            EngineLocation::ClientSide,
            secrets.AWS_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
            NodeManager::Default,
        ),
        Kind::Azure => Azure::docker_cr_engine(
            &context,
            logger.clone(),
            metrics_registry.clone(),
            localisation.as_str(),
            KubernetesKind::Aks,
            kubernetes_version.clone(),
            &ClusterDomain::Default {
                cluster_id: context.cluster_short_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            CpuArchitecture::AMD64,
            EngineLocation::ClientSide,
            secrets.AZURE_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
            NodeManager::Default,
        ),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context,
            logger.clone(),
            metrics_registry.clone(),
            localisation.as_str(),
            KubernetesKind::ScwKapsule,
            kubernetes_version.clone(),
            &ClusterDomain::Default {
                cluster_id: context.cluster_short_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            CpuArchitecture::AMD64,
            EngineLocation::ClientSide,
            secrets.SCALEWAY_TEST_KUBECONFIG_b64.as_ref().map(|s| s.to_string()),
            NodeManager::Default,
        ),
        Kind::Gcp => todo!(), // TODO(benjaminch): GKE integration
        Kind::OnPremise => todo!(),
    };

    let ret = environment.deploy_environment(&ea, &infra_ctx);
    assert!(ret.is_ok());

    match database_mode {
        CONTAINER => {
            match get_pvc(&infra_ctx, provider_kind.clone(), &environment) {
                Ok(pvc) => assert_eq!(
                    pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                    format!("{storage_size}Gi")
                ),
                Err(e) => panic!("Error: {e}"),
            };

            match get_svc(&infra_ctx, provider_kind.clone(), environment) {
                Ok(svc) => assert_eq!(
                    svc.items
                        .expect("No items in svc")
                        .into_iter()
                        .filter(|svc| svc.metadata.name == database_host && &svc.spec.svc_type == "LoadBalancer")
                        .count(),
                    match is_public {
                        true => 1,
                        false => 0,
                    }
                ),
                Err(e) => panic!("Error: {e}"),
            };
        }
        MANAGED => {
            match get_svc(&infra_ctx, provider_kind.clone(), environment) {
                Ok(svc) => {
                    let service = svc
                        .items
                        .expect("No items in svc")
                        .into_iter()
                        .filter(|svc| svc.metadata.name == database_host && svc.spec.svc_type == "ExternalName")
                        .collect::<Vec<SVCItem>>();
                    let annotations = &service[0].metadata.annotations;
                    assert_eq!(service.len(), 1);
                    match is_public {
                        true => {
                            assert!(annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                            assert_eq!(annotations["external-dns.alpha.kubernetes.io/hostname"], database_fqdn);
                        }
                        false => assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname")),
                    }
                }
                Err(e) => panic!("Error: {e}"),
            };
        }
    }

    let infra_ctx_for_delete = match provider_kind {
        Kind::Aws => AWS::docker_cr_engine(
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
            localisation.as_str(),
            KubernetesKind::Eks,
            kubernetes_version,
            &ClusterDomain::Default {
                cluster_id: context_for_delete.cluster_short_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            CpuArchitecture::AMD64,
            EngineLocation::ClientSide,
            secrets.AWS_TEST_KUBECONFIG_b64,
            NodeManager::Default,
        ),
        Kind::Azure => Azure::docker_cr_engine(
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
            localisation.as_str(),
            KubernetesKind::Aks,
            kubernetes_version,
            &ClusterDomain::Default {
                cluster_id: context_for_delete.cluster_short_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            CpuArchitecture::AMD64,
            EngineLocation::ClientSide,
            secrets.AZURE_TEST_KUBECONFIG_b64,
            NodeManager::Default,
        ),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
            localisation.as_str(),
            KubernetesKind::ScwKapsule,
            kubernetes_version,
            &ClusterDomain::Default {
                cluster_id: context_for_delete.cluster_short_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            CpuArchitecture::AMD64,
            EngineLocation::ClientSide,
            secrets.SCALEWAY_TEST_KUBECONFIG_b64,
            NodeManager::Default,
        ),
        Kind::Gcp => todo!(), // TODO(benjaminch): GKE integration
        Kind::OnPremise => todo!(),
    };

    let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_delete);
    assert!(ret.is_ok());

    test_name.to_string()
}

pub fn test_deploy_an_environment_with_db_and_resize_disk(
    db_kind: DatabaseKind,
    db_version: &str,
    test_name: &str,
    kubernetes_kind: KubernetesKind,
) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets
        .AWS_TEST_CLUSTER_LONG_ID
        .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
    let context = context_for_resource(
        secrets
            .AWS_TEST_ORGANIZATION_LONG_ID
            .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
        cluster_id,
    );
    let localisation = secrets
        .AWS_TEST_CLUSTER_REGION
        .as_ref()
        .expect("AWS_TEST_CLUSTER_REGION is not set")
        .to_string();

    let environment = database_test_environment(&context);

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        test_db(
            context,
            logger(),
            metrics_registry(),
            environment,
            secrets,
            db_version,
            test_name,
            db_kind,
            kubernetes_kind,
            CONTAINER,
            localisation,
            false,
            ClusterDomain::Default {
                cluster_id: cluster_id.to_string(),
            },
            None,
            StorageSize::Resize,
        )
    })
}
