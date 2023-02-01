use crate::helpers::aws::aws_default_infra_config;
use crate::helpers::database::StorageSize::NormalSize;
use crate::helpers::utilities::{context_for_resource, generate_id, get_svc_name, logger, FuncTestsSecrets};
use chrono::Utc;
use qovery_engine::cloud_provider::Kind::Aws;
use qovery_engine::engine::InfrastructureContext;
use qovery_engine::io_models::application::{Application, Port, Protocol, Storage, StorageType};
use qovery_engine::io_models::container::{Container, Registry};
use qovery_engine::io_models::database::DatabaseMode::CONTAINER;
use qovery_engine::io_models::database::{Database, DatabaseKind};
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::{Action, QoveryIdentifier};
use qovery_engine::utilities::to_short_id;
use std::collections::BTreeMap;
use url::Url;
use uuid::Uuid;

mod application;
mod container;
mod database;

pub enum TestEnvOption {
    WithDB,
    WithContainer,
    WithApp,
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

    let logger = logger();
    let infra_ctx = aws_default_infra_config(&context, logger.clone());

    let mut environment = EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: QoveryIdentifier::new_random().to_uuid(),
        name: "env".to_string(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![],
        containers: vec![],
        jobs: vec![],
        routers: vec![],
        databases: vec![],
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
                created_at: Utc::now(),
                version: "13".to_string(),
                fqdn_id: database_host.clone(),
                fqdn: database_host,
                port: 5432,
                username: "superuser".to_string(),
                password: generate_id().to_string(),
                total_cpus: "250m".to_string(),
                total_ram_in_mib: 512, // MySQL requires at least 512Mo in order to boot
                disk_size_in_gib: NormalSize.size(),
                database_instance_type: "db.t2.micro".to_string(),
                database_disk_type: "gp2".to_string(),
                encrypt_disk: true,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
                mode: CONTAINER,
            };
            environment.databases = vec![db];
        }
        TestEnvOption::WithContainer => {
            let container_id = QoveryIdentifier::new_random();
            let container_name = container_id.short().to_string();
            let storage_1_id = QoveryIdentifier::new_random().to_uuid();
            let storage_2_id = QoveryIdentifier::new_random().to_uuid();
            let container = Container {
                long_id: container_id.to_uuid(),
                name: container_name,
                action: Action::Create,
                registry: Registry::DockerHub {
                    url: Url::parse("https://docker.io").unwrap(),
                    long_id: Uuid::new_v4(),
                    credentials: None,
                },
                image: "debian".to_string(),
                tag: "bullseye".to_string(),
                command_args: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "apt-get update; apt-get install -y netcat; echo listening on port $PORT; env ; while true; do nc -l 8080; done".to_string(),
                ],
                entrypoint: None,
                cpu_request_in_mili: 250,
                cpu_limit_in_mili: 250,
                ram_request_in_mib: 250,
                ram_limit_in_mib: 250,
                min_instances: 1,
                max_instances: 1,
                ports: vec![
                    Port {
                        long_id: Uuid::new_v4(),
                        id: Uuid::new_v4().to_string(),
                        port: 8080,
                        is_default: true,
                        name: Some("http".to_string()),
                        publicly_accessible: false,
                        protocol: Protocol::HTTP,
                    },
                ],
                storages: vec![
                    Storage {
                    id: to_short_id(&storage_1_id),
                    long_id: storage_1_id,
                    name: "photos1".to_string(),
                    storage_type: StorageType::Ssd,
                    size_in_gib: NormalSize.size(),
                    mount_point: "/mnt/photos1".to_string(),
                    snapshot_retention_in_days: 0,
                    },
                    Storage {
                        id: to_short_id(&storage_2_id),
                        long_id: storage_2_id,
                        name: "photos2".to_string(),
                        storage_type: StorageType::Ssd,
                        size_in_gib:  NormalSize.size(),
                        mount_point: "/mnt/photos2".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                ],
                environment_vars: BTreeMap::default(),
                advanced_settings: Default::default(),
                mounted_files: vec![],
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
                name: application_name,
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                command_args: vec![],
                entrypoint: None,
                buildpack_language: None,
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: None,
                storage: vec![
                    Storage {
                        id: to_short_id(&storage_1_id),
                        long_id: storage_1_id,
                        name: "photos1".to_string(),
                        storage_type: StorageType::Ssd,
                        size_in_gib: NormalSize.size(),
                        mount_point: "/mnt/photos1".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                    Storage {
                        id: to_short_id(&storage_2_id),
                        long_id: storage_2_id,
                        name: "photos2".to_string(),
                        storage_type: StorageType::Ssd,
                        size_in_gib: NormalSize.size(),
                        mount_point: "/mnt/photos2".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                ],
                environment_vars: BTreeMap::default(),
                branch: "basic-app-deploy".to_string(),
                ports: vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 80,
                    is_default: true,
                    name: None,
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                }],
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                min_instances: 1,
                max_instances: 1,
                cpu_burst: "100m".to_string(),
                advanced_settings: Default::default(),
                mounted_files: vec![],
            };
            environment.applications = vec![app];
        }
    };

    (infra_ctx, environment)
}
