use std::fs::File;
use std::io::Read;

use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cmd::kubectl::create_sample_secret_terraform_in_namespace;
use qovery_engine::transaction::TransactionResult;
use test_utilities::aws::AWS_KUBERNETES_VERSION;
use test_utilities::cloudflare::dns_provider_cloudflare;
use test_utilities::utilities::{context, init};

pub fn do_not_delete_cluster_containing_tfstate() {
    init();
    // put some environments here, simulated or not

    let context = context();

    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let cloudflare = dns_provider_cloudflare(&context);

    let mut file = File::open("qovery-engine/tests/assets/eks-options.json").unwrap();
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

    /*
        create_sample_secret_terraform_in_namespace();
    */
    match tx.delete_kubernetes(&kubernetes) {
        Err(err) => panic!("{:?}", err),
        _ => {}
    }

    let _ = match tx.commit() {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
}
