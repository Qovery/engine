extern crate test_utilities;
use self::test_utilities::aws::{aws_access_key_id, aws_default_region, aws_secret_access_key};
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
use test_utilities::utilities::context;

pub const QOVERY_ENGINE_REPOSITORY_URL: &str = "CHANGE-ME";
pub const TMP_DESTINATION_GIT: &str = "/tmp/qovery-engine-main/";
pub const GIT_LOGIN: &str = "CHANGE-ME";
pub const GIT_TOKEN: &str = "CHANGE-ME";

fn upgrade_new_cluster() {
    init();
    // create a cluster with last version of the engine
    let tmp_dir = format!("{}{}", TMP_DESTINATION_GIT, generate_id());
    let current_path = env::current_dir().unwrap();
    fs::remove_dir_all(TMP_DESTINATION_GIT);
    let gr = GitRepository {
        url: QOVERY_ENGINE_REPOSITORY_URL.to_string(),
        // this repo is public !
        credentials: None,
        commit_id: "".to_string(),
        dockerfile_path: "".to_string(),
    };
    fs::create_dir_all(tmp_dir.clone());
    info!("Cloning old engine repository");
    let clone = git::clone(&gr.url, tmp_dir.clone(), &gr.credentials);
    match clone {
        Ok(repo) => info!("Well clone engine repository"),
        Err(e) => {
            info!("error while cloning the engine repo {}", e);
            assert!(false);
        }
    }
    // should generate json file assets
    let cargo_test = Command::new("bash")
        .arg("helper.sh")
        .arg("prepare_tests")
        .output();
    match cargo_test {
        Err(e) => {
            info!("generating json in assets failed {:?}", e);
            assert!(false);
        }
        Ok(o) => {
            info!("generating json in assets successful {:?}", o);
            assert!(true);
        }
    };
    // copy it in the tmp project
    let cpy = fs::copy(
        format!("qovery-engine/tests/assets/eks-options.json"),
        format!("{}/tests/assets/eks-options.json", &tmp_dir),
    );
    match cpy {
        Ok(_) => {
            info!("copy json file OK");
            assert!(true);
        }
        Err(e) => {
            info!("copy json file NOT OK {:?}", e);
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
    env::set_var("LIB_ROOT_DIR", format!("{}/lib", &tmp_dir));
    env::set_var("AWS_ACCESS_KEY_ID", aws_access_key_id());
    env::set_var("AWS_SECRET_ACCESS_KEY", aws_secret_access_key());
    env::set_var("AWS_DEFAULT_REGION", aws_default_region());
    env::set_var("AWS_DEFAULT_REGION", "eu-west-3");
    env::set_var(
        "EKS_OPTIONS",
        format!("{}/tests/assets/eks-options.json", &tmp_dir),
    );

    let cargo_test = Command::new("cargo")
        .arg("test")
        .arg("--package")
        .arg("qovery-engine")
        .arg("--test")
        .arg("lib")
        .arg("aws::aws_kubernetes::create_eks_cluster_in_eu_west_3")
        .arg("--")
        .arg("--ignored")
        .arg("--exact")
        .output();
    match cargo_test {
        Err(e) => {
            info!("cargo test failed {:?}", e);
            assert!(false);
        }
        Ok(o) => {
            info!("cargo test sucess {:?}", o);
            assert!(true);
        }
    };
    assert!(env::set_current_dir(&current_path).is_ok());
    create_eks_cluster_in_eu_west_3();
    delete_eks_cluster_in_eu_west_3();
}

#[test]
#[ignore]
fn create_eks_cluster_in_us_east_2() {
    // PERMIT_CLUSTER_CREATION env variable prevent you to not spend money on unnecessary cluster creation
    match env::var("PERMIT_CLUSTER_CREATION") {
        Ok(s) => {}
        _ => return,
    }
    init();

    let context = context();

    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let aws = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let cloudflare = dns_provider_cloudflare(&context);

    let mut file = File::open(env::var("EKS_OPTIONS").unwrap()).unwrap();
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
    // PERMIT_CLUSTER_CREATION env variable prevent you to not spend money on unnecessary cluster creation
    match env::var("PERMIT_CLUSTER_CREATION") {
        Ok(s) => {}
        _ => return,
    }
    init();

    let context = context();

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

    let context = context();

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

    let context = context();

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
