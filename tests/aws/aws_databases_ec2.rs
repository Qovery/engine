extern crate test_utilities;

use ::function_name::named;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::io_models::{Action, CloneForTest, Database, DatabaseKind, DatabaseMode, Port, Protocol};
use test_utilities::aws::aws_default_engine_config;
use tracing::{span, Level};
use uuid::Uuid;

use self::test_utilities::aws::{AWS_DATABASE_DISK_TYPE, AWS_DATABASE_INSTANCE_TYPE};
use self::test_utilities::utilities::{
    context, engine_run_test, generate_id, get_pods, get_svc_name, init, is_pod_restarted_env, logger, FuncTestsSecrets,
};
use crate::aws::aws_databases::{
    test_mongodb_configuration, test_mysql_configuration, test_postgresql_configuration, test_redis_configuration,
};
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::io_models::DatabaseMode::{CONTAINER, MANAGED};
use qovery_engine::transaction::TransactionResult;
use qovery_engine::utilities::to_short_id;
use test_utilities::common::{test_db, Infrastructure};

/**
**
** PostgreSQL tests
**
**/

// Postgres environment environment
#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_postgresql_v10_deploy_a_working_dev_environment() {
    test_postgresql_configuration("10", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v10_deploy_a_working_dev_environment() {
    test_postgresql_configuration("10", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn public_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

// Postgres production environment
#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_postgresql_v10_deploy_a_working_prod_environment() {
    test_postgresql_configuration("10", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn public_postgresql_v10_deploy_a_working_prod_environment() {
    test_postgresql_configuration("10", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_prod_environment() {
    test_postgresql_configuration("11", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn public_postgresql_v11_deploy_a_working_prod_environment() {
    test_postgresql_configuration("11", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_prod_environment() {
    test_postgresql_configuration("12", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn public_postgresql_v12_deploy_a_working_prod_environment() {
    test_postgresql_configuration("12", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_prod_environment() {
    test_postgresql_configuration("13", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn public_postgresql_v13_deploy_a_working_prod_environment() {
    test_postgresql_configuration("13", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

/**
**
** MongoDB tests
**
**/

// development environment
#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_mongodb_v3_6_deploy_a_working_dev_environment() {
    test_mongodb_configuration("3.6", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v3_6_deploy_a_working_dev_environment() {
    test_mongodb_configuration("3.6", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.0", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v4_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.0", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn public_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

// MongoDB production environment (DocumentDB)
#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_mongodb_v3_6_deploy_a_working_prod_environment() {
    test_mongodb_configuration("3.6", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v3_6_deploy_a_working_prod_environment() {
    test_mongodb_configuration("3.6", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_mongodb_v4_0_deploy_a_working_prod_environment() {
    test_mongodb_configuration("4.0", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v4_0_deploy_a_working_prod_environment() {
    test_mongodb_configuration("4.0", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

/**
**
** MySQL tests
**
**/

// MySQL self-hosted environment
#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn public_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

// MySQL production environment (RDS)
#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_prod_environment() {
    test_mysql_configuration("5.7", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn public_mysql_v5_7_deploy_a_working_prod_environment() {
    test_mysql_configuration("5.7", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_mysql_v8_0_deploy_a_working_prod_environment() {
    test_mysql_configuration("8.0", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn public_mysql_v8_0_deploy_a_working_prod_environment() {
    test_mysql_configuration("8.0", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

/**
**
** Redis tests
**
**/

// Redis self-hosted environment
#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn private_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6", function_name!(), CONTAINER, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn public_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6", function_name!(), CONTAINER, KubernetesKind::Ec2, true);
}

// Redis production environment (Elasticache)
#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_prod_environment() {
    test_redis_configuration("5", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
#[ignore]
fn public_redis_v5_deploy_a_working_prod_environment() {
    test_redis_configuration("5", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
fn private_redis_v6_deploy_a_working_prod_environment() {
    test_redis_configuration("6", function_name!(), MANAGED, KubernetesKind::Ec2, false);
}

#[cfg(feature = "test-aws-ec2-managed-services")]
#[named]
#[test]
#[ignore]
fn public_redis_v6_deploy_a_working_prod_environment() {
    test_redis_configuration("6", function_name!(), MANAGED, KubernetesKind::Ec2, true);
}
