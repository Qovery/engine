extern crate test_utilities;

use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::Kubernetes;
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::transaction::TransactionResult;
use std::borrow::Borrow;
use test_utilities::AWS_KUBERNETES_VERSION;

#[test]
fn create_eks_cluster_in_us_east_2() {
    let execution_id = test_utilities::execution_id();

    let engine = test_utilities::docker_ecr_aws_engine(execution_id.as_str());
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::cloud_provider_aws(execution_id.as_str());
    let nodes = test_utilities::aws_kubernetes_nodes();

    let kubernetes = EKS::new(
        execution_id.as_str(),
        "my-eks-on-us-east-2",
        "my-eks-us-east-2",
        AWS_KUBERNETES_VERSION,
        "us-east-2",
        &aws,
        nodes,
    );

    tx.create_kubernetes(&kubernetes);

    match tx.commit() {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn create_eks_cluster_in_eu_west_3() {
    let execution_id = test_utilities::execution_id();

    let engine = test_utilities::docker_ecr_aws_engine(execution_id.as_str());
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::cloud_provider_aws(execution_id.as_str());
    let nodes = test_utilities::aws_kubernetes_nodes();

    let kubernetes = EKS::new(
        execution_id.as_str(),
        "my-eks-on-eu-west-3",
        "my-eks-eu-west-3",
        AWS_KUBERNETES_VERSION,
        "eu-west-3",
        &aws,
        nodes,
    );

    tx.create_kubernetes(&kubernetes);

    match tx.commit() {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn delete_eks_cluster_in_us_east_2() {
    let execution_id = test_utilities::execution_id();

    let engine = test_utilities::docker_ecr_aws_engine(execution_id.as_str());
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::cloud_provider_aws(execution_id.as_str());
    let nodes = test_utilities::aws_kubernetes_nodes();

    let kubernetes = EKS::new(
        execution_id.as_str(),
        "my-eks-on-us-east-2",
        "my-eks-us-east-2",
        AWS_KUBERNETES_VERSION,
        "us-east-2",
        &aws,
        nodes,
    );

    tx.delete_kubernetes(&kubernetes);

    match tx.commit() {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn delete_eks_cluster_in_eu_west_3() {
    let execution_id = test_utilities::execution_id();

    let engine = test_utilities::docker_ecr_aws_engine(execution_id.as_str());
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::cloud_provider_aws(execution_id.as_str());
    let nodes = test_utilities::aws_kubernetes_nodes();

    let kubernetes = EKS::new(
        execution_id.as_str(),
        "my-eks-on-eu-west-3",
        "my-eks-eu-west-3",
        AWS_KUBERNETES_VERSION,
        "eu-west-3",
        &aws,
        nodes,
    );

    tx.delete_kubernetes(&kubernetes);

    match tx.commit() {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}
