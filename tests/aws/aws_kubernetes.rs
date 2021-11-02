pub use ::function_name::named;
pub use tracing::{span, Level};

pub use crate::helpers::helpers_aws::{
    aws_kubernetes_nodes, cloud_provider_aws, docker_ecr_aws_engine, eks_options, AWS_KUBERNETES_VERSION,
};
pub use crate::helpers::helpers_cloudflare::dns_provider_cloudflare;
pub use crate::helpers::utilities::{context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets};
pub use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode::{WithNatGateways, WithoutNatGateways};
pub use qovery_engine::cloud_provider::aws::kubernetes::{Eks, VpcQoveryNetworkMode};
pub use qovery_engine::transaction::TransactionResult;

#[cfg(test)]
fn create_upgrade_and_destroy_eks_cluster(
    region: &str,
    secrets: FuncTestsSecrets,
    boot_version: &str,
    upgrade_to_version: &str,
    test_name: &str,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let engine = docker_ecr_aws_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let aws = cloud_provider_aws(&context);
        let nodes = aws_kubernetes_nodes();

        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = Eks::new(
            context.clone(),
            generate_cluster_id(region).as_str(),
            uuid::Uuid::new_v4(),
            generate_cluster_id(region).as_str(),
            boot_version,
            region,
            &aws,
            &cloudflare,
            eks_options(secrets.clone()),
            nodes.clone(),
        )
        .unwrap();

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // Upgrade
        let kubernetes = Eks::new(
            context,
            generate_cluster_id(region).as_str(),
            uuid::Uuid::new_v4(),
            generate_cluster_id(region).as_str(),
            upgrade_to_version,
            region,
            &aws,
            &cloudflare,
            eks_options(secrets),
            nodes,
        )
        .unwrap();
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        test_name.to_string()
    })
}

#[cfg(test)]
fn create_and_destroy_eks_cluster(
    region: &str,
    secrets: FuncTestsSecrets,
    test_infra_pause: bool,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();

        let engine = docker_ecr_aws_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let aws = cloud_provider_aws(&context);
        let nodes = aws_kubernetes_nodes();
        let mut eks_options = eks_options(secrets);
        eks_options.vpc_qovery_network_mode = vpc_network_mode;

        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = Eks::new(
            context,
            generate_cluster_id(region).as_str(),
            uuid::Uuid::new_v4(),
            generate_cluster_id(region).as_str(),
            AWS_KUBERNETES_VERSION,
            region,
            &aws,
            &cloudflare,
            eks_options,
            nodes,
        )
        .unwrap();

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if test_infra_pause {
            // Pause
            if let Err(err) = tx.pause_kubernetes(&kubernetes) {
                panic!("{:?}", err)
            }
            match tx.commit() {
                TransactionResult::Ok => {}
                TransactionResult::Rollback(_) => panic!(),
                TransactionResult::UnrecoverableError(_, _) => panic!(),
            };

            // Resume
            if let Err(err) = tx.create_kubernetes(&kubernetes) {
                panic!("{:?}", err)
            }
            let _ = match tx.commit() {
                TransactionResult::Ok => {}
                TransactionResult::Rollback(_) => panic!(),
                TransactionResult::UnrecoverableError(_, _) => panic!(),
            };
        }

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        test_name.to_string()
    })
}

/*
    TESTS NOTES:
    It is useful to keep 2 clusters deployment tests to run in // to validate there is no name collision (overlaping)
*/

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_without_nat_gw_in_eu_west_3() {
    let region = "eu-west-3";
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_eks_cluster(region, secrets, false, WithoutNatGateways, function_name!());
}

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_with_nat_gw_in_eu_west_3() {
    let region = "eu-west-3";
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_eks_cluster(region, secrets, false, WithNatGateways, function_name!());
}

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_in_us_east_2() {
    let region = "us-east-2";
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_eks_cluster(region, secrets, true, WithoutNatGateways, function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[ignore]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_in_eu_west_3() {
    let region = "eu-west-3";
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_eks_cluster(region, secrets, "1.18", "1.19", function_name!());
}
