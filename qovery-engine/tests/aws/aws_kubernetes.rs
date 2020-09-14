extern crate test_utilities;

use self::test_utilities::utilities::init;
use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::{Kubernetes, KubernetesError};
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::transaction::TransactionResult;
use std::borrow::Borrow;
use test_utilities::aws::AWS_KUBERNETES_VERSION;

#[test]
#[ignore]
fn create_eks_cluster_in_us_east_2() {
    init();

    let context = test_utilities::aws::context();

    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let kubernetes = EKS::new(
        context,
        "my-eks-on-us-east-2",
        "my-eks-us-east-2",
        AWS_KUBERNETES_VERSION,
        "us-east-2",
        &aws,
        nodes,
    );

    match tx.create_kubernetes(&kubernetes) {
        Err(err) => panic!("{:?}", err),
        _ => {}
    }

    let _ = match tx.commit() {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn create_eks_cluster_in_eu_west_3() {
    init();

    let context = test_utilities::aws::context();

    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let kubernetes = EKS::new(
        context,
        "my-eks-on-eu-west-3",
        "my-eks-eu-west-3",
        AWS_KUBERNETES_VERSION,
        "eu-west-3",
        &aws,
        nodes,
    );

    match tx.create_kubernetes(&kubernetes) {
        Err(err) => panic!("{:?}", err),
        _ => {}
    }

    let _ = match tx.commit() {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn delete_eks_cluster_in_us_east_2() {
    init();

    let context = test_utilities::aws::context();

    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let kubernetes = EKS::new(
        context,
        "my-eks-on-us-east-2",
        "my-eks-us-east-2",
        AWS_KUBERNETES_VERSION,
        "us-east-2",
        &aws,
        nodes,
    );

    match tx.delete_kubernetes(&kubernetes) {
        Err(err) => panic!("{:?}", err),
        _ => {}
    }

    let _ = match tx.commit() {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn delete_eks_cluster_in_eu_west_3() {
    init();

    let context = test_utilities::aws::context();

    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let kubernetes = EKS::new(
        context,
        "my-eks-on-eu-west-3",
        "my-eks-eu-west-3",
        AWS_KUBERNETES_VERSION,
        "eu-west-3",
        &aws,
        nodes,
    );

    match tx.delete_kubernetes(&kubernetes) {
        Err(err) => panic!("{:?}", err),
        _ => {}
    }

    let _ = match tx.commit() {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}
