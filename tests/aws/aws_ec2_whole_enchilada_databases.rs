use crate::helpers::utilities::{
    context_for_ec2, engine_run_test, generate_cluster_id, generate_id, init, logger, FuncTestsSecrets,
};
use function_name::named;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::{Kind as KubernetesKind, Kind, KubernetesVersion};
use qovery_engine::cloud_provider::models::CpuArchitecture;
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::io_models::database::{DatabaseKind, DatabaseMode};
use qovery_engine::transaction::{Transaction, TransactionResult};
use qovery_engine::utilities::to_short_id;
use tracing::{span, Level};

use crate::helpers;
use crate::helpers::common::{Cluster, ClusterDomain};
use crate::helpers::database::{test_db, StorageSize};

// By design, there is only one node instance for EC2 preventing to run in parallel database tests because of port clash.
// This file aims to create a dedicated EC2 cluster for publicly exposed managed DB tests.

#[allow(dead_code)]
fn test_ec2_database(
    test_name: &str,
    database_mode: DatabaseMode,
    database_kind: DatabaseKind,
    is_public: bool,
    db_version: &str,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();

        let logger = logger();
        let organization_id = generate_id();
        let localisation = match database_mode {
            DatabaseMode::MANAGED => secrets
                .AWS_EC2_TEST_MANAGED_REGION
                .expect("AWS_EC2_TEST_MANAGED_REGION is not set"),
            DatabaseMode::CONTAINER => secrets
                .AWS_EC2_TEST_CONTAINER_REGION
                .expect("AWS_EC2_TEST_CONTAINER_REGION is not set"),
        };
        let cluster_id = generate_cluster_id(&localisation);
        let context = context_for_ec2(organization_id, cluster_id);

        // create dedicated EC2 cluster:
        let secrets = FuncTestsSecrets::new();
        let attributed_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN must be set")
            .to_string();
        let cluster_domain = ClusterDomain::QoveryOwnedDomain {
            cluster_id: to_short_id(&cluster_id),
            domain: attributed_domain,
        };

        let infra_ctx = AWS::docker_cr_engine(
            &context,
            logger.clone(),
            &localisation,
            Kind::Ec2,
            KubernetesVersion::V1_25 {
                prefix: Some('v'.to_string()),
                patch: Some(11),
                suffix: Some("+k3s1".to_string()),
            },
            &cluster_domain,
            None,
            1,
            1,
            CpuArchitecture::AMD64,
            EngineLocation::QoverySide,
        );

        let mut deploy_tx = Transaction::new(&infra_ctx).unwrap();
        assert!(deploy_tx.create_kubernetes().is_ok());
        assert!(matches!(deploy_tx.commit(), TransactionResult::Ok));
        let environment = helpers::database::database_test_environment(&context);

        test_db(
            context,
            logger.clone(),
            environment,
            secrets,
            db_version,
            test_name,
            database_kind,
            KubernetesKind::Ec2,
            database_mode.clone(),
            localisation,
            is_public,
            cluster_domain,
            Some(&infra_ctx),
            StorageSize::NormalSize,
        )
    })
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn test_public_postgres_managed_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::MANAGED, DatabaseKind::Postgresql, true, "13")
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn test_public_mysql_managed_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::MANAGED, DatabaseKind::Mysql, true, "8.0")
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn test_private_postgres_managed_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::MANAGED, DatabaseKind::Postgresql, false, "13")
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn test_private_mysql_managed_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::MANAGED, DatabaseKind::Mysql, false, "8.0")
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn test_private_mongodb_managed_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::MANAGED, DatabaseKind::Mongodb, false, "4.4")
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn test_private_redis_managed_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::MANAGED, DatabaseKind::Redis, false, "6")
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
// #[named]
#[test]
#[ignore = "Public containered DBs are not supported on EC2, it's a known limitation"]
fn test_public_containered_dbs() {
    // test_ec2_database(function_name!(), DatabaseMode::CONTAINER, true, DbVersionsToTest::Latest);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn test_private_postgres_containered_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::CONTAINER, DatabaseKind::Postgresql, false, "13")
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn test_private_mysql_containered_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::CONTAINER, DatabaseKind::Mysql, false, "8.0")
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn test_private_mongodb_containered_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::CONTAINER, DatabaseKind::Mongodb, false, "4.4")
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn test_private_redis_containered_dbs() {
    test_ec2_database(function_name!(), DatabaseMode::CONTAINER, DatabaseKind::Redis, false, "6")
}
