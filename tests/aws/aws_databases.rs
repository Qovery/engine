use crate::helpers;
use crate::helpers::aws::{aws_default_infra_config, AWS_DATABASE_INSTANCE_TYPE};
use crate::helpers::common::{ClusterDomain, Infrastructure};
use crate::helpers::database::{
    test_db, test_deploy_an_environment_with_db_and_resize_disk, test_pause_managed_db, StorageSize,
};
use crate::helpers::utilities::{
    context_for_resource, engine_run_test, get_pods, init, logger, metrics_registry, FuncTestsSecrets,
};
use crate::helpers::utilities::{generate_id, get_svc_name, is_pod_restarted_env};
use ::function_name::named;
use base64::engine::general_purpose;
use base64::Engine;
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::io_models::application::{Port, Protocol};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::database::DatabaseMode::{CONTAINER, MANAGED};
use qovery_engine::io_models::database::{Database, DatabaseKind, DatabaseMode};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::Action;
use qovery_engine::models::aws::AwsStorageType;
use qovery_engine::transaction::TransactionResult;
use qovery_engine::utilities::to_short_id;
use std::thread::sleep;
use std::time::Duration;
use tracing::{span, Level};
use uuid::Uuid;

/**
**
** Global database tests
**
**/

// to check overload between several databases and apps
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_an_environment_with_3_databases_and_3_apps() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let cluster_id = secrets
            .AWS_TEST_CLUSTER_LONG_ID
            .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            cluster_id,
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry.clone());
        let environment = helpers::database::environment_3_apps_3_databases(
            &context,
            Some(Box::new(AWS_DATABASE_INSTANCE_TYPE)),
            &AwsStorageType::GP2.to_k8s_storage_class(),
            Kind::Aws,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_an_environment_with_db_and_pause_it() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let cluster_id = secrets
            .AWS_TEST_CLUSTER_LONG_ID
            .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            cluster_id,
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::environment_2_app_2_routers_1_psql(
            &context,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            Some(Box::new(AWS_DATABASE_INSTANCE_TYPE)),
            &AwsStorageType::GP2.to_k8s_storage_class(),
            Kind::Aws,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment.pause_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this db
        let ret = get_pods(&infra_ctx, Kind::Aws, &environment, &environment.databases[0].long_id, secrets);
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

// Ensure a full environment can run correctly
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_development_environment_with_all_options() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let cluster_id = secrets
            .AWS_TEST_CLUSTER_LONG_ID
            .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            cluster_id,
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry.clone());
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = helpers::environment::environment_2_app_2_routers_1_psql(
            &context,
            test_domain.as_str(),
            Some(Box::new(AWS_DATABASE_INSTANCE_TYPE)),
            &AwsStorageType::GP2.to_k8s_storage_class(),
            Kind::Aws,
        );
        //let env_to_check = environment.clone();
        let mut environment_delete = helpers::environment::environment_2_app_2_routers_1_psql(
            &context_for_deletion,
            test_domain.as_str(),
            Some(Box::new(AWS_DATABASE_INSTANCE_TYPE)),
            &AwsStorageType::GP2.to_k8s_storage_class(),
            Kind::Aws,
        );

        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_for_deletion = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // TODO: should be uncommented as soon as cert-manager is fixed
        // for the moment this assert report a SSL issue on the second router, so it's works well
        /*    let connections = test_utilities::utilities::check_all_connections(&env_to_check);
        for con in connections {
            assert_eq!(con, true);
        }*/

        let ret = environment_delete.delete_environment(&ea_for_deletion, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

// Ensure redeploy works as expected
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_environment_and_redeploy() {
    use chrono::Utc;

    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
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
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_redeploy = context.clone_not_same_execution_id();
        let infra_ctx_for_redeploy =
            aws_default_infra_config(&context_for_redeploy, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete =
            aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry.clone());

        let mut environment = helpers::environment::working_minimal_environment_with_router(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let app_name = format!("postgresql-app-{}", generate_id());
        let database_host = get_svc_name(DatabaseKind::Postgresql, Kind::Aws).to_string();
        let database_port = 5432;
        let database_db_name = "postgres".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id().to_string();
        environment.databases = vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            long_id: Uuid::new_v4(),
            name: database_db_name.clone(),
            kube_name: database_db_name.clone(),
            created_at: Utc::now(),
            version: "11.22.0".to_string(),
            fqdn_id: database_host.clone(),
            fqdn: database_host.clone(),
            port: database_port,
            username: database_username.clone(),
            password: database_password.to_string(),
            cpu_request_in_milli: 500,
            cpu_limit_in_milli: 500,
            ram_request_in_mib: 512,
            ram_limit_in_mib: 512,
            disk_size_in_gib: 10,
            database_disk_type: AwsStorageType::GP2.to_k8s_storage_class(),
            encrypt_disk: false,
            activate_high_availability: false,
            activate_backups: false,
            publicly_accessible: false,
            mode: CONTAINER,
            database_instance_type: None,
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch.clone_from(&app_name);
                app.commit_id = "5990752647af11ef21c3d46a51abbde3da1ab351".to_string();
                app.ports = vec![Port {
                    long_id: Default::default(),
                    port: 1234,
                    name: "p1234".to_string(),
                    is_default: true,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                }];
                app.environment_vars_with_infos = btreemap! {
                     "PG_DBNAME".to_string() => VariableInfo{ value: general_purpose::STANDARD.encode(database_db_name.clone()), is_secret: false},
                     "PG_HOST".to_string() => VariableInfo{ value: general_purpose::STANDARD.encode(database_host.clone()), is_secret: false},
                     "PG_PORT".to_string() => VariableInfo{ value: general_purpose::STANDARD.encode(database_port.to_string()), is_secret: false},
                     "PG_USERNAME".to_string() => VariableInfo{ value: general_purpose::STANDARD.encode(database_username.clone()), is_secret: false},
                     "PG_PASSWORD".to_string() => VariableInfo{ value: general_purpose::STANDARD.encode(database_password.clone()), is_secret: false},
                };
                app.readiness_probe = Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 1234,
                    failure_threshold: 9,
                    success_threshold: 1,
                    initial_delay_seconds: 15,
                    period_seconds: 10,
                    timeout_seconds: 10,
                });
                app.liveness_probe = None;
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let environment_to_redeploy = environment.clone();
        let environment_check = environment.clone();
        let ea_redeploy = environment_to_redeploy.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        sleep(Duration::from_secs(60));

        let ret = environment_to_redeploy.deploy_environment(&ea_redeploy, &infra_ctx_for_redeploy);
        assert!(matches!(ret, TransactionResult::Ok));

        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let (ret, _) = is_pod_restarted_env(
            &infra_ctx,
            Kind::Aws,
            &environment_check,
            &environment_check.databases[0].long_id,
            secrets,
        );
        assert!(ret);

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok | TransactionResult::Error(_)));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn test_oversized_volume() {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets
        .AWS_TEST_CLUSTER_LONG_ID
        .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
    let localisation = secrets
        .AWS_TEST_CLUSTER_REGION
        .as_ref()
        .expect("AWS_TEST_CLUSTER_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .AWS_TEST_ORGANIZATION_LONG_ID
            .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
        cluster_id,
    );
    let environment = helpers::database::database_test_environment(&context);

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            metrics_registry(),
            environment,
            secrets,
            "13",
            function_name!(),
            DatabaseKind::Postgresql,
            KubernetesKind::Eks,
            DatabaseMode::CONTAINER,
            localisation,
            false,
            ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            None,
            StorageSize::OverSize,
        )
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn postgresql_disk_resize() {
    test_deploy_an_environment_with_db_and_resize_disk(
        DatabaseKind::Postgresql,
        "13",
        function_name!(),
        KubernetesKind::Eks,
    )
}

/**
**
** PostgreSQL tests
**
**/
#[allow(dead_code)]
pub fn test_postgresql_configuration(
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    kubernetes_kind: KubernetesKind,
    is_public: bool,
) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets
        .AWS_TEST_CLUSTER_LONG_ID
        .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
    let localisation = secrets
        .AWS_TEST_CLUSTER_REGION
        .as_ref()
        .expect("AWS_TEST_CLUSTER_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .AWS_TEST_ORGANIZATION_LONG_ID
            .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
        cluster_id,
    );

    let environment = helpers::database::database_test_environment(&context);

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
            version,
            test_name,
            DatabaseKind::Postgresql,
            kubernetes_kind,
            database_mode,
            localisation,
            is_public,
            ClusterDomain::Default {
                cluster_id: cluster_id.to_string(),
            },
            None,
            StorageSize::NormalSize,
        )
    })
}

#[allow(dead_code)]
pub fn test_postgresql_pause(
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    kubernetes_kind: KubernetesKind,
    is_public: bool,
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

    let environment = helpers::database::database_test_environment(&context);

    engine_run_test(|| {
        test_pause_managed_db(
            context.clone(),
            logger(),
            metrics_registry(),
            environment,
            secrets.clone(),
            version,
            test_name,
            DatabaseKind::Postgresql,
            kubernetes_kind,
            database_mode.clone(),
            is_public,
            ClusterDomain::Default {
                cluster_id: cluster_id.to_string(),
            },
            None,
        )
    })
}

// Postgres environment environment
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v14_deploy_a_working_dev_environment() {
    test_postgresql_configuration("14", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v14_deploy_a_working_dev_environment() {
    test_postgresql_configuration("14", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v15_deploy_a_working_dev_environment() {
    test_postgresql_configuration("15", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v15_deploy_a_working_dev_environment() {
    test_postgresql_configuration("15", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v16_deploy_a_working_dev_environment() {
    test_postgresql_configuration("16", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_postgresql_v16_deploy_a_working_dev_environment() {
    test_postgresql_configuration("16", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

// Postgres production environment
#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_prod_environment() {
    test_postgresql_configuration("11", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v11_deploy_a_working_prod_environment() {
    test_postgresql_configuration("11", function_name!(), MANAGED, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_prod_environment() {
    test_postgresql_configuration("12", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v12_deploy_a_working_prod_environment() {
    test_postgresql_configuration("12", function_name!(), MANAGED, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_prod_environment() {
    test_postgresql_configuration("13", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v13_deploy_a_working_prod_environment() {
    test_postgresql_configuration("13", function_name!(), MANAGED, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v13_deploy_and_pause() {
    test_postgresql_pause("13", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v14_deploy_a_working_prod_environment() {
    test_postgresql_configuration("14", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v14_deploy_a_working_prod_environment() {
    test_postgresql_configuration("14", function_name!(), MANAGED, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v15_deploy_a_working_prod_environment() {
    test_postgresql_configuration("15", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_postgresql_v15_deploy_a_working_prod_environment() {
    test_postgresql_configuration("15", function_name!(), MANAGED, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v16_deploy_a_working_prod_environment() {
    test_postgresql_configuration("16", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_postgresql_v16_deploy_a_working_prod_environment() {
    test_postgresql_configuration("16", function_name!(), MANAGED, KubernetesKind::Eks, true);
}

/**
**
** MongoDB tests
**
**/
#[allow(dead_code)]
pub fn test_mongodb_configuration(
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    kubernetes_kind: KubernetesKind,
    is_public: bool,
) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets
        .AWS_TEST_CLUSTER_LONG_ID
        .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
    let localisation = secrets
        .AWS_TEST_CLUSTER_REGION
        .as_ref()
        .expect("AWS_TEST_CLUSTER_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .AWS_TEST_ORGANIZATION_LONG_ID
            .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
        cluster_id,
    );
    let environment = helpers::database::database_test_environment(&context);

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
            version,
            test_name,
            DatabaseKind::Mongodb,
            kubernetes_kind,
            database_mode,
            localisation,
            is_public,
            ClusterDomain::Default {
                cluster_id: cluster_id.to_string(),
            },
            None,
            StorageSize::NormalSize,
        )
    })
}

//
// development environment
//

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn mongodb_disk_resize() {
    test_deploy_an_environment_with_db_and_resize_disk(
        DatabaseKind::Mongodb,
        "4.4",
        function_name!(),
        KubernetesKind::Eks,
    )
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v5_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("5.0", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_mongodb_v5_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("5.0", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v6_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("6.0", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_mongodb_v6_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("6.0", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v7_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("7.0", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_mongodb_v7_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("7.0", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

//
// MongoDB production environment (DocumentDB)
//

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_mongodb_v4_0_deploy_a_working_prod_environment() {
    test_mongodb_configuration("4.0", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_mongodb_v5_0_deploy_a_working_prod_environment() {
    test_mongodb_configuration("5.0", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

/**
**
** MySQL tests
**
**/
#[allow(dead_code)]
pub fn test_mysql_configuration(
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    kubernetes_kind: KubernetesKind,
    is_public: bool,
) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets
        .AWS_TEST_CLUSTER_LONG_ID
        .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
    let localisation = secrets
        .AWS_TEST_CLUSTER_REGION
        .as_ref()
        .expect("AWS_TEST_CLUSTER_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .AWS_TEST_ORGANIZATION_LONG_ID
            .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
        cluster_id,
    );
    let environment = helpers::database::database_test_environment(&context);

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
            version,
            test_name,
            DatabaseKind::Mysql,
            kubernetes_kind,
            database_mode,
            localisation,
            is_public,
            ClusterDomain::Default {
                cluster_id: cluster_id.to_string(),
            },
            None,
            StorageSize::NormalSize,
        )
    })
}

// MySQL self-hosted environment
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn mysql_disk_resize() {
    test_deploy_an_environment_with_db_and_resize_disk(
        DatabaseKind::Mysql,
        "8.0",
        function_name!(),
        KubernetesKind::Eks,
    )
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

// MySQL production environment (RDS)
#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_prod_environment() {
    test_mysql_configuration("5.7", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_mysql_v5_7_deploy_a_working_prod_environment() {
    test_mysql_configuration("5.7", function_name!(), MANAGED, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_mysql_v8_0_deploy_a_working_prod_environment() {
    test_mysql_configuration("8.0", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_mysql_v8_0_deploy_a_working_prod_environment() {
    test_mysql_configuration("8.0", function_name!(), MANAGED, KubernetesKind::Eks, true);
}

/**
**
** Redis tests
**
**/
#[allow(dead_code)]
pub fn test_redis_configuration(
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    kubernetes_kind: KubernetesKind,
    is_public: bool,
) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets
        .AWS_TEST_CLUSTER_LONG_ID
        .expect("AWS_TEST_CLUSTER_LONG_ID is not set");
    let localisation = secrets
        .AWS_TEST_CLUSTER_REGION
        .as_ref()
        .expect("AWS_TEST_CLUSTER_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .AWS_TEST_ORGANIZATION_LONG_ID
            .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
        cluster_id,
    );
    let environment = helpers::database::database_test_environment(&context);

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
            version,
            test_name,
            DatabaseKind::Redis,
            kubernetes_kind,
            database_mode,
            localisation,
            is_public,
            ClusterDomain::Default {
                cluster_id: cluster_id.to_string(),
            },
            None,
            StorageSize::NormalSize,
        )
    })
}

// Redis self-hosted environment
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn redis_disk_resize() {
    test_deploy_an_environment_with_db_and_resize_disk(
        DatabaseKind::Redis,
        "6.2",
        function_name!(),
        KubernetesKind::Eks,
    )
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5.0", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5.0", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6.0", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore = "Save up AWS quotas `RulesPerSecurityGroupLimitExceeded: The maximum number of rules per security group has been reached.`. Testing public only on latest version."]
fn public_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6.0", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_redis_v7_deploy_a_working_dev_environment() {
    test_redis_configuration("7.0", function_name!(), CONTAINER, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_redis_v7_deploy_a_working_dev_environment() {
    test_redis_configuration("7.0", function_name!(), CONTAINER, KubernetesKind::Eks, true);
}

// Redis production environment (Elasticache)
#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
#[ignore = "No managed db deploy in this version"]
fn private_redis_v5_deploy_a_working_prod_environment() {
    test_redis_configuration("5.0", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
#[ignore]
fn private_redis_v6_deploy_a_working_prod_environment() {
    test_redis_configuration("6.0", function_name!(), MANAGED, KubernetesKind::Eks, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_redis_v7_deploy_a_working_prod_environment() {
    test_redis_configuration("7.0", function_name!(), MANAGED, KubernetesKind::Eks, false);
}
