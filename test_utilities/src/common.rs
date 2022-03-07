extern crate serde;
extern crate serde_derive;

use chrono::Utc;

use qovery_engine::cloud_provider::utilities::sanitize_name;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::models::{
    Action, Application, Clone2, Context, Database, DatabaseKind, DatabaseMode, Environment, EnvironmentAction,
    GitCredentials, Port, Protocol, Route, Router, Storage, StorageType,
};
use qovery_engine::transaction::TransactionResult;

use crate::aws::AWS_KUBERNETES_VERSION;
use crate::cloudflare::dns_provider_cloudflare;
use crate::digitalocean::DO_KUBERNETES_VERSION;
use crate::scaleway::SCW_KUBERNETES_VERSION;
use crate::utilities::{
    db_disk_type, db_infos, db_instance_type, generate_cluster_id, generate_id, generate_password, get_pvc, get_svc,
    get_svc_name, init, FuncTestsSecrets,
};
use base64;
use qovery_engine::cloud_provider::aws::kubernetes::{VpcQoveryNetworkMode, EKS};
use qovery_engine::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::digitalocean::application::DoRegion;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::kubernetes::Kubernetes;
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::scaleway::application::ScwZone;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::{CloudProvider, Kind};
use qovery_engine::cmd::kubectl::kubernetes_get_all_hpas;
use qovery_engine::cmd::structs::SVCItem;
use qovery_engine::engine::Engine;
use qovery_engine::errors::CommandError;
use qovery_engine::logger::Logger;
use qovery_engine::models::DatabaseMode::CONTAINER;
use qovery_engine::transaction::DeploymentOption;
use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;
use tracing::{span, Level};

pub enum RegionActivationStatus {
    Deactivated,
    Activated,
}

pub enum ClusterDomain {
    Default,
    Custom(String),
}

pub trait Cluster<T, U> {
    fn docker_cr_engine(context: &Context, logger: Box<dyn Logger>) -> Engine;
    fn cloud_provider(context: &Context) -> Box<T>;
    fn kubernetes_nodes() -> Vec<NodeGroups>;
    fn kubernetes_cluster_options(secrets: FuncTestsSecrets, cluster_id: Option<String>) -> U;
}

pub trait Infrastructure {
    fn deploy_environment(
        &self,
        provider_kind: Kind,
        context: &Context,
        environment_action: &EnvironmentAction,
        logger: Box<dyn Logger>,
    ) -> TransactionResult;
    fn pause_environment(
        &self,
        provider_kind: Kind,
        context: &Context,
        environment_action: &EnvironmentAction,
        logger: Box<dyn Logger>,
    ) -> TransactionResult;
    fn delete_environment(
        &self,
        provider_kind: Kind,
        context: &Context,
        environment_action: &EnvironmentAction,
        logger: Box<dyn Logger>,
    ) -> TransactionResult;
}

impl Infrastructure for Environment {
    fn deploy_environment(
        &self,
        provider_kind: Kind,
        context: &Context,
        environment_action: &EnvironmentAction,
        logger: Box<dyn Logger>,
    ) -> TransactionResult {
        let engine: Engine = match provider_kind {
            Kind::Aws => AWS::docker_cr_engine(context, logger.clone()),
            Kind::Do => DO::docker_cr_engine(context, logger.clone()),
            Kind::Scw => Scaleway::docker_cr_engine(context, logger.clone()),
        };
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let dns_provider = dns_provider_cloudflare(context, ClusterDomain::Default);
        let cp: Box<dyn CloudProvider>;
        cp = match provider_kind {
            Kind::Aws => AWS::cloud_provider(context),
            Kind::Do => DO::cloud_provider(context),
            Kind::Scw => Scaleway::cloud_provider(context),
        };
        let k;
        k = get_environment_test_kubernetes(provider_kind, context, cp.as_ref(), &dns_provider, logger.as_ref());

        let _ = tx.deploy_environment_with_options(
            k.as_ref(),
            &environment_action,
            DeploymentOption {
                force_build: true,
                force_push: true,
            },
        );

        tx.commit(logger.clone())
    }

    fn pause_environment(
        &self,
        provider_kind: Kind,
        context: &Context,
        environment_action: &EnvironmentAction,
        logger: Box<dyn Logger>,
    ) -> TransactionResult {
        let engine: Engine = match provider_kind {
            Kind::Aws => AWS::docker_cr_engine(context, logger.clone()),
            Kind::Do => DO::docker_cr_engine(context, logger.clone()),
            Kind::Scw => Scaleway::docker_cr_engine(context, logger.clone()),
        };

        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let dns_provider = dns_provider_cloudflare(context, ClusterDomain::Default);
        let cp: Box<dyn CloudProvider>;
        cp = match provider_kind {
            Kind::Aws => AWS::cloud_provider(context),
            Kind::Do => DO::cloud_provider(context),
            Kind::Scw => Scaleway::cloud_provider(context),
        };
        let k;
        k = get_environment_test_kubernetes(provider_kind, context, cp.as_ref(), &dns_provider, logger.as_ref());

        let _ = tx.pause_environment(k.as_ref(), &environment_action);

        tx.commit(logger.clone())
    }

    fn delete_environment(
        &self,
        provider_kind: Kind,
        context: &Context,
        environment_action: &EnvironmentAction,
        logger: Box<dyn Logger>,
    ) -> TransactionResult {
        let engine: Engine = match provider_kind {
            Kind::Aws => AWS::docker_cr_engine(context, logger.clone()),
            Kind::Do => DO::docker_cr_engine(context, logger.clone()),
            Kind::Scw => Scaleway::docker_cr_engine(context, logger.clone()),
        };

        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let dns_provider = dns_provider_cloudflare(context, ClusterDomain::Default);
        let cp: Box<dyn CloudProvider>;
        cp = match provider_kind {
            Kind::Aws => AWS::cloud_provider(context),
            Kind::Do => DO::cloud_provider(context),
            Kind::Scw => Scaleway::cloud_provider(context),
        };
        let k;
        k = get_environment_test_kubernetes(provider_kind, context, cp.as_ref(), &dns_provider, logger.as_ref());

        let _ = tx.delete_environment(k.as_ref(), &environment_action);

        tx.commit(logger.clone())
    }
}

pub enum ClusterTestType {
    Classic,
    WithPause,
    WithUpgrade,
}

pub fn environment_3_apps_3_routers_3_databases(
    context: &Context,
    test_domain: &str,
    database_instance_type: &str,
    database_disk_type: &str,
    provider_kind: Kind,
) -> Environment {
    let app_name_1 = format!("{}-{}", "simple-app-1".to_string(), generate_id());
    let app_name_2 = format!("{}-{}", "simple-app-2".to_string(), generate_id());
    let app_name_3 = format!("{}-{}", "simple-app-3".to_string(), generate_id());

    // mongoDB management part
    let database_host_mongo = get_svc_name(DatabaseKind::Mongodb, provider_kind.clone()).to_string();
    let database_port_mongo = 27017;
    let database_db_name_mongo = "my-mongodb".to_string();
    let database_username_mongo = "superuser".to_string();
    let database_password_mongo = generate_password(false);
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
    let database_password = generate_password(true);
    let database_name = "postgres".to_string();

    // pSQL 2 management part
    let fqdn_2 = format!("{}2", get_svc_name(DatabaseKind::Postgresql, provider_kind.clone()));
    let database_username_2 = "superuser2".to_string();
    let database_name_2 = "postgres2".to_string();

    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: context.organization_id().to_string(),
        action: Action::Create,
        applications: vec![
            Application {
                id: generate_id(),
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
                start_timeout_in_seconds: 60,
            },
            Application {
                id: generate_id(),
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
                storage: vec![Storage {
                    id: generate_id(),
                    name: "photos".to_string(),
                    storage_type: StorageType::Ssd,
                    size_in_gib: 10,
                    mount_point: "/mnt/photos".to_string(),
                    snapshot_retention_in_days: 0,
                }],
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
                start_timeout_in_seconds: 60,
            },
            Application {
                id: generate_id(),
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
                storage: vec![Storage {
                    id: generate_id(),
                    name: "photos".to_string(),
                    storage_type: StorageType::Ssd,
                    size_in_gib: 10,
                    mount_point: "/mnt/photos".to_string(),
                    snapshot_retention_in_days: 0,
                }],
                environment_vars: btreemap! {
                    "IS_DOCUMENTDB".to_string() => base64::encode("false"),
                    "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string() => base64::encode(database_host_mongo.clone()),
                    "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string() => base64::encode(database_uri_mongo.clone()),
                    "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string() => base64::encode(database_port_mongo.to_string()),
                    "MONGODB_DBNAME".to_string() => base64::encode(&database_db_name_mongo.clone()),
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
                start_timeout_in_seconds: 60,
            },
        ],
        routers: vec![
            Router {
                id: generate_id(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id().to_string(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app1".to_string(),
                    application_name: app_name_1.clone(),
                }],
                sticky_sessions_enabled: false,
            },
            Router {
                id: generate_id(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id().to_string(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app2".to_string(),
                    application_name: app_name_2.clone(),
                }],
                sticky_sessions_enabled: false,
            },
            Router {
                id: generate_id(),
                name: "third-router".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id().to_string(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app3".to_string(),
                    application_name: app_name_3.clone(),
                }],
                sticky_sessions_enabled: false,
            },
        ],
        databases: vec![
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                id: generate_id(),
                name: database_name.clone(),
                version: "11.8.0".to_string(),
                fqdn_id: fqdn.clone(),
                fqdn: fqdn.clone(),
                port: database_port.clone(),
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
            },
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                id: generate_id(),
                name: database_name_2.clone(),
                version: "11.8.0".to_string(),
                fqdn_id: fqdn_2.clone(),
                fqdn: fqdn_2.clone(),
                port: database_port.clone(),
                username: database_username_2.clone(),
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
                kind: DatabaseKind::Mongodb,
                action: Action::Create,
                id: generate_id(),
                name: database_db_name_mongo.clone(),
                version: version_mongo.to_string(),
                fqdn_id: database_host_mongo.clone(),
                fqdn: database_host_mongo.clone(),
                port: database_port_mongo.clone(),
                username: database_username_mongo.clone(),
                password: database_password_mongo.clone(),
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

pub fn working_minimal_environment(context: &Context, test_domain: &str) -> Environment {
    let suffix = generate_id();
    let application_id = generate_id();
    let application_name = format!("{}-{}", "simple-app".to_string(), &suffix);
    let router_id = generate_id();
    let router_name = "main".to_string();
    let application_domain = format!(
        "{}.{}.{}",
        application_id,
        context.cluster_id().to_string(),
        test_domain
    );
    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: context.organization_id().to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: application_id,
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
            start_timeout_in_seconds: 60,
        }],
        routers: vec![Router {
            id: router_id,
            name: router_name,
            action: Action::Create,
            default_domain: application_domain,
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name: format!("{}-{}", "simple-app".to_string(), &suffix),
            }],
            sticky_sessions_enabled: false,
        }],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

pub fn environment_only_http_server_router_with_sticky_session(context: &Context, test_domain: &str) -> Environment {
    let mut env = environment_only_http_server_router(context, test_domain.clone());

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
) -> Environment {
    let fqdn = get_svc_name(DatabaseKind::Postgresql, provider_kind.clone()).to_string();

    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_password(true);
    let database_name = "postgres".to_string();

    let suffix = generate_id();
    let application_name1 = sanitize_name("postgresql", &format!("{}-{}", "postgresql-app1", &suffix));
    let application_name2 = sanitize_name("postgresql", &format!("{}-{}", "postgresql-app2", &suffix));

    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: context.organization_id().to_string(),
        action: Action::Create,
        databases: vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            id: generate_id(),
            name: database_name.clone(),
            version: "11.8.0".to_string(),
            fqdn_id: fqdn.clone(),
            fqdn: fqdn.clone(),
            port: database_port.clone(),
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
                id: generate_id(),
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
                storage: vec![Storage {
                    id: generate_id(),
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
                start_timeout_in_seconds: 60,
            },
            Application {
                id: generate_id(),
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
                storage: vec![Storage {
                    id: generate_id(),
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
                start_timeout_in_seconds: 60,
            },
        ],
        routers: vec![
            Router {
                id: generate_id(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id().to_string(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/".to_string(),
                    application_name: application_name1.to_string(),
                }],
                sticky_sessions_enabled: false,
            },
            Router {
                id: generate_id(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id().to_string(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/coco".to_string(),
                    application_name: application_name2.to_string(),
                }],
                sticky_sessions_enabled: false,
            },
        ],
        clone_from_environment_id: None,
    }
}

pub fn non_working_environment(context: &Context, test_domain: &str) -> Environment {
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
pub fn echo_app_environment(context: &Context, test_domain: &str) -> Environment {
    let suffix = generate_id();
    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: context.organization_id().to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: generate_id(),
            name: format!("{}-{}", "echo-app".to_string(), &suffix),
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
            start_timeout_in_seconds: 60,
        }],
        routers: vec![Router {
            id: generate_id(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id().to_string(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name: format!("{}-{}", "echo-app".to_string(), &suffix),
            }],
            sticky_sessions_enabled: false,
        }],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

pub fn environment_only_http_server(context: &Context) -> Environment {
    let suffix = generate_id();
    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: context.organization_id().to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: generate_id(),
            name: format!("{}-{}", "mini-http".to_string(), &suffix),
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
            start_timeout_in_seconds: 60,
        }],
        routers: vec![],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

pub fn environment_only_http_server_router(context: &Context, test_domain: &str) -> Environment {
    let suffix = generate_id();
    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: context.organization_id().to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: generate_id(),
            name: format!("{}-{}", "mini-http".to_string(), &suffix),
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
            start_timeout_in_seconds: 60,
        }],
        routers: vec![Router {
            id: generate_id(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: format!("{}.{}.{}", generate_id(), context.cluster_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name: format!("{}-{}", "mini-http".to_string(), &suffix),
            }],
            sticky_sessions_enabled: false,
        }],
        databases: vec![],
        clone_from_environment_id: None,
    }
}

/// Test if stick session are activated on given routers via cookie.
pub fn routers_sessions_are_sticky(routers: Vec<Router>) -> bool {
    let mut is_ok = true;
    let http_client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true) // this test ignores certificate validity (not its purpose)
        .build()
        .expect("Cannot build reqwest client");

    for router in routers.iter() {
        for route in router.routes.iter() {
            let http_request_result = http_client
                .get(format!("https://{}{}", router.default_domain, route.path))
                .send();

            if http_request_result.is_err() {
                return false;
            }

            let http_response = http_request_result.expect("cannot retrieve HTTP request result");

            is_ok &= match http_response.headers().get("Set-Cookie") {
                None => false,
                Some(value) => match value.to_str() {
                    Err(_) => false,
                    Ok(s) => s.contains("INGRESSCOOKIE_QOVERY=") && s.contains("Max-Age=85400"),
                },
            };
        }
    }

    is_ok
}

pub fn test_db(
    context: Context,
    logger: Box<dyn Logger>,
    mut environment: Environment,
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

    let app_id = generate_id();

    let database_username = "superuser".to_string();
    let database_password = generate_id();
    let db_kind_str = db_kind.name().to_string();
    let db_id = generate_id();
    let database_host = format!("{}-{}", db_id, db_kind_str.clone());
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
    let database_port = db_infos.db_port.clone();
    let storage_size = 10;
    let db_disk_type = db_disk_type(provider_kind.clone(), database_mode.clone());
    let db_instance_type = db_instance_type(provider_kind.clone(), db_kind.clone(), database_mode.clone());
    let db = Database {
        kind: db_kind.clone(),
        action: Action::Create,
        id: db_id.clone(),
        name: db_id.clone(),
        version: version.to_string(),
        fqdn_id: database_host.clone(),
        fqdn: database_fqdn.clone(),
        port: database_port.clone(),
        username: database_username.clone(),
        password: database_password.clone(),
        total_cpus: "100m".to_string(),
        total_ram_in_mib: 512,
        disk_size_in_gib: storage_size.clone(),
        database_instance_type: db_instance_type.to_string(),
        database_disk_type: db_disk_type.to_string(),
        encrypt_disk: true,
        activate_high_availability: false,
        activate_backups: false,
        publicly_accessible: is_public.clone(),
        mode: database_mode.clone(),
    };

    environment.databases = vec![db.clone()];

    let app_name = format!("{}-app-{}", db_kind_str.clone(), generate_id());
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.id = app_id.clone();
            app.name = app_id.clone();
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
        .collect::<Vec<qovery_engine::models::Application>>();
    environment.routers[0].routes[0].application_name = app_id.clone();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment.clone());
    let ea_delete = EnvironmentAction::Environment(environment_delete.clone());

    let ret = environment.deploy_environment(provider_kind.clone(), &context, &ea, logger.clone());
    assert!(matches!(ret, TransactionResult::Ok));

    match database_mode.clone() {
        DatabaseMode::CONTAINER => {
            match get_pvc(
                context.clone(),
                provider_kind.clone(),
                environment.clone(),
                secrets.clone(),
            ) {
                Ok(pvc) => assert_eq!(
                    pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                    format!("{}Gi", storage_size)
                ),
                Err(_) => assert!(false),
            };

            match get_svc(
                context.clone(),
                provider_kind.clone(),
                environment.clone(),
                secrets.clone(),
            ) {
                Ok(svc) => assert_eq!(
                    svc.items
                        .expect("No items in svc")
                        .into_iter()
                        .filter(|svc| svc.metadata.name == database_host && &svc.spec.svc_type == "LoadBalancer")
                        .collect::<Vec<SVCItem>>()
                        .len(),
                    match is_public {
                        true => 1,
                        false => 0,
                    }
                ),
                Err(_) => assert!(false),
            };
        }
        DatabaseMode::MANAGED => {
            match get_svc(context, provider_kind.clone(), environment.clone(), secrets.clone()) {
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
                Err(_) => assert!(false),
            };
        }
    }

    let ret = environment_delete.delete_environment(provider_kind.clone(), &context_for_delete, &ea_delete, logger);
    assert!(matches!(ret, TransactionResult::Ok));

    return test_name.to_string();
}

pub fn get_environment_test_kubernetes<'a>(
    provider_kind: Kind,
    context: &Context,
    cloud_provider: &'a dyn CloudProvider,
    dns_provider: &'a dyn DnsProvider,
    logger: &'a dyn Logger,
) -> Box<dyn Kubernetes + 'a> {
    let secrets = FuncTestsSecrets::new();
    let k: Box<dyn Kubernetes>;

    match provider_kind {
        Kind::Aws => {
            let region = secrets
                .AWS_DEFAULT_REGION
                .as_ref()
                .expect("AWS_DEFAULT_REGION is not set")
                .as_str();
            let aws_region = AwsRegion::from_str(region).expect("wrong AWS region name, please ensure it's correct");
            k = Box::new(
                EKS::new(
                    context.clone(),
                    context.cluster_id(),
                    uuid::Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_id()).as_str(),
                    AWS_KUBERNETES_VERSION,
                    aws_region.clone(),
                    aws_region.get_zones_to_string(),
                    cloud_provider,
                    dns_provider,
                    AWS::kubernetes_cluster_options(secrets.clone(), None),
                    AWS::kubernetes_nodes(),
                    logger,
                )
                .unwrap(),
            );
        }
        Kind::Do => {
            k = Box::new(
                DOKS::new(
                    context.clone(),
                    context.cluster_id().to_string(),
                    uuid::Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_id()),
                    DO_KUBERNETES_VERSION.to_string(),
                    DoRegion::from_str(
                        secrets
                            .clone()
                            .DIGITAL_OCEAN_DEFAULT_REGION
                            .expect("DIGITAL_OCEAN_DEFAULT_REGION is not set")
                            .as_str(),
                    )
                    .unwrap(),
                    cloud_provider,
                    dns_provider,
                    DO::kubernetes_nodes(),
                    DO::kubernetes_cluster_options(secrets.clone(), Option::from(context.cluster_id().to_string())),
                    logger,
                )
                .unwrap(),
            );
        }
        Kind::Scw => {
            k = Box::new(
                Kapsule::new(
                    context.clone(),
                    context.cluster_id().to_string(),
                    uuid::Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_id()),
                    SCW_KUBERNETES_VERSION.to_string(),
                    ScwZone::from_str(
                        secrets
                            .clone()
                            .SCALEWAY_DEFAULT_REGION
                            .expect("SCALEWAY_DEFAULT_REGION is not set")
                            .as_str(),
                    )
                    .unwrap(),
                    cloud_provider,
                    dns_provider,
                    Scaleway::kubernetes_nodes(),
                    Scaleway::kubernetes_cluster_options(secrets, None),
                    logger,
                )
                .unwrap(),
            );
        }
    }

    return k;
}

pub fn get_cluster_test_kubernetes<'a>(
    provider_kind: Kind,
    secrets: FuncTestsSecrets,
    context: &Context,
    cluster_id: String,
    cluster_name: String,
    boot_version: String,
    localisation: &str,
    aws_zones: Option<Vec<AwsZones>>,
    cloud_provider: &'a dyn CloudProvider,
    dns_provider: &'a dyn DnsProvider,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    logger: &'a dyn Logger,
) -> Box<dyn Kubernetes + 'a> {
    let k: Box<dyn Kubernetes>;

    match provider_kind {
        Kind::Aws => {
            let mut options = AWS::kubernetes_cluster_options(secrets, None);
            let aws_region = AwsRegion::from_str(localisation).expect("expected correct AWS region");
            options.vpc_qovery_network_mode = vpc_network_mode.unwrap();
            let aws_zones = aws_zones.unwrap().into_iter().map(|zone| zone.to_string()).collect();
            k = Box::new(
                EKS::new(
                    context.clone(),
                    cluster_id.as_str(),
                    uuid::Uuid::new_v4(),
                    cluster_name.as_str(),
                    boot_version.as_str(),
                    aws_region.clone(),
                    aws_zones,
                    cloud_provider,
                    dns_provider,
                    options,
                    AWS::kubernetes_nodes(),
                    logger,
                )
                .unwrap(),
            );
        }
        Kind::Do => {
            k = Box::new(
                DOKS::new(
                    context.clone(),
                    cluster_id.clone(),
                    uuid::Uuid::new_v4(),
                    cluster_name.clone(),
                    boot_version,
                    DoRegion::from_str(localisation.clone()).expect("Unknown region set for DOKS"),
                    cloud_provider,
                    dns_provider,
                    DO::kubernetes_nodes(),
                    DO::kubernetes_cluster_options(secrets, Option::from(cluster_name)),
                    logger,
                )
                .unwrap(),
            );
        }
        Kind::Scw => {
            k = Box::new(
                Kapsule::new(
                    context.clone(),
                    cluster_id.clone(),
                    uuid::Uuid::new_v4(),
                    cluster_name.clone(),
                    boot_version,
                    ScwZone::from_str(localisation.clone()).expect("Unknown zone set for Kapsule"),
                    cloud_provider,
                    dns_provider,
                    Scaleway::kubernetes_nodes(),
                    Scaleway::kubernetes_cluster_options(secrets, None),
                    logger,
                )
                .unwrap(),
            );
        }
    }

    return k;
}

pub fn cluster_test(
    test_name: &str,
    provider_kind: Kind,
    context: Context,
    logger: Box<dyn Logger>,
    localisation: &str,
    aws_zones: Option<Vec<AwsZones>>,
    secrets: FuncTestsSecrets,
    test_type: ClusterTestType,
    major_boot_version: u8,
    minor_boot_version: u8,
    cluster_domain: ClusterDomain,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    environment_to_deploy: Option<&EnvironmentAction>,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();

    let cluster_id = generate_cluster_id(localisation.clone());
    let cluster_name = generate_cluster_id(localisation.clone());
    let boot_version = format!("{}.{}", major_boot_version, minor_boot_version.clone());

    let engine;
    match provider_kind {
        Kind::Aws => engine = AWS::docker_cr_engine(&context, logger.clone()),
        Kind::Do => engine = DO::docker_cr_engine(&context, logger.clone()),
        Kind::Scw => engine = Scaleway::docker_cr_engine(&context, logger.clone()),
    };
    let dns_provider = dns_provider_cloudflare(&context, cluster_domain);
    let mut deploy_tx = engine.session().unwrap().transaction();
    let mut delete_tx = engine.session().unwrap().transaction();

    let cp: Box<dyn CloudProvider>;
    cp = match provider_kind {
        Kind::Aws => AWS::cloud_provider(&context),
        Kind::Do => DO::cloud_provider(&context),
        Kind::Scw => Scaleway::cloud_provider(&context),
    };

    let mut aws_zones_string: Vec<String> = Vec::with_capacity(3);
    if aws_zones.is_some() {
        for zone in aws_zones.clone().unwrap() {
            aws_zones_string.push(zone.to_string())
        }
    };

    let kubernetes = get_cluster_test_kubernetes(
        provider_kind.clone(),
        secrets.clone(),
        &context,
        cluster_id.clone(),
        cluster_name.clone(),
        boot_version.clone(),
        localisation.clone(),
        aws_zones.clone(),
        cp.as_ref(),
        &dns_provider,
        vpc_network_mode.clone(),
        logger.as_ref(),
    );

    // Deploy
    if let Err(err) = deploy_tx.create_kubernetes(kubernetes.as_ref()) {
        panic!("{:?}", err)
    }
    assert!(matches!(deploy_tx.commit(logger.clone()), TransactionResult::Ok));

    // Deploy env if any
    if let Some(env) = environment_to_deploy {
        let mut deploy_env_tx = engine.session().unwrap().transaction();

        // Deploy env
        if let Err(err) = deploy_env_tx.deploy_environment(kubernetes.as_ref(), env) {
            panic!("{:?}", err)
        }

        assert!(matches!(deploy_env_tx.commit(logger.clone()), TransactionResult::Ok));
    }

    if let Err(err) = metrics_server_test(
        kubernetes
            .get_kubeconfig_file_path()
            .expect("Unable to get config file path"),
        kubernetes.cloud_provider().credentials_environment_variables(),
    ) {
        panic!("{:?}", err)
    }

    match test_type {
        ClusterTestType::Classic => {}
        ClusterTestType::WithPause => {
            let mut pause_tx = engine.session().unwrap().transaction();
            let mut resume_tx = engine.session().unwrap().transaction();

            // Pause
            if let Err(err) = pause_tx.pause_kubernetes(kubernetes.as_ref()) {
                panic!("{:?}", err)
            }
            assert!(matches!(pause_tx.commit(logger.clone()), TransactionResult::Ok));

            // Resume
            if let Err(err) = resume_tx.create_kubernetes(kubernetes.as_ref()) {
                panic!("{:?}", err)
            }

            assert!(matches!(resume_tx.commit(logger.clone()), TransactionResult::Ok));

            if let Err(err) = metrics_server_test(
                kubernetes
                    .get_kubeconfig_file_path()
                    .expect("Unable to get config file path"),
                kubernetes.cloud_provider().credentials_environment_variables(),
            ) {
                panic!("{:?}", err)
            }
        }
        ClusterTestType::WithUpgrade => {
            let upgrade_to_version = format!("{}.{}", major_boot_version, minor_boot_version.clone() + 1);
            let upgraded_kubernetes = get_cluster_test_kubernetes(
                provider_kind.clone(),
                secrets.clone(),
                &context,
                cluster_id.clone(),
                cluster_name.clone(),
                upgrade_to_version.clone(),
                localisation.clone(),
                aws_zones,
                cp.as_ref(),
                &dns_provider,
                vpc_network_mode.clone(),
                logger.as_ref(),
            );
            let mut upgrade_tx = engine.session().unwrap().transaction();
            let mut delete_tx = engine.session().unwrap().transaction();

            // Upgrade
            if let Err(err) = upgrade_tx.create_kubernetes(upgraded_kubernetes.as_ref()) {
                panic!("{:?}", err)
            }
            assert!(matches!(upgrade_tx.commit(logger.clone()), TransactionResult::Ok));

            if let Err(err) = metrics_server_test(
                upgraded_kubernetes
                    .as_ref()
                    .get_kubeconfig_file_path()
                    .expect("Unable to get config file path"),
                upgraded_kubernetes
                    .as_ref()
                    .cloud_provider()
                    .credentials_environment_variables(),
            ) {
                panic!("{:?}", err)
            }

            // Delete
            if let Err(err) = delete_tx.delete_kubernetes(upgraded_kubernetes.as_ref()) {
                panic!("{:?}", err)
            }
            assert!(matches!(delete_tx.commit(logger.clone()), TransactionResult::Ok));

            return test_name.to_string();
        }
    }

    // Destroy env if any
    if let Some(env) = environment_to_deploy {
        let mut destroy_env_tx = engine.session().unwrap().transaction();

        // Deploy env
        if let Err(err) = destroy_env_tx.delete_environment(kubernetes.as_ref(), env) {
            panic!("{:?}", err)
        }
        assert!(matches!(destroy_env_tx.commit(logger.clone()), TransactionResult::Ok));
    }

    // Delete
    if let Err(err) = delete_tx.delete_kubernetes(kubernetes.as_ref()) {
        panic!("{:?}", err)
    }
    assert!(matches!(delete_tx.commit(logger.clone()), TransactionResult::Ok));

    test_name.to_string()
}

pub fn metrics_server_test<P>(kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let result = kubernetes_get_all_hpas(kubernetes_config, envs, None);

    match result {
        Ok(hpas) => {
            for hpa in hpas.items.expect("No hpa item").into_iter() {
                if !hpa
                    .metadata
                    .annotations
                    .expect("No hpa annotation.")
                    .conditions
                    .expect("No hpa condition.")
                    .contains("ValidMetricFound")
                {
                    return Err(CommandError::new_from_safe_message(
                        "Metrics server doesn't work".to_string(),
                    ));
                }
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}
