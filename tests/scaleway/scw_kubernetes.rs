extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, generate_id, generate_cluster_id, init, FuncTestsSecrets};
use std::env;
use test_utilities::aws::AWS_KUBERNETES_VERSION;
use tracing::{span, Level};

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
