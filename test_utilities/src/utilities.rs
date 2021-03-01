use chrono::Utc;
use curl::easy::Easy;
use dirs::home_dir;
use std::fs::read_to_string;
use std::fs::File;
use std::io::{Error, ErrorKind, Write};
use std::path::Path;

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use retry::delay::Fibonacci;
use retry::OperationResult;
use std::env;
use std::os::unix::fs::PermissionsExt;
use tracing::info;
use tracing_subscriber;

use crate::aws::{aws_access_key_id, aws_secret_key, KUBE_CLUSTER_ID};
use hashicorp_vault;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cmd;
use qovery_engine::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use qovery_engine::error::{SimpleError, SimpleErrorKind};
use qovery_engine::models::{Context, Environment, Metadata};
use serde::{Deserialize, Serialize};
extern crate time;
use time::Instant;

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct VaultFuncTestsSecrets {
    pub AWS_ACCESS_KEY_ID: String,
    AWS_DEFAULT_REGION: String,
    AWS_SECRET_ACCESS_KEY: String,
    BIN_VERSION_FILE: String,
    CLOUDFLARE_DOMAIN: String,
    CLOUDFLARE_ID: String,
    CLOUDFLARE_TOKEN: String,
    CUSTOM_TEST_DOMAIN: String,
    DEFAULT_TEST_DOMAIN: String,
    DIGITAL_OCEAN_SPACES_ACCESS_ID: String,
    DIGITAL_OCEAN_SPACES_SECRET_ID: String,
    DIGITAL_OCEAN_TOKEN: String,
    DISCORD_API_URL: String,
    EKS_ACCESS_CIDR_BLOCKS: String,
    GITHUB_ACCESS_TOKEN: String,
    HTTP_LISTEN_ON: String,
    LETS_ENCRYPT_EMAIL_REPORT: String,
    LIB_ROOT_DIR: String,
    QOVERY_AGENT_CONTROLLER_TOKEN: String,
    QOVERY_API_URL: String,
    QOVERY_ENGINE_CONTROLLER_TOKEN: String,
    QOVERY_NATS_URL: String,
    QOVERY_SSH_USER: String,
    RUST_LOG: String,
    TERRAFORM_AWS_ACCESS_KEY_ID: String,
    TERRAFORM_AWS_SECRET_ACCESS_KEY: String,
}

struct VaultConfig {
    address: String,
    token: String,
}

impl VaultFuncTestsSecrets {
    pub fn new() -> Self {
        match Self::check_requirements() {
            Ok(vault_config) => Self::get_secrets_from_vault(vault_config),
            Err(e) => {
                println!("{}", e);
                Self::get_screts_from_env_var()
            }
        }
    }

    fn get_secret(&self) {
        self.AWS_ACCESS_KEY_ID
    }

    fn check_requirements() -> Result<VaultConfig, Error> {
        let vault_addr = match env::var_os("VAULT_ADDR") {
            Some(x) => x.into_string().unwrap(),
            None => {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("VAULT_ADDR environment variable is missing"),
                ))
            }
        };

        let vault_token = match env::var_os("VAULT_TOKEN") {
            Some(x) => x.into_string().unwrap(),
            None => {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("VAULT_TOKEN environment variable is missing"),
                ))
            }
        };

        Ok(VaultConfig {
            address: vault_addr,
            token: vault_token,
        })
    }

    fn check_env_var_exists(name: &str) -> String {
        match env::var_os(&name) {
            Some(x) => x.into_string().unwrap(),
            None => "".to_string(),
        }
    }

    fn get_secrets_from_vault(vault_config: VaultConfig) -> VaultFuncTestsSecrets {
        let client = hashicorp_vault::Client::new(vault_config.address, vault_config.token).unwrap();
        let res: Result<VaultFuncTestsSecrets, _> = client.get_custom_secret("functional-tests");
        match res {
            Ok(r) => r,
            Err(_) => {
                println!("can't contact Vault, fallback on environment variables");
                Self::get_screts_from_env_var()
            }
        }
    }

    fn get_screts_from_env_var() -> VaultFuncTestsSecrets {
        VaultFuncTestsSecrets {
            AWS_ACCESS_KEY_ID: Self::check_env_var_exists("AWS_ACCESS_KEY_ID"),
            AWS_DEFAULT_REGION: Self::check_env_var_exists("AWS_DEFAULT_REGION"),
            AWS_SECRET_ACCESS_KEY: Self::check_env_var_exists("AWS_SECRET_ACCESS_KEY"),
            BIN_VERSION_FILE: Self::check_env_var_exists("BIN_VERSION_FILE"),
            CLOUDFLARE_DOMAIN: Self::check_env_var_exists("CLOUDFLARE_DOMAIN"),
            CLOUDFLARE_ID: Self::check_env_var_exists("CLOUDFLARE_ID"),
            CLOUDFLARE_TOKEN: Self::check_env_var_exists("CLOUDFLARE_TOKEN"),
            CUSTOM_TEST_DOMAIN: Self::check_env_var_exists("CUSTOM_TEST_DOMAIN"),
            DEFAULT_TEST_DOMAIN: Self::check_env_var_exists("DEFAULT_TEST_DOMAIN"),
            DIGITAL_OCEAN_SPACES_ACCESS_ID: Self::check_env_var_exists("DIGITAL_OCEAN_SPACES_ACCESS_ID"),
            DIGITAL_OCEAN_SPACES_SECRET_ID: Self::check_env_var_exists("DIGITAL_OCEAN_SPACES_SECRET_ID"),
            DIGITAL_OCEAN_TOKEN: Self::check_env_var_exists("DIGITAL_OCEAN_TOKEN"),
            DISCORD_API_URL: Self::check_env_var_exists("DISCORD_API_URL"),
            EKS_ACCESS_CIDR_BLOCKS: Self::check_env_var_exists("EKS_ACCESS_CIDR_BLOCKS"),
            GITHUB_ACCESS_TOKEN: Self::check_env_var_exists("GITHUB_ACCESS_TOKEN"),
            HTTP_LISTEN_ON: Self::check_env_var_exists("HTTP_LISTEN_ON"),
            LETS_ENCRYPT_EMAIL_REPORT: Self::check_env_var_exists("LETS_ENCRYPT_EMAIL_REPORT"),
            LIB_ROOT_DIR: Self::check_env_var_exists("LIB_ROOT_DIR"),
            QOVERY_AGENT_CONTROLLER_TOKEN: Self::check_env_var_exists("QOVERY_AGENT_CONTROLLER_TOKEN"),
            QOVERY_API_URL: Self::check_env_var_exists("QOVERY_API_URL"),
            QOVERY_ENGINE_CONTROLLER_TOKEN: Self::check_env_var_exists("QOVERY_ENGINE_CONTROLLER_TOKEN"),
            QOVERY_NATS_URL: Self::check_env_var_exists("QOVERY_NATS_URL"),
            QOVERY_SSH_USER: Self::check_env_var_exists("QOVERY_SSH_USER"),
            RUST_LOG: Self::check_env_var_exists("RUST_LOG"),
            TERRAFORM_AWS_ACCESS_KEY_ID: Self::check_env_var_exists("TERRAFORM_AWS_ACCESS_KEY_ID"),
            TERRAFORM_AWS_SECRET_ACCESS_KEY: Self::check_env_var_exists("TERRAFORM_AWS_SECRET_ACCESS_KEY"),
        }
    }
}

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(context.clone(), "oxqlm3r99vwcmvuj", "qovery-local-docker")
}

pub fn init() -> Instant {
    // check if it's currently running on GitHub action or Gitlab CI, using a common env var
    let ci_var = "CI";

    let _ = match env::var_os(ci_var) {
        Some(_) => tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_current_span(false)
            .try_init(),
        None => tracing_subscriber::fmt().try_init(),
    };

    info!(
        "running from current directory: {}",
        std::env::current_dir().unwrap().to_str().unwrap()
    );

    Instant::now()
}

pub fn teardown(start_time: Instant, test_name: String) {
    let end = Instant::now();
    let elapsed = end - start_time;
    info!("{} seconds for test {}", elapsed.as_seconds_f64(), test_name);
}

pub fn engine_run_test<T>(test: T) -> ()
where
    T: FnOnce() -> String,
{
    let start = init();

    let test_name = test();

    teardown(start, test_name);
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
        Ok(_) => return true,

        Err(e) => {
            println!("TEST Error : while trying to call {}", e);
            return false;
        }
    }
}

pub fn context() -> Context {
    let execution_id = execution_id();
    let home_dir = std::env::var("WORKSPACE_ROOT_DIR").unwrap_or(home_dir().unwrap().to_str().unwrap().to_string());
    let lib_root_dir = std::env::var("LIB_ROOT_DIR").expect("LIB_ROOT_DIR is mandatory");
    let metadata = Metadata {
        dry_run_deploy: Option::from(false),
        resource_expiration_in_seconds: Some(2700),
    };

    Context::new(execution_id, home_dir, lib_root_dir, true, None, Option::from(metadata))
}

fn kubernetes_config_path(
    workspace_directory: &str,
    kubernetes_cluster_id: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<String, SimpleError> {
    let kubernetes_config_bucket_name = format!("qovery-kubeconfigs-{}", kubernetes_cluster_id);
    let kubernetes_config_object_key = format!("{}.yaml", kubernetes_cluster_id);

    let kubernetes_config_file_path = format!("{}/kubernetes_config_{}", workspace_directory, kubernetes_cluster_id);

    let _ = get_kubernetes_config_file(
        access_key_id,
        secret_access_key,
        kubernetes_config_bucket_name.as_str(),
        kubernetes_config_object_key.as_str(),
        kubernetes_config_file_path.as_str(),
    )?;

    Ok(kubernetes_config_file_path)
}

fn get_kubernetes_config_file<P>(
    access_key_id: &str,
    secret_access_key: &str,
    kubernetes_config_bucket_name: &str,
    kubernetes_config_object_key: &str,
    file_path: P,
) -> Result<File, SimpleError>
where
    P: AsRef<Path>,
{
    // return the file if it already exists
    let _ = match File::open(file_path.as_ref()) {
        Ok(f) => return Ok(f),
        Err(_) => {}
    };

    let file_content_result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
        let file_content = get_object_via_aws_cli(
            access_key_id,
            secret_access_key,
            kubernetes_config_bucket_name,
            kubernetes_config_object_key,
        );

        match file_content {
            Ok(file_content) => OperationResult::Ok(file_content),
            Err(err) => OperationResult::Retry(err),
        }
    });

    let file_content = match file_content_result {
        Ok(file_content) => file_content,
        Err(_) => {
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("file content is empty (retry failed multiple times) - which is not the expected content - what's wrong?"),
            ));
        }
    };

    let mut kubernetes_config_file = File::create(file_path.as_ref())?;
    let _ = kubernetes_config_file.write_all(file_content.as_bytes())?;
    // removes warning kubeconfig is (world/group) readable
    let metadata = kubernetes_config_file.metadata()?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o400);
    std::fs::set_permissions(file_path.as_ref(), permissions)?;
    Ok(kubernetes_config_file)
}

/// gets an aws s3 object using aws-cli
/// used as a failover when rusoto_s3 acts up
fn get_object_via_aws_cli(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
    object_key: &str,
) -> Result<String, SimpleError> {
    let s3_url = format!("s3://{}/{}", bucket_name, object_key);
    let local_path = format!("/tmp/{}", object_key); // FIXME: change hardcoded /tmp/

    qovery_engine::cmd::utilities::exec_with_envs(
        "aws",
        vec!["s3", "cp", &s3_url, &local_path],
        vec![
            (AWS_ACCESS_KEY_ID, access_key_id),
            (AWS_SECRET_ACCESS_KEY, secret_access_key),
        ],
    )?;

    let s = read_to_string(&local_path)?;
    Ok(s)
}

pub fn is_pod_restarted_aws_env(environment_check: Environment, pod_to_check: &str) -> (bool, String) {
    let namespace_name = format!(
        "{}-{}",
        &environment_check.project_id.clone(),
        &environment_check.id.clone(),
    );

    let access_key = aws_access_key_id();
    let secret_key = aws_secret_key();
    let aws_credentials_envs = vec![
        ("AWS_ACCESS_KEY_ID", access_key.as_str()),
        ("AWS_SECRET_ACCESS_KEY", secret_key.as_str()),
    ];

    let kubernetes_config = kubernetes_config_path(
        "/tmp",
        KUBE_CLUSTER_ID,
        aws_access_key_id().as_str(),
        aws_secret_key().as_str(),
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
