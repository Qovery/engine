use crate::helpers::aws::{AWS_KUBERNETES_VERSION, AWS_TEST_REGION};
use crate::helpers::common::{compute_test_cluster_endpoint, Cluster, ClusterDomain, Infrastructure};
use crate::helpers::kubernetes::{KUBERNETES_MAX_NODES, KUBERNETES_MIN_NODES};
use crate::helpers::scaleway::{SCW_KUBERNETES_VERSION, SCW_TEST_ZONE};
use crate::helpers::utilities::{
    db_disk_type, db_infos, db_instance_type, generate_id, generate_password, get_pvc, get_svc, get_svc_name, init,
    FuncTestsSecrets,
};
use chrono::Utc;
use core::cell::RefCell;
use core::default::Default;
use core::option::Option;
use core::option::Option::{None, Some};
use core::result::Result::{Err, Ok};
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::environment::Environment;
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::cmd::structs::SVCItem;
use qovery_engine::engine::EngineConfig;
use qovery_engine::io_models::application::{Application, GitCredentials, Port, Protocol, Storage, StorageType};
use qovery_engine::io_models::context::{CloneForTest, Context};
use qovery_engine::io_models::database::DatabaseMode::{CONTAINER, MANAGED};
use qovery_engine::io_models::database::{Database, DatabaseKind, DatabaseMode};
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::Action;
use qovery_engine::logger::Logger;
use qovery_engine::transaction::{DeploymentOption, Transaction, TransactionResult};
use qovery_engine::utilities::to_short_id;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::str::FromStr;
use tracing::{span, Level};
use uuid::Uuid;

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
                engine_config.container_registry(),
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
                engine_config.container_registry(),
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
                engine_config.container_registry(),
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
                engine_config.container_registry(),
                logger,
            )
            .unwrap();
        let env = Rc::new(RefCell::new(env));
        let _ = tx.delete_environment(&env);

        tx.commit()
    }
}

pub fn environment_3_apps_3_databases(
    context: &Context,
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
    let database_password_mongo = generate_password(CONTAINER);
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
    let database_password = generate_password(CONTAINER);
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
                name: app_name_1,
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
                storage: {
                    let id = Uuid::new_v4();
                    vec![Storage {
                        id: to_short_id(&id),
                        long_id: id,
                        name: "photos".to_string(),
                        storage_type: StorageType::Ssd,
                        size_in_gib: 10,
                        mount_point: "/mnt/photos".to_string(),
                        snapshot_retention_in_days: 0,
                    }]
                },
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
                    is_default: true,
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
                name: app_name_2,
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
                    is_default: true,
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
                name: app_name_3,
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
                    is_default: true,
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
        containers: vec![],
        routers: vec![],
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
        containers: vec![],
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
        containers: vec![],
        routers: vec![],
        databases: vec![],
        clone_from_environment_id: None,
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
    let database_username = match db_kind {
        DatabaseKind::Redis => match database_mode {
            MANAGED => match version {
                "6" => "default".to_string(),
                _ => "superuser".to_string(),
            },
            CONTAINER => "".to_string(),
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

    environment.databases = vec![db.clone()];

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
                is_default: true,
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
        Kind::Do => ("".to_string(), "".to_string()),
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
                KubernetesKind::Doks => todo!(),
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

            match get_svc(context, provider_kind, environment, secrets) {
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
        MANAGED => {
            match get_svc(context, provider_kind, environment, secrets) {
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
                    kubernetes_version,
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
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    1,
                    1,
                    EngineLocation::QoverySide, // EC2 is not meant to run Engine
                ),
                KubernetesKind::Doks => todo!(),
                KubernetesKind::ScwKapsule => Scaleway::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version,
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

    let ret = environment_delete.delete_environment(&ea_delete, logger, engine_config_for_delete);
    assert!(matches!(ret, TransactionResult::Ok));

    test_name.to_string()
}

pub fn test_pause_managed_db(
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
    environment.applications = vec![];

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let mut environment_pause = environment.clone();
    environment_pause.action = Action::Pause;

    let (localisation, kubernetes_version) = match provider_kind {
        Kind::Aws => (AWS_TEST_REGION.to_string(), AWS_KUBERNETES_VERSION.to_string()),
        Kind::Do => ("".to_string(), "".to_string()),
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
                KubernetesKind::Doks => todo!(),
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

    let ret = environment.deploy_environment(&environment, logger.clone(), engine_config);
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

            match get_svc(context, provider_kind, environment, secrets) {
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
        MANAGED => {
            match get_svc(context, provider_kind, environment, secrets) {
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
                    kubernetes_version,
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
                    kubernetes_version,
                    &cluster_domain,
                    None,
                    1,
                    1,
                    EngineLocation::QoverySide, // EC2 is not meant to run Engine
                ),
                KubernetesKind::Doks => todo!(),
                KubernetesKind::ScwKapsule => Scaleway::docker_cr_engine(
                    &context_for_delete,
                    logger.clone(),
                    localisation.as_str(),
                    KubernetesKind::ScwKapsule,
                    kubernetes_version,
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

    let ret = environment_pause.pause_environment(&environment_pause, logger.clone(), engine_config);
    assert!(matches!(ret, TransactionResult::Ok));

    let ret = environment_delete.delete_environment(&environment_delete, logger, engine_config_for_delete);
    assert!(matches!(ret, TransactionResult::Ok));

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
        context.cluster_short_id(),
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
                is_default: true,
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
        Kind::Do => ("".to_string(), "".to_string()),
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
                cluster_id: context.cluster_short_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Do => todo!(),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context,
            logger.clone(),
            localisation.as_str(),
            KubernetesKind::ScwKapsule,
            kubernetes_version.clone(),
            &ClusterDomain::Default {
                cluster_id: context.cluster_short_id().to_string(),
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
        MANAGED => {
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
                cluster_id: context_for_delete.cluster_short_id().to_string(),
            },
            None,
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Do => todo!(),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context_for_delete,
            logger.clone(),
            localisation.as_str(),
            KubernetesKind::ScwKapsule,
            kubernetes_version,
            &ClusterDomain::Default {
                cluster_id: context_for_delete.cluster_short_id().to_string(),
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
