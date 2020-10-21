/*extern crate test_utilities;
use serde_json::value::Value;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::init;
use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::{Kubernetes, KubernetesError};
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Clone2;
use qovery_engine::transaction::TransactionResult;
use std::borrow::Borrow;
use std::fs::File;
use std::io::{BufReader, Read};
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

    let cloudflare = dns_provider_cloudflare(&context);

    let mut file = File::open("tests/assets/eks-options.json").unwrap();
    let mut read_buf = String::new();
    file.read_to_string(&mut read_buf).unwrap();

    let options_result = serde_json::from_str::<
        qovery_engine::cloud_provider::aws::kubernetes::Options,
    >(read_buf.as_str());

    let kubernetes = EKS::new(
        context,
        "eks-on-us-east-2",
        "eks-us-east-2",
        AWS_KUBERNETES_VERSION,
        "us-east-2",
        &aws,
        &cloudflare,
        options_result.expect("Oh my god an error in test... Options options options"),
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

pub fn read_file(filepath: &str) -> String {
    let file = File::open(filepath).expect("could not open file");
    let mut buffered_reader = BufReader::new(file);
    let mut contents = String::new();
    let _number_of_bytes: usize = match buffered_reader.read_to_string(&mut contents) {
        Ok(number_of_bytes) => number_of_bytes,
        Err(_err) => 0,
    };

    contents
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

    let cloudflare = dns_provider_cloudflare(&context);

    let mut file = File::open("tests/assets/eks-options.json").unwrap();
    let mut read_buf = String::new();
    file.read_to_string(&mut read_buf).unwrap();

    let options_result = serde_json::from_str::<
        qovery_engine::cloud_provider::aws::kubernetes::Options,
    >(read_buf.as_str());

    let kubernetes = EKS::new(
        context.clone(),
        "eks-on-eu-west-3",
        "eks-eu-west-3",
        AWS_KUBERNETES_VERSION,
        "eu-west-3",
        &aws,
        &cloudflare,
        options_result.expect("Oh my god an error in test... Options options options"),
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

    let cloudflare = dns_provider_cloudflare(&context);

    let mut file = File::open("tests/assets/eks-options.json").unwrap();
    let mut read_buf = String::new();
    file.read_to_string(&mut read_buf).unwrap();

    let options_result = serde_json::from_str::<
        qovery_engine::cloud_provider::aws::kubernetes::Options,
    >(read_buf.as_str());

    let kubernetes = EKS::new(
        context,
        "eks-on-us-east-2",
        "eks-us-east-2",
        AWS_KUBERNETES_VERSION,
        "us-east-2",
        &aws,
        &cloudflare,
        options_result.expect("Oh my god an error in test... Options options options"),
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
    // put some environments here, simulated or not

    let context = test_utilities::aws::context();

    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let cloudflare = dns_provider_cloudflare(&context);

    let mut file = File::open("tests/assets/eks-options.json").unwrap();
    let mut read_buf = String::new();
    file.read_to_string(&mut read_buf).unwrap();

    let options_result = serde_json::from_str::<
        qovery_engine::cloud_provider::aws::kubernetes::Options,
    >(read_buf.as_str());

    let kubernetes = EKS::new(
        context,
        "eks-on-eu-west-3",
        "eks-eu-west-3",
        AWS_KUBERNETES_VERSION,
        "eu-west-3",
        &aws,
        &cloudflare,
        options_result.expect("Oh my god an error in test... Options options options"),
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
*/
