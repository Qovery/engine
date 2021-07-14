extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets};
use test_utilities::aws::AWS_KUBERNETES_VERSION;
use tracing::{span, Level};

use self::test_utilities::aws::eks_options;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::transaction::TransactionResult;

#[allow(dead_code)]
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
        let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let aws = test_utilities::aws::cloud_provider_aws(&context);
        let nodes = test_utilities::aws::aws_kubernetes_nodes();

        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = EKS::new(
            context.clone(),
            generate_cluster_id(region).as_str(),
            generate_cluster_id(region).as_str(),
            boot_version,
            region,
            &aws,
            &cloudflare,
            eks_options(secrets.clone()),
            nodes.clone(),
        );

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Upgrade
        let kubernetes = EKS::new(
            context,
            generate_cluster_id(region).as_str(),
            generate_cluster_id(region).as_str(),
            upgrade_to_version,
            region,
            &aws,
            &cloudflare,
            eks_options(secrets),
            nodes,
        );
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    })
}

fn create_and_destroy_eks_cluster(region: &str, secrets: FuncTestsSecrets, test_infra_pause: bool, test_name: &str) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let aws = test_utilities::aws::cloud_provider_aws(&context);
        let nodes = test_utilities::aws::aws_kubernetes_nodes();

        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = EKS::new(
            context,
            generate_cluster_id(region).as_str(),
            generate_cluster_id(region).as_str(),
            AWS_KUBERNETES_VERSION,
            region,
            &aws,
            &cloudflare,
            eks_options(secrets),
            nodes,
        );

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if test_infra_pause {
            // Pause
            if let Err(err) = tx.pause_kubernetes(&kubernetes) {
                panic!("{:?}", err)
            }
            match tx.commit() {
                TransactionResult::Ok => assert!(true),
                TransactionResult::Rollback(_) => assert!(false),
                TransactionResult::UnrecoverableError(_, _) => assert!(false),
            };

            // Resume
            if let Err(err) = tx.create_kubernetes(&kubernetes) {
                panic!("{:?}", err)
            }
            let _ = match tx.commit() {
                TransactionResult::Ok => assert!(true),
                TransactionResult::Rollback(_) => assert!(false),
                TransactionResult::UnrecoverableError(_, _) => assert!(false),
            };
        }

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    })
}

/*
    TESTS NOTES:
    It is useful to keep 2 clusters deployment tests to run in // to validate there is no name collision (overlaping)
*/

#[cfg(feature = "test-aws-infra")]
#[test]
fn create_and_destroy_eks_cluster_in_eu_west_3() {
    let region = "eu-west-3";
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_eks_cluster(
        &region,
        secrets,
        false,
        &format!("create_and_destroy_eks_cluster_in_{}", region.replace("-", "_")),
    );
}

#[cfg(feature = "test-aws-infra")]
#[test]
fn create_and_destroy_eks_cluster_in_us_east_2() {
    let region = "us-east-2";
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_eks_cluster(
        &region,
        secrets,
        true,
        &format!("create_and_destroy_eks_cluster_in_{}", region.replace("-", "_")),
    );
}

// only enable this test manually when we want to perform and validate upgrade process
//#[test]
#[allow(dead_code)]
fn create_upgrade_and_destroy_eks_cluster_in_eu_west_3() {
    let region = "eu-west-3";
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_eks_cluster(
        &region,
        secrets,
        "1.16",
        "1.17",
        &format!("create_upgrade_and_destroy_eks_cluster_in_{}", region.replace("-", "_")),
    );
}
