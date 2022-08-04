use crate::helpers::utilities::{
    context, engine_run_test, generate_cluster_id, generate_id, init, logger, FuncTestsSecrets,
};
use ::function_name::named;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::{Kind as KubernetesKind, Kind};
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::engine::EngineConfig;
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::database::{DatabaseKind, DatabaseMode};
use qovery_engine::logger::Logger;
use qovery_engine::transaction::{Transaction, TransactionResult};

use crate::helpers;
use crate::helpers::aws::AWS_TEST_REGION;
use crate::helpers::aws_ec2::AWS_K3S_VERSION;
use crate::helpers::common::{Cluster, ClusterDomain};
use crate::helpers::database::test_db;

// By design, there is only one node instance for EC2 preventing to run in parallel database tests because of port clash.
// This file aims to create a dedicated EC2 cluster for publicly exposed managed DB tests.

#[derive(Clone)]
#[allow(dead_code)]
enum DbVersionsToTest {
    AllSupported,
    LatestPublicManaged,
    LatestPrivateManaged,
}

#[allow(dead_code)]
fn test_ec2_postgres(
    context: Context,
    logger: Box<dyn Logger>,
    is_public: bool,
    database_mode: DatabaseMode,
    db_versions_to_test: &DbVersionsToTest,
    secrets: FuncTestsSecrets,
    cluster_domain: ClusterDomain,
    engine_config: &EngineConfig,
) {
    let environment = helpers::database::database_test_environment(&context);

    let test_name_accessibility = match is_public {
        true => "public",
        false => "private",
    };
    let test_name_mode = match database_mode {
        DatabaseMode::MANAGED => "prod",
        DatabaseMode::CONTAINER => "dev",
    };

    let postgres_versions_to_be_tested = match db_versions_to_test {
        DbVersionsToTest::AllSupported => vec!["14", "13", "12", "11"],
        DbVersionsToTest::LatestPublicManaged => vec!["14"],
        DbVersionsToTest::LatestPrivateManaged => vec!["14"],
    };
    for postgres_version in postgres_versions_to_be_tested {
        test_db(
            context.clone(),
            logger.clone(),
            environment.clone(),
            secrets.clone(),
            postgres_version,
            format!(
                "{}_postgresql_v{}_deploy_a_working_{}_environment",
                test_name_accessibility, postgres_version, test_name_mode
            )
            .as_str(),
            DatabaseKind::Postgresql,
            KubernetesKind::Ec2,
            database_mode.clone(),
            is_public,
            cluster_domain.clone(),
            Some(engine_config),
        );
    }
}

#[allow(dead_code)]
fn test_ec2_mongo(
    context: Context,
    logger: Box<dyn Logger>,
    is_public: bool,
    database_mode: DatabaseMode,
    db_versions_to_test: &DbVersionsToTest,
    secrets: FuncTestsSecrets,
    cluster_domain: ClusterDomain,
    engine_config: &EngineConfig,
) {
    let environment = helpers::database::database_test_environment(&context);

    let test_name_accessibility = match is_public {
        true => "public",
        false => "private",
    };
    let test_name_mode = match database_mode {
        DatabaseMode::MANAGED => "prod",
        DatabaseMode::CONTAINER => "dev",
    };

    let mongodb_versions_to_be_tested = match db_versions_to_test {
        DbVersionsToTest::AllSupported => vec!["4.4", "4.2", "4.0"],
        DbVersionsToTest::LatestPublicManaged => vec![],
        DbVersionsToTest::LatestPrivateManaged => vec!["4.0"],
    };
    for mongodb_version in mongodb_versions_to_be_tested {
        test_db(
            context.clone(),
            logger.clone(),
            environment.clone(),
            secrets.clone(),
            mongodb_version,
            format!(
                "{}_mongodb_v{}_deploy_a_working_{}_environment",
                test_name_accessibility, mongodb_version, test_name_mode
            )
            .as_str(),
            DatabaseKind::Mongodb,
            KubernetesKind::Ec2,
            database_mode.clone(),
            is_public,
            cluster_domain.clone(),
            Some(engine_config),
        );
    }
}

#[allow(dead_code)]
fn test_ec2_mysql(
    context: Context,
    logger: Box<dyn Logger>,
    is_public: bool,
    database_mode: DatabaseMode,
    db_versions_to_test: &DbVersionsToTest,
    secrets: FuncTestsSecrets,
    cluster_domain: ClusterDomain,
    engine_config: &EngineConfig,
) {
    let environment = helpers::database::database_test_environment(&context);

    let test_name_accessibility = match is_public {
        true => "public",
        false => "private",
    };
    let test_name_mode = match database_mode {
        DatabaseMode::MANAGED => "prod",
        DatabaseMode::CONTAINER => "dev",
    };

    let mysql_versions_to_be_tested = match db_versions_to_test {
        DbVersionsToTest::AllSupported => vec!["8.0", "5.7"],
        DbVersionsToTest::LatestPublicManaged => vec!["8.0"],
        DbVersionsToTest::LatestPrivateManaged => vec!["8.0"],
    };
    for mysql_version in mysql_versions_to_be_tested {
        test_db(
            context.clone(),
            logger.clone(),
            environment.clone(),
            secrets.clone(),
            mysql_version,
            format!(
                "{}_mysql_v{}_deploy_a_working_{}_environment",
                test_name_accessibility, mysql_version, test_name_mode
            )
            .as_str(),
            DatabaseKind::Mysql,
            KubernetesKind::Ec2,
            database_mode.clone(),
            is_public,
            cluster_domain.clone(),
            Some(engine_config),
        );
    }
}

#[allow(dead_code)]
fn test_ec2_redis(
    context: Context,
    logger: Box<dyn Logger>,
    is_public: bool,
    database_mode: DatabaseMode,
    db_versions_to_test: &DbVersionsToTest,
    secrets: FuncTestsSecrets,
    cluster_domain: ClusterDomain,
    engine_config: &EngineConfig,
) {
    let environment = helpers::database::database_test_environment(&context);

    let test_name_accessibility = match is_public {
        true => "public",
        false => "private",
    };
    let test_name_mode = match database_mode {
        DatabaseMode::MANAGED => "prod",
        DatabaseMode::CONTAINER => "dev",
    };

    let redis_versions_to_be_tested = match db_versions_to_test {
        DbVersionsToTest::AllSupported => vec!["7", "6", "5"],
        DbVersionsToTest::LatestPublicManaged => vec![],
        DbVersionsToTest::LatestPrivateManaged => vec!["6"],
    };
    for redis_version in redis_versions_to_be_tested {
        test_db(
            context.clone(),
            logger.clone(),
            environment.clone(),
            secrets.clone(),
            redis_version,
            format!(
                "{}_redis_v{}_deploy_a_working_{}_environment",
                test_name_accessibility, redis_version, test_name_mode
            )
            .as_str(),
            DatabaseKind::Redis,
            KubernetesKind::Ec2,
            database_mode.clone(),
            is_public,
            cluster_domain.clone(),
            Some(engine_config),
        );
    }
}

#[allow(dead_code)]
fn test_ec2_database(
    test_name: &str,
    database_mode: DatabaseMode,
    database_kind: DatabaseKind,
    is_public: bool,
    db_versions_to_test: DbVersionsToTest,
) {
    engine_run_test(|| {
        init();
        let logger = logger();
        let organization_id = generate_id();
        let cluster_id = generate_cluster_id(AWS_TEST_REGION.to_aws_format());
        let context = context(organization_id.as_str(), cluster_id.as_str());

        // create dedicated EC2 cluster:
        let secrets = FuncTestsSecrets::new();
        let attributed_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN must be set")
            .to_string();
        let cluster_domain = ClusterDomain::QoveryOwnedDomain {
            cluster_id,
            domain: attributed_domain,
        };

        let engine_config = AWS::docker_cr_engine(
            &context,
            logger.clone(),
            AWS_TEST_REGION.to_aws_format(),
            Kind::Ec2,
            AWS_K3S_VERSION.to_string(),
            &cluster_domain,
            None,
            1,
            1,
            EngineLocation::QoverySide,
        );

        let mut deploy_tx =
            Transaction::new(&engine_config, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
        assert!(deploy_tx.create_kubernetes().is_ok());
        assert!(matches!(deploy_tx.commit(), TransactionResult::Ok));

        match database_kind {
            DatabaseKind::Postgresql => test_ec2_postgres(
                context,
                logger.clone(),
                is_public,
                database_mode,
                &db_versions_to_test,
                secrets,
                cluster_domain,
                &engine_config,
            ),
            DatabaseKind::Mysql => test_ec2_mysql(
                context,
                logger.clone(),
                is_public,
                database_mode,
                &db_versions_to_test,
                secrets,
                cluster_domain,
                &engine_config,
            ),
            DatabaseKind::Mongodb => test_ec2_mongo(
                context,
                logger.clone(),
                is_public,
                database_mode,
                &db_versions_to_test,
                secrets,
                cluster_domain,
                &engine_config,
            ),
            DatabaseKind::Redis => test_ec2_redis(
                context,
                logger.clone(),
                is_public,
                database_mode,
                &db_versions_to_test,
                secrets,
                cluster_domain,
                &engine_config,
            ),
        };

        // Delete
        let mut delete_tx = Transaction::new(&engine_config, logger, Box::new(|| false), Box::new(|_| {})).unwrap();
        assert!(delete_tx.delete_kubernetes().is_ok());
        assert!(matches!(delete_tx.commit(), TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[test]
#[named]
fn test_public_postgres_managed_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::MANAGED,
        DatabaseKind::Postgresql,
        true,
        DbVersionsToTest::LatestPublicManaged,
    );
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[test]
#[named]
fn test_public_mysql_managed_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::MANAGED,
        DatabaseKind::Mysql,
        true,
        DbVersionsToTest::LatestPublicManaged,
    );
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[test]
#[named]
fn test_private_postgres_managed_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::MANAGED,
        DatabaseKind::Postgresql,
        false,
        DbVersionsToTest::AllSupported,
    );
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[test]
#[named]
fn test_private_mysql_managed_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::MANAGED,
        DatabaseKind::Mysql,
        false,
        DbVersionsToTest::AllSupported,
    );
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[test]
#[named]
fn test_private_mongodb_managed_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::MANAGED,
        DatabaseKind::Mongodb,
        false,
        DbVersionsToTest::AllSupported,
    );
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[test]
#[named]
fn test_private_redis_managed_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::MANAGED,
        DatabaseKind::Redis,
        false,
        DbVersionsToTest::AllSupported,
    );
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[test]
#[ignore = "Public containered DBs are not supported on EC2, it's a known limitation"]
fn test_public_containered_dbs() {
    // test_ec2_database(DatabaseMode::CONTAINER, true, DbVersionsToTest::Latest);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[test]
#[named]
fn test_private_postgres_containered_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::CONTAINER,
        DatabaseKind::Postgresql,
        false,
        DbVersionsToTest::AllSupported,
    );
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[test]
#[named]
fn test_private_mysql_containered_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::CONTAINER,
        DatabaseKind::Mysql,
        false,
        DbVersionsToTest::AllSupported,
    );
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[test]
#[named]
fn test_private_mongodb_containered_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::CONTAINER,
        DatabaseKind::Mongodb,
        false,
        DbVersionsToTest::AllSupported,
    );
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[test]
#[named]
fn test_private_redis_containered_dbs() {
    test_ec2_database(
        function_name!(),
        DatabaseMode::CONTAINER,
        DatabaseKind::Redis,
        false,
        DbVersionsToTest::AllSupported,
    );
}
