extern crate serde;
extern crate serde_derive;

use chrono::Utc;
use std::cell::RefCell;

use qovery_engine::cloud_provider::utilities::sanitize_name;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::io_models::{
    Action, Application, CloneForTest, Context, Database, DatabaseKind, DatabaseMode, EnvironmentRequest,
    GitCredentials, Port, Protocol, Route, Router, Storage, StorageType,
};

use crate::aws::{AWS_KUBERNETES_VERSION, AWS_TEST_REGION};
use crate::aws_ec2::ec2_kubernetes_instance;
use crate::digitalocean::{DO_KUBERNETES_VERSION, DO_TEST_REGION};
use crate::scaleway::{SCW_KUBERNETES_VERSION, SCW_TEST_ZONE};
use crate::utilities::{
    db_disk_type, db_infos, db_instance_type, generate_id, generate_password, get_pvc, get_svc, get_svc_name, init,
    FuncTestsSecrets,
};
use base64;
use qovery_engine::cloud_provider::aws::kubernetes::ec2::EC2;
use qovery_engine::cloud_provider::aws::kubernetes::eks::EKS;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::environment::Environment;
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::cloud_provider::kubernetes::Kubernetes;
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::{CloudProvider, Kind};
use qovery_engine::cmd::structs::SVCItem;
use qovery_engine::engine::EngineConfig;
use qovery_engine::io_models::DatabaseMode::CONTAINER;
use qovery_engine::logger::Logger;
use qovery_engine::models::digital_ocean::DoRegion;
use qovery_engine::models::scaleway::ScwZone;
use qovery_engine::transaction::{DeploymentOption, Transaction, TransactionResult};
use qovery_engine::utilities::to_short_id;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{span, Level};
use url::Url;
use uuid::Uuid;

pub const KUBERNETES_MIN_NODES: i32 = 5;
pub const KUBERNETES_MAX_NODES: i32 = 10;

pub enum RegionActivationStatus {
    Deactivated,
    Activated,
}

#[derive(Clone)]
pub enum ClusterDomain {
    Default { cluster_id: String },
    QoveryOwnedDomain { cluster_id: String, domain: String },
    Custom { domain: String },
}

pub trait Cluster<T, U> {
    fn docker_cr_engine(
        context: &Context,
        logger: Box<dyn Logger>,
        localisation: &str,
        kubernetes_kind: KubernetesKind,
        kubernetes_version: String,
        cluster_domain: &ClusterDomain,
        vpc_network_mode: Option<VpcQoveryNetworkMode>,
        min_nodes: i32,
        max_nodes: i32,
        engine_location: EngineLocation,
    ) -> EngineConfig;
    fn cloud_provider(context: &Context, kubernetes_kind: KubernetesKind) -> Box<T>;
    fn kubernetes_nodes(min_nodes: i32, max_nodes: i32) -> Vec<NodeGroups>;
    fn kubernetes_cluster_options(
        secrets: FuncTestsSecrets,
        cluster_id: Option<String>,
        engine_location: EngineLocation,
    ) -> U;
}

pub trait Infrastructure {
    fn build_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> (Environment, TransactionResult);

    fn deploy_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult;

    fn pause_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult;

    fn delete_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult;
}

impl Infrastructure for EnvironmentRequest {
    fn build_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> (Environment, TransactionResult) {
        let mut tx = Transaction::new(engine_config, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
        let env = environment
            .to_environment_domain(
                engine_config.context(),
                engine_config.cloud_provider(),
                engine_config.container_registry().registry_info(),
                logger,
            )
            .unwrap();

        let env = Rc::new(RefCell::new(env));
        let _ = tx.build_environment(
            &env,
            DeploymentOption {
                force_build: true,
                force_push: true,
            },
        );

        let ret = tx.commit();
        (Rc::try_unwrap(env).ok().unwrap().into_inner(), ret)
    }

    fn deploy_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult {
        let mut tx = Transaction::new(engine_config, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
        let env = environment
            .to_environment_domain(
                engine_config.context(),
                engine_config.cloud_provider(),
                engine_config.container_registry().registry_info(),
                logger,
            )
            .unwrap();

        let env = Rc::new(RefCell::new(env));
        let _ = tx.deploy_environment_with_options(
            &env,
            DeploymentOption {
                force_build: true,
                force_push: true,
            },
        );

        tx.commit()
    }

    fn pause_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult {
        let mut tx = Transaction::new(engine_config, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
        let env = environment
            .to_environment_domain(
                engine_config.context(),
                engine_config.cloud_provider(),
                engine_config.container_registry().registry_info(),
                logger,
            )
            .unwrap();
        let env = Rc::new(RefCell::new(env));
        let _ = tx.pause_environment(&env);

        tx.commit()
    }

    fn delete_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult {
        let mut tx = Transaction::new(engine_config, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
        let env = environment
            .to_environment_domain(
                engine_config.context(),
                engine_config.cloud_provider(),
                engine_config.container_registry().registry_info(),
                logger,
            )
            .unwrap();
        let env = Rc::new(RefCell::new(env));
        let _ = tx.delete_environment(&env);

        tx.commit()
    }
}

pub enum ClusterTestType {
    Classic,
    WithPause,
    WithUpgrade,
    WithNodesResize,
}

pub fn environment_3_apps_3_routers_3_databases(
    context: &Context,
    test_domain: &str,
    database_instance_type: &str,
    database_disk_type: &str,
    provider_kind: Kind,
) -> EnvironmentRequest {
    let app_name_1 = format!("{}-{}", "simple-app-1", generate_id());
    let app_name_2 = format!("{}-{}", "simple-app-2", generate_id());
    let app_name_3 = format!("{}-{}", "simple-app-3", generate_id());

    // mongoDB management part
    let database_host_mongo = get_svc_name(DatabaseKind::Mongodb, provider_kind.clone()).to_string();
    let database_port_mongo = 27017;
    let database_db_name_mongo = "my-mongodb".to_string();
    let database_username_mongo = "superuser".to_string();
    let database_password_mongo = generate_password(provider_kind.clone(), CONTAINER);
    let database_uri_mongo = format!(
        "mongodb://{}:{}@{}:{}/{}",
        database_username_mongo,
        database_password_mongo,
        database_host_mongo,
        database_port_mongo,
        database_db_name_mongo
    );
    let version_mongo = "4.4";

    // pSQL 1 management part
    let fqdn = get_svc_name(DatabaseKind::Postgresql, provider_kind.clone()).to_string();
    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_password(provider_kind.clone(), CONTAINER);
    let database_name = "postgres".to_string();

    // pSQL 2 management part
    let fqdn_2 = format!("{}2", get_svc_name(DatabaseKind::Postgresql, provider_kind));
    let database_username_2 = "superuser2".to_string();
    let database_name_2 = "postgres2".to_string();

    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: Uuid::new_v4(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![
            Application {
                long_id: Uuid::new_v4(),
                name: app_name_1.clone(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "5990752647af11ef21c3d46a51abbde3da1ab351".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                buildpack_language: None,
                root_path: "/".to_string(),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
                storage: vec![Storage {
                    id: generate_id(),
                    long_id: Uuid::new_v4(),
                    name: "photos".to_string(),
                    storage_type: StorageType::Ssd,
                    size_in_gib: 10,
                    mount_point: "/mnt/photos".to_string(),
                    snapshot_retention_in_days: 0,
                }],
                environment_vars: btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_name.clone()),
                     "PG_HOST".to_string() => base64::encode(fqdn.clone()),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username.clone()),
                     "PG_PASSWORD".to_string() => base64::encode(database_password.clone()),
                },
                branch: "master".to_string(),
                ports: vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 1234,
                    public_port: Some(443),
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }],
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                min_instances: 2,
                max_instances: 2,
                cpu_burst: "100m".to_string(),
                advanced_settings: Default::default(),
            },
            Application {
                long_id: Uuid::new_v4(),
                name: app_name_2.clone(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "5990752647af11ef21c3d46a51abbde3da1ab351".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                buildpack_language: None,
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
                storage: vec![],
                environment_vars: btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_name_2.clone()),
                     "PG_HOST".to_string() => base64::encode(fqdn_2.clone()),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username_2.clone()),
                     "PG_PASSWORD".to_string() => base64::encode(database_password.clone()),
                },
                branch: "master".to_string(),
                ports: vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 1234,
                    public_port: Some(443),
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }],
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                min_instances: 2,
                max_instances: 2,
                cpu_burst: "100m".to_string(),
                advanced_settings: Default::default(),
            },
            Application {
                long_id: Uuid::new_v4(),
                name: app_name_3.clone(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "158ea8ebc9897c50a7c56b910db33ce837ac1e61".to_string(),
                dockerfile_path: Some(format!("Dockerfile-{}", version_mongo)),
                buildpack_language: None,
                action: Action::Create,
                root_path: String::from("/"),
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
                storage: vec![],
                environment_vars: btreemap! {
                    "IS_DOCUMENTDB".to_string() => base64::encode("false"),
                    "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string() => base64::encode(database_host_mongo.clone()),
                    "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string() => base64::encode(database_uri_mongo),
                    "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string() => base64::encode(database_port_mongo.to_string()),
                    "MONGODB_DBNAME".to_string() => base64::encode(&database_db_name_mongo),
                    "QOVERY_DATABASE_TESTING_DATABASE_USERNAME".to_string() => base64::encode(database_username_mongo.clone()),
                    "QOVERY_DATABASE_TESTING_DATABASE_PASSWORD".to_string() => base64::encode(database_password_mongo.clone()),
                },
                branch: "master".to_string(),
                ports: vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 1234,
                    public_port: Some(443),
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }],
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                min_instances: 2,
                max_instances: 2,
                cpu_burst: "100m".to_string(),
                advanced_settings: Default::default(),
            },
        ],
        routers: vec![
            Router {
                long_id: Uuid::new_v4(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app1".to_string(),
                    application_name: app_name_1,
                }],
                sticky_sessions_enabled: false,
            },
            Router {
                long_id: Uuid::new_v4(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app2".to_string(),
                    application_name: app_name_2,
                }],
                sticky_sessions_enabled: false,
            },
            Router {
                long_id: Uuid::new_v4(),
                name: "third-router".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app3".to_string(),
                    application_name: app_name_3,
                }],
                sticky_sessions_enabled: false,
            },
        ],
        databases: vec![
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                long_id: Uuid::new_v4(),
                name: database_name,
                version: "11.8.0".to_string(),
                fqdn_id: fqdn.clone(),
                fqdn,
                port: database_port,
                username: database_username,
                password: database_password.clone(),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: database_instance_type.to_string(),
                database_disk_type: database_disk_type.to_string(),
                encrypt_disk: true,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
                mode: CONTAINER,
            },
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                long_id: Uuid::new_v4(),
                name: database_name_2,
                version: "11.8.0".to_string(),
                fqdn_id: fqdn_2.clone(),
                fqdn: fqdn_2,
                port: database_port,
                username: database_username_2,
                password: database_password,
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: database_instance_type.to_string(),
                database_disk_type: database_disk_type.to_string(),
                encrypt_disk: true,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
                mode: CONTAINER,
            },
            Database {
                kind: DatabaseKind::Mongodb,
                action: Action::Create,
                long_id: Uuid::new_v4(),
                name: database_db_name_mongo,
                version: version_mongo.to_string(),
                fqdn_id: database_host_mongo.clone(),
                fqdn: database_host_mongo,
                port: database_port_mongo,
                username: database_username_mongo,
                password: database_password_mongo,
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: database_instance_type.to_string(),
                database_disk_type: database_disk_type.to_string(),
                encrypt_disk: true,
                activate_high_availability: false,
                activate_backups: false,
                publicly_accessible: false,
                mode: CONTAINER,
            },
        ],
        clone_from_environment_id: None,
    }
}

pub fn working_minimal_environment(context: &Context, test_domain: &str) -> EnvironmentRequest {
    let application_id = Uuid::new_v4();
    let application_name = to_short_id(&application_id);
    let router_name = "main".to_string();
    let application_domain = format!("{}.{}.{}", application_name, context.cluster_id(), test_domain);
    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: application_id,
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: application_id,
            name: application_name.to_string(),
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: BTreeMap::default(),
            branch: "basic-app-deploy".to_string(),
            ports: vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 80,
                public_port: Some(443),
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 2,
            max_instances: 2,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        routers: vec![Router {
            long_id: Uuid::new_v4(),
            name: router_name,
            action: Action::Create,
            default_domain: application_domain,
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name,
            }],
            sticky_sessions_enabled: false,
        }],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

pub fn database_test_environment(context: &Context) -> EnvironmentRequest {
    let suffix = generate_id();
    let application_name = format!("{}-{}", "simple-app", &suffix);

    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: Uuid::new_v4(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: Uuid::new_v4(),
            name: application_name,
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: BTreeMap::default(),
            branch: "basic-app-deploy".to_string(),
            ports: vec![],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 1,
            max_instances: 1,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        routers: vec![],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

pub fn database_test_environment_on_upgrade(context: &Context) -> EnvironmentRequest {
    let suffix = Uuid::new_v4();
    let application_name = format!("{}-{}", "simple-app", to_short_id(&suffix));

    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: suffix,
        project_long_id: suffix,
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: Uuid::from_str("9d0158db-b783-4bc2-a23b-c7d9228cbe90").unwrap(),
            name: application_name,
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: BTreeMap::default(),
            branch: "basic-app-deploy".to_string(),
            ports: vec![],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 1,
            max_instances: 1,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        routers: vec![],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

pub fn environment_only_http_server_router_with_sticky_session(
    context: &Context,
    test_domain: &str,
) -> EnvironmentRequest {
    let mut env = environment_only_http_server_router(context, test_domain);

    for mut router in &mut env.routers {
        router.sticky_sessions_enabled = true;
    }

    env.clone()
}

pub fn environnement_2_app_2_routers_1_psql(
    context: &Context,
    test_domain: &str,
    database_instance_type: &str,
    database_disk_type: &str,
    provider_kind: Kind,
) -> EnvironmentRequest {
    let fqdn = get_svc_name(DatabaseKind::Postgresql, provider_kind.clone()).to_string();

    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_password(provider_kind, CONTAINER);
    let database_name = "postgres".to_string();

    let suffix = generate_id();
    let application_name1 = sanitize_name("postgresql", &format!("{}-{}", "postgresql-app1", &suffix));
    let application_name2 = sanitize_name("postgresql", &format!("{}-{}", "postgresql-app2", &suffix));

    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: Uuid::new_v4(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        databases: vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            long_id: Uuid::new_v4(),
            name: database_name.clone(),
            version: "11.8.0".to_string(),
            fqdn_id: fqdn.clone(),
            fqdn: fqdn.clone(),
            port: database_port,
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
            database_instance_type: database_instance_type.to_string(),
            database_disk_type: database_disk_type.to_string(),
            encrypt_disk: true,
            activate_high_availability: false,
            activate_backups: false,
            publicly_accessible: false,
            mode: CONTAINER,
        }],
        applications: vec![
            Application {
                long_id: Uuid::new_v4(),
                name: application_name1.to_string(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "680550d1937b3f90551849c0da8f77c39916913b".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                buildpack_language: None,
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
                storage: vec![],
                environment_vars: btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_name.clone()),
                     "PG_HOST".to_string() => base64::encode(fqdn.clone()),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username.clone()),
                     "PG_PASSWORD".to_string() => base64::encode(database_password.clone()),
                },
                branch: "master".to_string(),
                ports: vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 1234,
                    public_port: Some(443),
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }],
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                min_instances: 2,
                max_instances: 2,
                cpu_burst: "100m".to_string(),
                advanced_settings: Default::default(),
            },
            Application {
                long_id: Uuid::new_v4(),
                name: application_name2.to_string(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "680550d1937b3f90551849c0da8f77c39916913b".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                buildpack_language: None,
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
                storage: vec![],
                environment_vars: btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_name),
                     "PG_HOST".to_string() => base64::encode(fqdn),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username),
                     "PG_PASSWORD".to_string() => base64::encode(database_password),
                },
                branch: "master".to_string(),
                ports: vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 1234,
                    public_port: Some(443),
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }],
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                min_instances: 2,
                max_instances: 2,
                cpu_burst: "100m".to_string(),
                advanced_settings: Default::default(),
            },
        ],
        routers: vec![
            Router {
                long_id: Uuid::new_v4(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/".to_string(),
                    application_name: application_name1,
                }],
                sticky_sessions_enabled: false,
            },
            Router {
                long_id: Uuid::new_v4(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/coco".to_string(),
                    application_name: application_name2,
                }],
                sticky_sessions_enabled: false,
            },
        ],
        clone_from_environment_id: None,
    }
}

pub fn non_working_environment(context: &Context, test_domain: &str) -> EnvironmentRequest {
    let mut environment = working_minimal_environment(context, test_domain);

    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
            app.branch = "bugged-image".to_string();
            app.commit_id = "c2b2d7b5d96832732df25fe992721f53842b5eac".to_string();
            app
        })
        .collect::<Vec<_>>();

    environment
}

// echo app environment is an environment that contains http-echo container (forked from hashicorp)
// ECHO_TEXT var will be the content of the application root path
pub fn echo_app_environment(context: &Context, test_domain: &str) -> EnvironmentRequest {
    let suffix = generate_id();
    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: Uuid::new_v4(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: Uuid::new_v4(),
            name: format!("{}-{}", "echo-app", &suffix),
            /*name: "simple-app".to_string(),*/
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "2205adea1db295547b99f7b17229afd7e879b6ff".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: btreemap! {
                "ECHO_TEXT".to_string() => base64::encode("42"),
            },
            branch: "echo-app".to_string(),
            ports: vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 5678,
                public_port: Some(443),
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 2,
            max_instances: 2,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        routers: vec![Router {
            long_id: Uuid::new_v4(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name: format!("{}-{}", "echo-app", &suffix),
            }],
            sticky_sessions_enabled: false,
        }],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

pub fn environment_only_http_server(context: &Context) -> EnvironmentRequest {
    let suffix = generate_id();
    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: Uuid::new_v4(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: Uuid::new_v4(),
            name: format!("{}-{}", "mini-http", &suffix),
            /*name: "simple-app".to_string(),*/
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "a873edd459c97beb51453db056c40bca85f36ef9".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: BTreeMap::default(),
            branch: "mini-http".to_string(),
            ports: vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 3000,
                public_port: Some(443),
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 2,
            max_instances: 2,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        routers: vec![],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

pub fn environment_only_http_server_router(context: &Context, test_domain: &str) -> EnvironmentRequest {
    let suffix = generate_id();
    let id = Uuid::new_v4();
    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: Uuid::new_v4(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: id,
            name: format!("{}-{}", "mini-http", &suffix),
            /*name: "simple-app".to_string(),*/
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "a873edd459c97beb51453db056c40bca85f36ef9".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: BTreeMap::default(),
            branch: "mini-http".to_string(),
            ports: vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 3000,
                public_port: Some(443),
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 2,
            max_instances: 2,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        routers: vec![Router {
            long_id: Uuid::new_v4(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name: format!("{}-{}", "mini-http", &suffix),
            }],
            sticky_sessions_enabled: false,
        }],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

/// Test if stick sessions are activated on given routers via cookie.
pub fn session_is_sticky(url: Url, host: String, max_age: u32) -> bool {
    let mut is_ok = true;
    let http_client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true) // this test ignores certificate validity (not its purpose)
        .build()
        .expect("Cannot build reqwest client");

    let http_request_result = http_client.get(url.to_string()).header("Host", host.as_str()).send();

    if http_request_result.is_err() {
        return false;
    }

    let http_response = http_request_result.expect("cannot retrieve HTTP request result");

    is_ok &= match http_response.headers().get("Set-Cookie") {
        None => false,
        Some(value) => match value.to_str() {
            Err(_) => false,
            Ok(s) => s.contains("INGRESSCOOKIE_QOVERY=") && s.contains(format!("Max-Age={}", max_age).as_str()),
        },
    };

    is_ok
}

fn compute_test_cluster_endpoint(cluster_domain: &ClusterDomain, default_domain: String) -> String {
    match cluster_domain {
        ClusterDomain::Default { cluster_id } => format!("{}.{}", cluster_id, default_domain),
        ClusterDomain::QoveryOwnedDomain { cluster_id, domain } => format!("{}.{}", cluster_id, domain),
        ClusterDomain::Custom { domain } => domain.to_string(),
    }
}

pub fn test_db(
    context: Context,
    logger: Box<dyn Logger>,
    mut environment: EnvironmentRequest,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    db_kind: DatabaseKind,
    kubernetes_kind: KubernetesKind,
    database_mode: DatabaseMode,
    is_public: bool,
    cluster_domain: ClusterDomain,
    existing_engine_config: Option<&EngineConfig>,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();
    let context_for_delete = context.clone_not_same_execution_id();

    let provider_kind = kubernetes_kind.get_cloud_provider_kind();
    let app_id = Uuid::new_v4();
    let database_username = "superuser".to_string();
    let database_password = generate_password(provider_kind.clone(), database_mode.clone());
    let db_kind_str = db_kind.name().to_string();
    let db_id = generate_id();
    let database_host = format!("{}-{}", db_id, db_kind_str);
    let database_fqdn = format!(
        "{}.{}",
        database_host,
        compute_test_cluster_endpoint(
            &cluster_domain,
            match kubernetes_kind {
                KubernetesKind::Ec2 => secrets
                    .AWS_EC2_TEST_CLUSTER_DOMAIN
                    .as_ref()
                    .expect("AWS_EC2_TEST_CLUSTER_DOMAIN must be set")
                    .to_string(),
                _ => secrets
                    .DEFAULT_TEST_DOMAIN
                    .as_ref()
                    .expect("DEFAULT_TEST_DOMAIN must be set")
                    .to_string(),
            }
        )
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
        long_id: Uuid::new_v4(),
        name: db_id,
        version: version.to_string(),
        fqdn_id: database_host.clone(),
        fqdn: database_fqdn.clone(),
        port: database_port,
        username: database_username,
        password: database_password,
        total_cpus: "250m".to_string(),
        total_ram_in_mib: 512, // MySQL requires at least 512Mo in order to boot
        disk_size_in_gib: storage_size,
        database_instance_type: db_instance_type,
        database_disk_type: db_disk_type,
        encrypt_disk: true,
        activate_high_availability: false,
        activate_backups: false,
        publicly_accessible: is_public,
        mode: database_mode.clone(),
    };

    environment.databases = vec![db];

    let app_name = format!("{}-app-{}", db_kind_str, generate_id());
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.long_id = app_id;
            app.name = to_short_id(&app_id);
            app.branch = app_name.clone();
            app.commit_id = db_infos.app_commit.clone();
            app.ports = vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 1234,
                public_port: Some(1234),
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }];
            app.dockerfile_path = Some(format!("Dockerfile-{}", version));
            app.environment_vars = db_infos.app_env_vars.clone();
            app
        })
        .collect::<Vec<Application>>();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = environment.clone();
    let ea_delete = environment_delete.clone();

    let (localisation, kubernetes_version) = match provider_kind {
        Kind::Aws => (AWS_TEST_REGION.to_string(), AWS_KUBERNETES_VERSION.to_string()),
        Kind::Do => (DO_TEST_REGION.to_string(), DO_KUBERNETES_VERSION.to_string()),
        Kind::Scw => (SCW_TEST_ZONE.to_string(), SCW_KUBERNETES_VERSION.to_string()),
    };

    let computed_engine_config: EngineConfig;
    let engine_config = match existing_engine_config {
        Some(c) => c,
        None => {
            computed_engine_config = match kubernetes_kind {
                KubernetesKind::Eks => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::Eks,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
                KubernetesKind::Ec2 => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::Ec2,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    1,
                    1,
                    EngineLocation::QoverySide, // EC2 is not meant to run Engine
                ),
                KubernetesKind::Doks => DO::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::Doks,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
                KubernetesKind::ScwKapsule => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
            };
            &computed_engine_config
        }
    };

    let ret = environment.deploy_environment(&ea, logger.clone(), engine_config);
    assert!(matches!(ret, TransactionResult::Ok));

    match database_mode {
        CONTAINER => {
            match get_pvc(context.clone(), provider_kind.clone(), environment.clone(), secrets.clone()) {
                Ok(pvc) => assert_eq!(
                    pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                    format!("{}Gi", storage_size)
                ),
                Err(_) => panic!(),
            };

            match get_svc(context.clone(), provider_kind.clone(), environment, secrets) {
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
                Err(_) => panic!(),
            };
        }
        DatabaseMode::MANAGED => {
            match get_svc(context.clone(), provider_kind.clone(), environment, secrets) {
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
                Err(_) => panic!(),
            };
        }
    }

    let computed_engine_config_for_delete: EngineConfig;
    let engine_config_for_delete = match existing_engine_config {
        Some(c) => c,
        None => {
            computed_engine_config_for_delete = match kubernetes_kind {
                KubernetesKind::Eks => AWS::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::Eks,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
                KubernetesKind::Ec2 => AWS::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::Ec2,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    1,
                    1,
                    EngineLocation::QoverySide, // EC2 is not meant to run Engine
                ),
                KubernetesKind::Doks => DO::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::Doks,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
                KubernetesKind::ScwKapsule => Scaleway::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version.clone(),
                    &cluster_domain,
                    None,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
            };
            &computed_engine_config_for_delete
        }
    };

    let ret = environment_delete.delete_environment(&ea_delete, logger, &engine_config_for_delete);
    assert!(matches!(ret, TransactionResult::Ok));

    test_name.to_string()
}

pub fn get_environment_test_kubernetes(
    context: &Context,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    kubernetes_version: &str,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    logger: Box<dyn Logger>,
    localisation: &str,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    min_nodes: i32,
    max_nodes: i32,
    engine_location: EngineLocation,
) -> Box<dyn Kubernetes> {
    let secrets = FuncTestsSecrets::new();

    let kubernetes: Box<dyn Kubernetes> = match cloud_provider.kubernetes_kind() {
        KubernetesKind::Eks => {
            let region = AwsRegion::from_str(localisation).expect("AWS region not supported");
            let mut options = AWS::kubernetes_cluster_options(secrets, None, engine_location);
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }

            Box::new(
                EKS::new(
                    context.clone(),
                    context.cluster_id(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_id()).as_str(),
                    kubernetes_version,
                    region.clone(),
                    region.get_zones_to_string(),
                    cloud_provider,
                    dns_provider,
                    options,
                    AWS::kubernetes_nodes(min_nodes, max_nodes),
                    logger,
                )
                .unwrap(),
            )
        }
        KubernetesKind::Ec2 => {
            let region = AwsRegion::from_str(localisation).expect("AWS region not supported");
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::QoverySide);
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }

            Box::new(
                EC2::new(
                    context.clone(),
                    context.cluster_id(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_id()).as_str(),
                    kubernetes_version,
                    region.clone(),
                    region.get_zones_to_string(),
                    cloud_provider,
                    dns_provider,
                    options,
                    ec2_kubernetes_instance(),
                    logger,
                )
                .unwrap(),
            )
        }
        KubernetesKind::Doks => {
            let region = DoRegion::from_str(localisation).expect("DO region not supported");
            Box::new(
                DOKS::new(
                    context.clone(),
                    context.cluster_id().to_string(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_id()),
                    kubernetes_version.to_string(),
                    region,
                    cloud_provider,
                    dns_provider,
                    DO::kubernetes_nodes(min_nodes, max_nodes),
                    DO::kubernetes_cluster_options(
                        secrets,
                        Option::from(context.cluster_id().to_string()),
                        EngineLocation::ClientSide,
                    ),
                    logger,
                )
                .unwrap(),
            )
        }
        KubernetesKind::ScwKapsule => {
            let zone = ScwZone::from_str(localisation).expect("SCW zone not supported");
            Box::new(
                Kapsule::new(
                    context.clone(),
                    context.cluster_id().to_string(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_id()),
                    kubernetes_version.to_string(),
                    zone,
                    cloud_provider,
                    dns_provider,
                    Scaleway::kubernetes_nodes(min_nodes, max_nodes),
                    Scaleway::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide),
                    logger,
                )
                .unwrap(),
            )
        }
    };

    kubernetes
}

pub fn get_cluster_test_kubernetes<'a>(
    secrets: FuncTestsSecrets,
    context: &Context,
    cluster_id: String,
    cluster_name: String,
    boot_version: String,
    localisation: &str,
    aws_zones: Option<Vec<AwsZones>>,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    kubernetes_provider: KubernetesKind,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    logger: Box<dyn Logger>,
    min_nodes: i32,
    max_nodes: i32,
) -> Box<dyn Kubernetes + 'a> {
    let kubernetes: Box<dyn Kubernetes> = match kubernetes_provider {
        KubernetesKind::Eks => {
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide);
            let aws_region = AwsRegion::from_str(localisation).expect("expected correct AWS region");
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }
            let aws_zones = aws_zones.unwrap().into_iter().map(|zone| zone.to_string()).collect();

            Box::new(
                EKS::new(
                    context.clone(),
                    cluster_id.as_str(),
                    Uuid::new_v4(),
                    cluster_name.as_str(),
                    boot_version.as_str(),
                    aws_region,
                    aws_zones,
                    cloud_provider,
                    dns_provider,
                    options,
                    AWS::kubernetes_nodes(min_nodes, max_nodes),
                    logger,
                )
                .unwrap(),
            )
        }
        KubernetesKind::Ec2 => {
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::QoverySide);
            let aws_region = AwsRegion::from_str(localisation).expect("expected correct AWS region");
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }
            let aws_zones = aws_zones.unwrap().into_iter().map(|zone| zone.to_string()).collect();

            Box::new(
                EC2::new(
                    context.clone(),
                    cluster_id.as_str(),
                    Uuid::new_v4(),
                    cluster_name.as_str(),
                    boot_version.as_str(),
                    aws_region,
                    aws_zones,
                    cloud_provider,
                    dns_provider,
                    options,
                    ec2_kubernetes_instance(),
                    logger,
                )
                .unwrap(),
            )
        }
        KubernetesKind::Doks => Box::new(
            DOKS::new(
                context.clone(),
                cluster_id,
                Uuid::new_v4(),
                cluster_name.clone(),
                boot_version,
                DoRegion::from_str(localisation).expect("Unknown region set for DOKS"),
                cloud_provider,
                dns_provider,
                DO::kubernetes_nodes(min_nodes, max_nodes),
                DO::kubernetes_cluster_options(secrets, Option::from(cluster_name), EngineLocation::ClientSide),
                logger,
            )
            .unwrap(),
        ),
        KubernetesKind::ScwKapsule => Box::new(
            Kapsule::new(
                context.clone(),
                cluster_id,
                Uuid::new_v4(),
                cluster_name,
                boot_version,
                ScwZone::from_str(localisation).expect("Unknown zone set for Kapsule"),
                cloud_provider,
                dns_provider,
                Scaleway::kubernetes_nodes(min_nodes, max_nodes),
                Scaleway::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide),
                logger,
            )
            .unwrap(),
        ),
    };

    kubernetes
}

pub fn cluster_test(
    test_name: &str,
    provider_kind: Kind,
    kubernetes_kind: KubernetesKind,
    context: Context,
    logger: Box<dyn Logger>,
    localisation: &str,
    aws_zones: Option<Vec<AwsZones>>,
    test_type: ClusterTestType,
    major_boot_version: u8,
    minor_boot_version: u8,
    cluster_domain: &ClusterDomain,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    environment_to_deploy: Option<&EnvironmentRequest>,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();
    let boot_version = format!("{}.{}", major_boot_version, minor_boot_version.clone());

    let engine = match provider_kind {
        Kind::Aws => AWS::docker_cr_engine(
            &context,
            logger.clone(),
            localisation,
            kubernetes_kind,
            boot_version,
            cluster_domain,
            vpc_network_mode.clone(),
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Do => DO::docker_cr_engine(
            &context,
            logger.clone(),
            localisation,
            kubernetes_kind,
            boot_version,
            cluster_domain,
            vpc_network_mode.clone(),
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context,
            logger.clone(),
            localisation,
            kubernetes_kind,
            boot_version,
            cluster_domain,
            vpc_network_mode.clone(),
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
    };
    let mut deploy_tx = Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
    let mut delete_tx = Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();

    let mut aws_zones_string: Vec<String> = Vec::with_capacity(3);
    if let Some(aws_zones) = aws_zones {
        for zone in aws_zones {
            aws_zones_string.push(zone.to_string())
        }
    };

    // Deploy
    if let Err(err) = deploy_tx.create_kubernetes() {
        panic!("{:?}", err)
    }
    assert!(matches!(deploy_tx.commit(), TransactionResult::Ok));

    // Deploy env if any
    if let Some(env) = environment_to_deploy {
        let mut deploy_env_tx =
            Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();

        // Deploy env
        let env = env
            .to_environment_domain(
                &context,
                engine.cloud_provider(),
                engine.container_registry().registry_info(),
                logger.clone(),
            )
            .unwrap();
        let env = Rc::new(RefCell::new(env));
        if let Err(err) = deploy_env_tx.deploy_environment(&env) {
            panic!("{:?}", err)
        }

        assert!(matches!(deploy_env_tx.commit(), TransactionResult::Ok));
    }

    match test_type {
        // TODO new test type
        ClusterTestType::Classic => {}
        ClusterTestType::WithPause => {
            let mut pause_tx = Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
            let mut resume_tx =
                Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();

            // Pause
            if let Err(err) = pause_tx.pause_kubernetes() {
                panic!("{:?}", err)
            }
            assert!(matches!(pause_tx.commit(), TransactionResult::Ok));

            // Resume
            if let Err(err) = resume_tx.create_kubernetes() {
                panic!("{:?}", err)
            }

            assert!(matches!(resume_tx.commit(), TransactionResult::Ok));
        }
        ClusterTestType::WithUpgrade => {
            let upgrade_to_version = format!("{}.{}", major_boot_version, (minor_boot_version + 1));
            let engine = match provider_kind {
                Kind::Aws => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::Eks,
                    upgrade_to_version,
                    cluster_domain,
                    vpc_network_mode,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
                Kind::Do => DO::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::Doks,
                    upgrade_to_version,
                    cluster_domain,
                    vpc_network_mode,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
                Kind::Scw => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::ScwKapsule,
                    upgrade_to_version,
                    cluster_domain,
                    vpc_network_mode,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
            };
            let mut upgrade_tx =
                Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
            let mut delete_tx =
                Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();

            // Upgrade
            if let Err(err) = upgrade_tx.create_kubernetes() {
                panic!("{:?}", err)
            }
            assert!(matches!(upgrade_tx.commit(), TransactionResult::Ok));

            // Delete
            if let Err(err) = delete_tx.delete_kubernetes() {
                panic!("{:?}", err)
            }
            assert!(matches!(delete_tx.commit(), TransactionResult::Ok));

            return test_name.to_string();
        }
        ClusterTestType::WithNodesResize => {
            let min_nodes = 11;
            let max_nodes = 15;
            let kubernetes_version = format!("{}.{}", major_boot_version, minor_boot_version.clone());
            let engine = match provider_kind {
                Kind::Aws => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::Eks,
                    kubernetes_version,
                    cluster_domain,
                    vpc_network_mode,
                    min_nodes,
                    max_nodes,
                    EngineLocation::ClientSide,
                ),
                Kind::Do => DO::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::Doks,
                    kubernetes_version,
                    cluster_domain,
                    vpc_network_mode,
                    min_nodes,
                    max_nodes,
                    EngineLocation::ClientSide,
                ),
                Kind::Scw => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::ScwKapsule,
                    kubernetes_version,
                    cluster_domain,
                    vpc_network_mode,
                    min_nodes,
                    max_nodes,
                    EngineLocation::ClientSide,
                ),
            };
            let mut upgrade_tx =
                Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
            let mut delete_tx =
                Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
            // Upgrade
            if let Err(err) = upgrade_tx.create_kubernetes() {
                panic!("{:?}", err)
            }
            assert!(matches!(upgrade_tx.commit(), TransactionResult::Ok));

            // Delete
            if let Err(err) = delete_tx.delete_kubernetes() {
                panic!("{:?}", err)
            }
            assert!(matches!(delete_tx.commit(), TransactionResult::Ok));
            return test_name.to_string();
        }
    }

    // Destroy env if any
    if let Some(env) = environment_to_deploy {
        let mut destroy_env_tx =
            Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();

        // Deploy env
        let env = env
            .to_environment_domain(
                &context,
                engine.cloud_provider(),
                engine.container_registry().registry_info(),
                logger.clone(),
            )
            .unwrap();
        let env = Rc::new(RefCell::new(env));
        if let Err(err) = destroy_env_tx.delete_environment(&env) {
            panic!("{:?}", err)
        }
        assert!(matches!(destroy_env_tx.commit(), TransactionResult::Ok));
    }

    // Delete
    if let Err(err) = delete_tx.delete_kubernetes() {
        panic!("{:?}", err)
    }
    assert!(matches!(delete_tx.commit(), TransactionResult::Ok));

    test_name.to_string()
}

pub fn test_db_on_upgrade(
    context: Context,
    logger: Box<dyn Logger>,
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
    let database_host = format!("{}-{}", db_id, db_kind_str);
    let database_fqdn = format!(
        "{}.{}.{}",
        database_host,
        context.cluster_id(),
        secrets
            .clone()
            .DEFAULT_TEST_DOMAIN
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
        name: db_id,
        version: version.to_string(),
        fqdn_id: database_host.clone(),
        fqdn: database_fqdn.clone(),
        port: database_port,
        username: database_username,
        password: database_password,
        total_cpus: "50m".to_string(),
        total_ram_in_mib: 256,
        disk_size_in_gib: storage_size,
        database_instance_type: db_instance_type,
        database_disk_type: db_disk_type,
        encrypt_disk: true,
        activate_high_availability: false,
        activate_backups: false,
        publicly_accessible: is_public,
        mode: database_mode.clone(),
    };

    environment.databases = vec![db];

    let app_name = format!("{}-app-{}", db_kind_str, generate_id());
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.long_id = app_id;
            app.name = to_short_id(&app_id);
            app.branch = app_name.clone();
            app.commit_id = db_infos.app_commit.clone();
            app.ports = vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 1234,
                public_port: Some(1234),
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }];
            app.dockerfile_path = Some(format!("Dockerfile-{}", version));
            app.environment_vars = db_infos.app_env_vars.clone();
            app
        })
        .collect::<Vec<Application>>();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = environment.clone();
    let ea_delete = environment_delete.clone();

    let (localisation, kubernetes_version) = match provider_kind {
        Kind::Aws => (AWS_TEST_REGION.to_string(), AWS_KUBERNETES_VERSION.to_string()),
        Kind::Do => (DO_TEST_REGION.to_string(), DO_KUBERNETES_VERSION.to_string()),
        Kind::Scw => (SCW_TEST_ZONE.to_string(), SCW_KUBERNETES_VERSION.to_string()),
    };

    let engine_config = match provider_kind {
        Kind::Aws => AWS::docker_cr_engine(
            &context,
            logger.clone(),
            localisation.as_str(),
            KubernetesKind::Eks,
            kubernetes_version.clone(),
            &ClusterDomain::Default {
                cluster_id: context.cluster_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Do => DO::docker_cr_engine(
            &context,
            logger.clone(),
            localisation.as_str(),
            KubernetesKind::Doks,
            kubernetes_version.clone(),
            &ClusterDomain::Default {
                cluster_id: context.cluster_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context,
            logger.clone(),
            localisation.as_str(),
            KubernetesKind::ScwKapsule,
            kubernetes_version.clone(),
            &ClusterDomain::Default {
                cluster_id: context.cluster_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
    };

    let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
    assert!(matches!(ret, TransactionResult::Ok));

    match database_mode {
        CONTAINER => {
            match get_pvc(context.clone(), provider_kind.clone(), environment.clone(), secrets.clone()) {
                Ok(pvc) => assert_eq!(
                    pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                    format!("{}Gi", storage_size)
                ),
                Err(_) => panic!(),
            };

            match get_svc(context, provider_kind.clone(), environment, secrets) {
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
                Err(_) => panic!(),
            };
        }
        DatabaseMode::MANAGED => {
            match get_svc(context, provider_kind.clone(), environment, secrets) {
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
                Err(_) => panic!(),
            };
        }
    }

    let engine_config_for_delete = match provider_kind {
        Kind::Aws => AWS::docker_cr_engine(
            &context_for_delete,
            logger.clone(),
            localisation.as_str(),
            KubernetesKind::Eks,
            kubernetes_version,
            &ClusterDomain::Default {
                cluster_id: context_for_delete.cluster_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Do => DO::docker_cr_engine(
            &context_for_delete,
            logger.clone(),
            localisation.as_str(),
            KubernetesKind::Doks,
            kubernetes_version,
            &ClusterDomain::Default {
                cluster_id: context_for_delete.cluster_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context_for_delete,
            logger.clone(),
            localisation.as_str(),
            KubernetesKind::ScwKapsule,
            kubernetes_version,
            &ClusterDomain::Default {
                cluster_id: context_for_delete.cluster_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
    };

    let ret = environment_delete.delete_environment(&ea_delete, logger, &engine_config_for_delete);
    assert!(matches!(ret, TransactionResult::Ok));

    test_name.to_string()
}
