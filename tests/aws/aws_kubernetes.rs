extern crate test_utilities;
use self::test_utilities::aws::{aws_access_key_id, aws_default_region, aws_secret_access_key};
use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{generate_id, init};
use gethostname;
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
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde_json::value::Value;
use std::borrow::Borrow;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::Command;
use std::{env, fs};
use test_utilities::aws::AWS_KUBERNETES_VERSION;

pub const QOVERY_ENGINE_REPOSITORY_URL: &str = "CHANGE-ME";
pub const TMP_DESTINATION_GIT: &str = "/tmp/qovery-engine-main/";
pub const GIT_LOGIN: &str = "CHANGE-ME";
pub const GIT_TOKEN: &str = "CHANGE-ME";

// avoid test collisions
fn generate_cluster_id(region: &str) -> String {
    let name = gethostname::gethostname().into_string();
    match name {
        // shrink to 15 chars in order to avoid resources name issues
        Ok(mut current_name) => {
            let mut shrink_size = 15;
            // avoid out of bounds issue
            if current_name.chars().count() < shrink_size {
                shrink_size = current_name.chars().count()
            }
            let mut final_name = format!("{}", &current_name[..shrink_size]);
            // do not end with a non alphanumeric char
            while !final_name.chars().last().unwrap().is_alphanumeric() {
                shrink_size -= 1;
                final_name = format!("{}", &current_name[..shrink_size]);
            }
            format!("{}-{}", final_name, region)
        },
        _ => generate_id(),
    }
}

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

    let region = "us-east-2";
    let kubernetes = EKS::new(
        context,
        generate_cluster_id(region).as_str(),
        generate_cluster_id(region).as_str(),
        AWS_KUBERNETES_VERSION,
        region,
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

    let region = "eu-west-3";
    let kubernetes = EKS::new(
        context.clone(),
        generate_cluster_id(region).as_str(),
        generate_cluster_id(region).as_str(),
        AWS_KUBERNETES_VERSION,
        region,
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

    let region = "us-east-2";
    let kubernetes = EKS::new(
        context,
        generate_cluster_id(region).as_str(),
        generate_cluster_id(region).as_str(),
        AWS_KUBERNETES_VERSION,
        region,
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

    let region = "eu-west-3";
    let kubernetes = EKS::new(
        context,
        generate_cluster_id(region).as_str(),
        generate_cluster_id(region).as_str(),
        AWS_KUBERNETES_VERSION,
        region,
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
