use crate::helpers::aws::aws_infra_config;
use crate::helpers::database::StorageSize::NormalSize;
use crate::helpers::kubernetes::TargetCluster;
use crate::helpers::utilities::{
    context_for_resource, generate_id, get_svc_name, logger, metrics_registry, FuncTestsSecrets,
};
use chrono::Utc;
use qovery_engine::environment::models::aws::AwsStorageType;
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::cloud_provider::Kind::Aws;
use qovery_engine::io_models::application::{Application, Port, Protocol, Storage};
use qovery_engine::io_models::container::{Container, Registry};
use qovery_engine::io_models::database::DatabaseMode::CONTAINER;
use qovery_engine::io_models::database::{Database, DatabaseKind};
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::job::{ContainerRegistries, Job, JobSchedule, JobSource};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::{Action, QoveryIdentifier};
use qovery_engine::utilities::to_short_id;
use std::collections::{BTreeMap, BTreeSet};
use url::Url;
use uuid::Uuid;

mod application;
mod container;
mod database;
mod jobs;

/// This mod holds kubernetes tests for features not specific to any cloud providers.

pub enum TestEnvOption {
    WithDB,
    WithContainer,
    WithApp,
    WithJob,
}

pub fn kube_test_env(options: TestEnvOption) -> (InfrastructureContext, EnvironmentRequest) {
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
    let target_cluster_aws_test = TargetCluster::MutualizedTestCluster {
        kubeconfig: secrets
            .AWS_TEST_KUBECONFIG_b64
            .expect("AWS_TEST_KUBECONFIG_b64 is not set")
            .to_string(),
    };

    let logger = logger();
    let metrics_registry = metrics_registry();
    let infra_ctx = aws_infra_config(&target_cluster_aws_test, &context, logger.clone(), metrics_registry.clone());

    let env_id = Uuid::new_v4();
    let mut environment = EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: env_id,
        name: "env".to_string(),
        kube_name: format!("env-{}-my-env", to_short_id(&env_id)),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        max_parallel_build: 1,
        max_parallel_deploy: 1,
        applications: vec![],
        containers: vec![],
        jobs: vec![],
        routers: vec![],
        databases: vec![],
        helms: vec![],
        annotations_groups: btreemap! {},
        labels_groups: btreemap! {},
    };

    match options {
        TestEnvOption::WithDB => {
            let db_id = Uuid::new_v4();
            let database_host = get_svc_name(DatabaseKind::Postgresql, Aws).to_string();
            let db = Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                long_id: db_id,
                name: to_short_id(&db_id),
                kube_name: to_short_id(&db_id),
                created_at: Utc::now(),
                version: "13".to_string(),
                fqdn_id: database_host.clone(),
                fqdn: database_host,
                port: 5432,
                username: "superuser".to_string(),
                password: generate_id().to_string(),
                cpu_request_in_milli: 250,
                cpu_limit_in_milli: 250,
                ram_request_in_mib: 512,
                ram_limit_in_mib: 512, // MySQL requires at least 512Mo in order to boot
                disk_size_in_gib: NormalSize.size(),
                database_disk_type: AwsStorageType::GP2.to_k8s_storage_class(),
                encrypt_disk: true,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
                mode: CONTAINER,
                database_instance_type: None,
                annotations_group_ids: btreeset! {},
                labels_group_ids: btreeset! {},
            };
            environment.databases = vec![db];
        }
        TestEnvOption::WithContainer => {
            let container_id = QoveryIdentifier::new_random();
            let container_name = container_id.short().to_string();
            let storage_1_id = QoveryIdentifier::new_random().to_uuid();
            let storage_2_id = QoveryIdentifier::new_random().to_uuid();
            let service_id = Uuid::new_v4();
            let container = Container {
                long_id: service_id,
                name: container_name.clone(),
                kube_name: container_name,
                action: Action::Create,
                registry: Registry::PublicEcr {
                    long_id: Uuid::new_v4(),
                    url: Url::parse("https://public.ecr.aws").unwrap(),
                },
                image: "r3m4q3r9/pub-mirror-debian".to_string(),
                tag: "11.6-ci".to_string(),
                command_args: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    r#"
                apt-get update;
                apt-get install -y socat procps iproute2;
                echo listening on port $PORT;
                env
                socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
                "#
                    .to_string(),
                ],
                entrypoint: None,
                cpu_request_in_milli: 250,
                cpu_limit_in_milli: 250,
                ram_request_in_mib: 250,
                ram_limit_in_mib: 250,
                min_instances: 1,
                max_instances: 1,
                public_domain: format!("{}.{}", service_id, infra_ctx.dns_provider().domain()),
                ports: vec![Port {
                    long_id: Uuid::new_v4(),
                    port: 8080,
                    is_default: true,
                    name: "http".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                }],
                readiness_probe: Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 8080,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                liveness_probe: Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 8080,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                storages: vec![
                    Storage {
                        id: to_short_id(&storage_1_id),
                        long_id: storage_1_id,
                        name: "photos1".to_string(),
                        storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
                        size_in_gib: NormalSize.size(),
                        mount_point: "/mnt/photos1".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                    Storage {
                        id: to_short_id(&storage_2_id),
                        long_id: storage_2_id,
                        name: "photos2".to_string(),
                        storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
                        size_in_gib: NormalSize.size(),
                        mount_point: "/mnt/photos2".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                ],
                environment_vars_with_infos: BTreeMap::default(),
                advanced_settings: Default::default(),
                mounted_files: vec![],
                annotations_group_ids: BTreeSet::new(),
                labels_group_ids: btreeset! {},
            };
            environment.containers = vec![container];
        }
        TestEnvOption::WithApp => {
            let application_id = QoveryIdentifier::new_random();
            let application_name = application_id.short().to_string();
            let storage_1_id = QoveryIdentifier::new_random().to_uuid();
            let storage_2_id = QoveryIdentifier::new_random().to_uuid();
            let app = Application {
                long_id: application_id.to_uuid(),
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
                storage: vec![
                    Storage {
                        id: to_short_id(&storage_1_id),
                        long_id: storage_1_id,
                        name: "photos1".to_string(),
                        storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
                        size_in_gib: NormalSize.size(),
                        mount_point: "/mnt/photos1".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                    Storage {
                        id: to_short_id(&storage_2_id),
                        long_id: storage_2_id,
                        name: "photos2".to_string(),
                        storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
                        size_in_gib: NormalSize.size(),
                        mount_point: "/mnt/photos2".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                ],
                environment_vars_with_infos: BTreeMap::default(),
                branch: "basic-app-deploy".to_string(),
                public_domain: format!("{}.{}", application_id, infra_ctx.dns_provider().domain()),
                ports: vec![Port {
                    long_id: Default::default(),
                    port: 80,
                    is_default: true,
                    name: "p80".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                }],
                readiness_probe: Some(Probe {
                    r#type: ProbeType::Http {
                        path: "/".to_string(),
                        scheme: "HTTP".to_string(),
                    },
                    port: 80,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                liveness_probe: Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 80,
                    initial_delay_seconds: 1,
                    timeout_seconds: 2,
                    period_seconds: 3,
                    success_threshold: 1,
                    failure_threshold: 5,
                }),
                cpu_request_in_milli: 100,
                cpu_limit_in_milli: 100,
                ram_request_in_mib: 256,
                ram_limit_in_mib: 256,
                min_instances: 1,
                max_instances: 1,
                advanced_settings: Default::default(),
                mounted_files: vec![],
                container_registries: Vec::new(),
                annotations_group_ids: BTreeSet::new(),
                labels_group_ids: btreeset! {},
                should_delete_shared_registry: false,
                shared_image_feature_enabled: false,
            };
            environment.applications = vec![app];
        }
        TestEnvOption::WithJob => {
            let job_id = QoveryIdentifier::new_random();
            let job_name = job_id.short().to_string();
            let job = Job {
                long_id: job_id.to_uuid(),
                name: job_name.clone(),
                kube_name: job_name,
                command_args: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "apt-get update; apt-get install -y netcat; echo listening on port $PORT; env; test -f $APP_CONFIG; timeout 15 nc -l 8080; exit 0;"
                        .to_string(),
                ],
                entrypoint: None,
                force_trigger: true,
                cpu_request_in_milli: 250,
                cpu_limit_in_milli: 250,
                ram_request_in_mib: 250,
                ram_limit_in_mib: 250,
                action: Action::Create,
                schedule: JobSchedule::Cron {
                    schedule: "*/30 * * * *".to_string(), // <- every 30 minutes
                    timezone: "Etc/UTC".to_string(),
                },
                source: JobSource::Image {
                registry: Registry::PublicEcr {
                        long_id: Uuid::new_v4(),
                        url: Url::parse("https://public.ecr.aws").unwrap(),
                    },
                    image: "r3m4q3r9/pub-mirror-debian".to_string(),
                    tag: "11.6-ci".to_string(),
                },
                max_nb_restart: 1,
                max_duration_in_sec: 120,
                environment_vars_with_infos: BTreeMap::default(),
                advanced_settings: Default::default(),
                mounted_files: vec![],
                default_port: None,
                readiness_probe: None,
                liveness_probe: None,
                container_registries: ContainerRegistries { registries: vec![] },
                annotations_group_ids: btreeset! {},
                labels_group_ids: btreeset! {},
                should_delete_shared_registry: false,
                shared_image_feature_enabled: false,
            };
            environment.jobs = vec![job];
        }
    };

    (infra_ctx, environment)
}
