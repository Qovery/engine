use curl::easy::Easy;
use curl::Error;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::models::{Context, Environment, Metadata};
use chrono::Utc;
use dirs::home_dir;
use crate::aws::{aws_access_key_id, KUBE_CLUSTER_ID, aws_secret_access_key, aws_default_region};
use qovery_engine::cloud_provider::aws::common;
use qovery_engine::cmd;

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(context.clone(), "oxqlm3r99vwcmvuj", "qovery-local-docker")
}

pub fn init() {
    env_logger::try_init();
    println!(
        "running from current directory: {}",
        std::env::current_dir().unwrap().to_str().unwrap()
    );
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

pub fn context() -> Context {
    let execution_id = execution_id();
    let home_dir = std::env::var("WORKSPACE_ROOT_DIR")
        .unwrap_or(home_dir().unwrap().to_str().unwrap().to_string());
    let lib_root_dir = std::env::var("LIB_ROOT_DIR").expect("LIB_ROOT_DIR is mandatory");
    let metadata = Metadata {
        test: Option::from(true),
        dry_run_deploy: Option::from(false),
        resource_expiration_in_seconds: Some(2700),
    };

    Context::new(
        execution_id.as_str(),
        home_dir.as_str(),
        lib_root_dir.as_str(),
        None,
        Option::from(metadata),
    )
}

pub fn is_pod_restarted_aws_env(environment_check: Environment, pod_to_check: &str) -> (bool, String) {
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
                pod_to_check,
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
        Err(_e) => return (false, "".to_string()),
    }
}

pub fn execution_id() -> String {
    Utc::now()
        .to_rfc3339()
        .replace(":", "-")
        .replace(".", "-")
        .replace("+", "-")
}
