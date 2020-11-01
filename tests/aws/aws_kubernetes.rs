extern crate test_utilities;
use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{generate_id, init};
use log::{info, warn};
use qovery_engine::build_platform::GitRepository;
use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::Kubernetes;
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::git::Credentials;
use qovery_engine::models::{Clone2, GitCredentials};
use qovery_engine::transaction::TransactionResult;
use qovery_engine::{cmd, git};
use serde_json::value::Value;
use std::borrow::Borrow;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::Command;
use std::{env, fs};
use test_utilities::aws::AWS_KUBERNETES_VERSION;

pub const QOVERY_ENGINE_REPOSITORY_URL: &str = "CHANGE-ME";
pub const TMP_DESTINATION_GIT: &str = "/tmp/qovery-engine-master/";
pub const GIT_LOGIN: &str = "CHANGE-ME";
pub const GIT_TOKEN: &str = "CHANGE-ME";

#[test]
#[ignore]
fn create_and_upgrade_cluster_from_master_branch() {
    init();
    let tmp_dir = format!("{}{}", TMP_DESTINATION_GIT, generate_id());
    let current_path = env::current_dir().unwrap();
    fs::remove_dir_all(TMP_DESTINATION_GIT);
    let gr = GitRepository {
        url: QOVERY_ENGINE_REPOSITORY_URL.to_string(),
        credentials: Some(Credentials {
            login: GIT_LOGIN.to_string(),
            password: GIT_TOKEN.to_string(),
        }),
        commit_id: "".to_string(),
        dockerfile_path: "".to_string(),
    };
    fs::create_dir_all(tmp_dir.clone());
    info!("Cloning qovery-engine repository");
    let clone = git::clone(&gr.url, tmp_dir.clone(), &gr.credentials);
    match clone {
        Ok(repo) => info!("Well cloned qovery-engine repository"),
        Err(e) => {
            info!("error while cloning the qovery-engine repo {}", e);
            assert!(false);
        }
    }
    let tmp_qe = Path::new(&tmp_dir);
    assert!(env::set_current_dir(&tmp_qe).is_ok());
    info!("Building qovery-engine (could take some time...)");
    let cargo_build = Command::new("cargo").arg("build").output();
    match cargo_build {
        Err(e) => match e {
            _ => {
                warn!("cargo build error {:?}", e);
                assert!(false);
            }
        },
        Ok(o) => {
            info!("cargo build sucess {:?}", o);
            assert!(true);
        }
    };
    info!("Cargo test create eks cluster");
    let cargo_test = Command::new("cargo")
        .arg("test")
        .arg("--package")
        .arg("qovery-engine")
        .arg("--test")
        .arg("lib")
        .arg("aws::aws_environment::create_eks_cluster_in_eu_west_3")
        .arg("--")
        .arg("--exact")
        .env("LIB_ROOT_DIR", format!("{}/lib", &tmp_dir))
        .output();

    match cargo_test {
        Err(e) => match e {
            _ => assert!(false),
        },
        _ => assert!(true),
    };
    assert!(env::set_current_dir(&current_path).is_ok());
    create_eks_cluster_in_eu_west_3();
    delete_eks_cluster_in_eu_west_3();
}

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
