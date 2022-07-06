extern crate test_utilities;

use qovery_engine::cloud_provider::aws::AWS;

use qovery_engine::io_models::{DatabaseKind, DatabaseMode};

use self::test_utilities::utilities::{
    context, engine_run_test, generate_cluster_id, generate_id, logger, FuncTestsSecrets,
};
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::constants::AWS_DEFAULT_REGION;
use qovery_engine::transaction::Transaction;

use self::test_utilities::aws::AWS_TEST_REGION;
use self::test_utilities::aws_ec2::AWS_K3S_VERSION;
use self::test_utilities::common::{Cluster, ClusterDomain};
use qovery_engine::transaction::TransactionResult;
use test_utilities::common::test_db;

// By design, there is only one node instance for EC2 preventing to run in parallel database tests because of port clash.
// This file aims to create a dedicated EC2 cluster for publicly exposed managed DB tests.

#[derive(Clone)]
enum DbVersionsToTest {
    Latest,
    AllSupported,
}

#[allow(dead_code)]
fn test_ec2_database(database_mode: DatabaseMode, is_public: bool, db_versions_to_test: DbVersionsToTest) {
    engine_run_test(|| {
        // create dedicated EC2 cluster:
        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let organization_id = generate_id();
        let cluster_id = generate_cluster_id(AWS_DEFAULT_REGION);
        let context = context(organization_id.as_str(), cluster_id.as_str());
        let attributed_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN must be set")
            .to_string();
        let cluster_domain = ClusterDomain::QoveryOwnedDomain {
            cluster_id: cluster_id.to_string(),
            domain: attributed_domain.to_string(),
        };

        let engine_config = AWS::docker_cr_engine(
            &context,
            logger.clone(),
            AWS_TEST_REGION.to_aws_format().as_str(),
            KubernetesKind::Ec2,
            AWS_K3S_VERSION.to_string(),
            &cluster_domain,
            None,
            1,
            1,
            EngineLocation::QoverySide,
        );

        // deploy cluster:
        let mut deploy_tx =
            Transaction::new(&engine_config, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
        let mut delete_tx =
            Transaction::new(&engine_config, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();
        if let Err(err) = deploy_tx.create_kubernetes() {
            panic!("{:?}", err)
        }
        assert!(matches!(deploy_tx.commit(), TransactionResult::Ok));

        let environment = test_utilities::common::database_test_environment(&context);

        let test_name_accessibility = match is_public {
            true => "public",
            false => "private",
        };
        let test_name_mode = match database_mode {
            DatabaseMode::MANAGED => "prod",
            DatabaseMode::CONTAINER => "dev",
        };

        // PostgreSQL
        let postgres_versions_to_be_tested = match &db_versions_to_test {
            DbVersionsToTest::Latest => vec!["13"],
            DbVersionsToTest::AllSupported => vec!["13", "12", "11", "10"],
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
                Some(&engine_config),
            );
        }

        // MongoDB
        let mongodb_versions_to_be_tested = match &db_versions_to_test {
            DbVersionsToTest::Latest => vec!["4.0"],
            DbVersionsToTest::AllSupported => vec!["4.4", "4.2", "4.0", "3.6"],
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
                Some(&engine_config),
            );
        }

        // MySQL
        let mysql_versions_to_be_tested = match &db_versions_to_test {
            DbVersionsToTest::Latest => vec!["8.0"],
            DbVersionsToTest::AllSupported => vec!["8.0", "5.7"],
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
                Some(&engine_config),
            );
        }

        // Redis
        let redis_versions_to_be_tested = match &db_versions_to_test {
            DbVersionsToTest::Latest => vec!["6"],
            DbVersionsToTest::AllSupported => vec!["6", "5"],
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
                Some(&engine_config),
            );
        }

        // Delete
        if let Err(err) = delete_tx.delete_kubernetes() {
            panic!("{:?}", err)
        }
        assert!(matches!(delete_tx.commit(), TransactionResult::Ok));

        "OK".to_string()
    })
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[test]
fn test_public_managed_dbs() {
    // NOTE: this one can be long since it will test MySQL, Postgres, Redis and Mongo sequentially
    // and it's up to 20 minutes to provide such DBs AWS side.
    // Approx 80 minutes to complete
    test_ec2_database(DatabaseMode::MANAGED, true, DbVersionsToTest::Latest);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[test]
fn test_private_managed_dbs() {
    // NOTE: this one can be long since it will test MySQL, Postgres, Redis and Mongo sequentially
    // and it's up to 20 minutes to provide such DBs AWS side.
    // Approx 80 minutes to complete
    test_ec2_database(DatabaseMode::MANAGED, false, DbVersionsToTest::Latest);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[test]
#[ignore = "Public containered DBs are not supported on EC2, it's a known limitation"]
fn test_public_containered_dbs() {
    // test_ec2_database(DatabaseMode::CONTAINER, true, DbVersionsToTest::AllSupported);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[test]
fn test_private_containered_dbs() {
    test_ec2_database(DatabaseMode::CONTAINER, false, DbVersionsToTest::AllSupported);
}
