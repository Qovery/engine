use curl::Error;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use crate::aws::{aws_access_key_id, aws_default_region, aws_secret_access_key, KUBE_CLUSTER_ID};

use chrono::Utc;
use curl::easy::Easy;
use digitalocean::error::Error::ReqwestError;
use dirs::home_dir;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::common;
use qovery_engine::cmd;
use qovery_engine::models::{Context, Environment, Metadata};
use reqwest::StatusCode;
use std::path::Path;

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(context.clone(), "oxqlm3r99vwcmvuj", "qovery-local-docker")
}

pub fn init() {
    env_logger::try_init();
    println!(
        "running from current directory: {}",
        std::env::current_dir().unwrap().to_str().unwrap()
    );
    let lib_root_dir =
        std::env::var("LIB_ROOT_DIR").expect("env var LIB_ROOT_DIR is mandatory");
    // check the lib root dir
    let path = format!("{}/helm-freeze.yaml", lib_root_dir);
    match Path::new(path.as_str()).exists() {
        false => {
            println!("Please check the LIB_ROOT_DIR env var, can't retrieve the helm-freeze file");
            assert!(false);
        }
        true => println!("LIB_ROOT_DIR env var seems to be ok"),
    }
}

pub fn generate_id() -> String {
    // Should follow DNS naming convention https://tools.ietf.org/html/rfc1035
    let uuid;

    loop {
        let rand_string: String = thread_rng().sample_iter(Alphanumeric).take(15).collect();
        if rand_string.chars().next().unwrap().is_alphabetic() {
            uuid = rand_string.to_lowercase();
            break;
        }
    }
    uuid
}

pub fn check_all_connections(env: &Environment) -> Vec<bool> {
    let mut checking: Vec<bool> = Vec::with_capacity(env.routers.len());

    for router_to_test in &env.routers {
        let path_to_test = format!(
            "https://{}{}",
            &router_to_test.default_domain, &router_to_test.routes[0].path
        );

        checking.push(curl_path(path_to_test.as_str()));
    }
    return checking;
}

fn curl_path(path: &str) -> bool {
    let mut easy = Easy::new();
    easy.url(path).unwrap();
    let res = easy.perform();
    match res {
        Ok(out) => return true,

        Err(e) => {
            println!("TEST Error : while trying to call {}", e);
            return false;
        }
    }
}

pub fn is_pod_restarted(environment_check: Environment, podToCheck: &str) -> (bool, String) {
    let namespace_name = format!(
        "{}-{}",
        &environment_check.project_id.clone(),
        &environment_check.id.clone(),
    );
    let access_key = aws_access_key_id();
    let secret_key = aws_secret_access_key();
    let aws_credentials_envs = vec![
        ("AWS_ACCESS_KEY_ID", access_key.as_str()),
        ("AWS_SECRET_ACCESS_KEY", secret_key.as_str()),
    ];

    let kubernetes_config = common::kubernetes_config_path(
        "/tmp",
        &environment_check.organization_id.as_str(),
        KUBE_CLUSTER_ID,
        aws_access_key_id().as_str(),
        aws_secret_access_key().as_str(),
        aws_default_region().as_str(),
    );
    match kubernetes_config {
        Ok(path) => {
            let restarted_database = cmd::kubectl::kubectl_exec_get_number_of_restart(
                path.as_str(),
                namespace_name.clone().as_str(),
                podToCheck,
                aws_credentials_envs,
            );
            match restarted_database {
                Ok(count) => match count.trim().eq("0") {
                    true => return (true, "0".to_string()),
                    false => return (true, count.to_string()),
                },
                _ => return (false, "".to_string()),
            }
        }
        Err(e) => return (false, "".to_string()),
    }
}

// curl it and compare the body result, invalid certs are accepted
pub fn curl_it_and_compare(path: &str, should_return_str: &str) -> Result<bool, reqwest::Error> {
    use reqwest::blocking::Client;
    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    let res = client.get(path).send();

    match res {
        Ok(output) => match output.status() {
            StatusCode::OK | StatusCode::PERMANENT_REDIRECT => {
                let returned = output.text().unwrap();
                match should_return_str.eq(returned.as_str()) {
                    true => Ok(true),
                    false => Ok(false),
                }
            }
            _ => Ok(false),
        },
        Err(e) => Err(e),
    }
}

pub fn context() -> Context {
    let execution_id = execution_id();
    let home_dir = std::env::var("WORKSPACE_ROOT_DIR")
        .unwrap_or(home_dir().unwrap().to_str().unwrap().to_string());
    let lib_root_dir = std::env::var("LIB_ROOT_DIR").expect("LIB_ROOT_DIR is mandatory");
    let metadata = Metadata {
        test: Option::from(true),
        dry_run_deploy: Option::from(true),
    };

    Context::new(
        execution_id.as_str(),
        home_dir.as_str(),
        lib_root_dir.as_str(),
        None,
        Option::from(metadata),
    )
}

pub fn execution_id() -> String {
    Utc::now()
        .to_rfc3339()
        .replace(":", "-")
        .replace(".", "-")
        .replace("+", "-")
}
