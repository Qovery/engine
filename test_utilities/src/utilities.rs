extern crate base64;
extern crate bstr;
extern crate scaleway_api_rs;

use base64::decode;
use bstr::ByteSlice;
use chrono::Utc;
use curl::easy::Easy;
use dirs::home_dir;
use gethostname;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::str::FromStr;

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use retry::delay::Fibonacci;
use retry::OperationResult;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tracing::{error, info, warn};
use tracing_subscriber;

use hashicorp_vault;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::cmd;
use qovery_engine::constants::{
    AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, SCALEWAY_ACCESS_KEY, SCALEWAY_DEFAULT_PROJECT_ID, SCALEWAY_SECRET_KEY,
};
use qovery_engine::error::{SimpleError, SimpleErrorKind};
use qovery_engine::models::{Context, Environment, Features, Metadata};
use serde::{Deserialize, Serialize};
extern crate time;
use crate::scaleway::SCW_TEST_CLUSTER_ID;
use qovery_engine::cmd::structs::{KubernetesList, KubernetesPod};
use qovery_engine::object_storage::scaleway_object_storage::{BucketDeleteStrategy, ScalewayOS};
use qovery_engine::object_storage::ObjectStorage;
use qovery_engine::runtime::block_on;
use time::Instant;

pub fn context() -> Context {
    let execution_id = execution_id();
    let home_dir = std::env::var("WORKSPACE_ROOT_DIR").unwrap_or(home_dir().unwrap().to_str().unwrap().to_string());
    let lib_root_dir = std::env::var("LIB_ROOT_DIR").expect("LIB_ROOT_DIR is mandatory");

    let metadata = Metadata {
        dry_run_deploy: Option::from({
            match env::var_os("dry_run_deploy") {
                Some(_) => true,
                None => false,
            }
        }),
        resource_expiration_in_seconds: {
            // set a custom ttl as environment variable for manual tests
            match env::var_os("ttl") {
                Some(ttl) => {
                    let ttl_converted: u32 = ttl.into_string().unwrap().parse().unwrap();
                    Some(ttl_converted)
                }
                None => Some(7200),
            }
        },
        docker_build_options: Some("--network host".to_string()),
        forced_upgrade: Option::from({
            match env::var_os("forced_upgrade") {
                Some(_) => true,
                None => false,
            }
        }),
        disable_pleco: Some(true),
    };

    let enabled_features = vec![Features::LogsHistory, Features::MetricsHistory];

    Context::new(
        execution_id,
        home_dir,
        lib_root_dir,
        true,
        None,
        enabled_features,
        Option::from(metadata),
    )
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(non_snake_case)]
pub struct FuncTestsSecrets {
    pub AWS_ACCESS_KEY_ID: Option<String>,
    pub AWS_DEFAULT_REGION: Option<String>,
    pub AWS_SECRET_ACCESS_KEY: Option<String>,
    pub BIN_VERSION_FILE: Option<String>,
    pub CLOUDFLARE_DOMAIN: Option<String>,
    pub CLOUDFLARE_ID: Option<String>,
    pub CLOUDFLARE_TOKEN: Option<String>,
    pub CUSTOM_TEST_DOMAIN: Option<String>,
    pub DEFAULT_TEST_DOMAIN: Option<String>,
    pub DIGITAL_OCEAN_SPACES_ACCESS_ID: Option<String>,
    pub DIGITAL_OCEAN_SPACES_SECRET_ID: Option<String>,
    pub DIGITAL_OCEAN_TOKEN: Option<String>,
    pub DISCORD_API_URL: Option<String>,
    pub EKS_ACCESS_CIDR_BLOCKS: Option<String>,
    pub GITHUB_ACCESS_TOKEN: Option<String>,
    pub HTTP_LISTEN_ON: Option<String>,
    pub LETS_ENCRYPT_EMAIL_REPORT: Option<String>,
    pub LIB_ROOT_DIR: Option<String>,
    pub QOVERY_AGENT_CONTROLLER_TOKEN: Option<String>,
    pub QOVERY_API_URL: Option<String>,
    pub QOVERY_ENGINE_CONTROLLER_TOKEN: Option<String>,
    pub QOVERY_NATS_URL: Option<String>,
    pub QOVERY_NATS_USERNAME: Option<String>,
    pub QOVERY_NATS_PASSWORD: Option<String>,
    pub QOVERY_SSH_USER: Option<String>,
    pub RUST_LOG: Option<String>,
    pub SCALEWAY_DEFAULT_PROJECT_ID: Option<String>,
    pub SCALEWAY_ACCESS_KEY: Option<String>,
    pub SCALEWAY_SECRET_KEY: Option<String>,
    pub SCALEWAY_DEFAULT_REGION: Option<String>,
    pub TERRAFORM_AWS_ACCESS_KEY_ID: Option<String>,
    pub TERRAFORM_AWS_SECRET_ACCESS_KEY: Option<String>,
    pub TERRAFORM_AWS_REGION: Option<String>,
}

struct VaultConfig {
    address: String,
    token: String,
}

impl FuncTestsSecrets {
    pub fn new() -> Self {
        Self::get_all_secrets()
    }

    fn get_vault_config() -> Result<VaultConfig, Error> {
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

    fn get_secrets_from_vault() -> FuncTestsSecrets {
        let secret_name = "functional-tests";
        let empty_secrets = FuncTestsSecrets {
            AWS_ACCESS_KEY_ID: None,
            AWS_DEFAULT_REGION: None,
            AWS_SECRET_ACCESS_KEY: None,
            BIN_VERSION_FILE: None,
            CLOUDFLARE_DOMAIN: None,
            CLOUDFLARE_ID: None,
            CLOUDFLARE_TOKEN: None,
            CUSTOM_TEST_DOMAIN: None,
            DEFAULT_TEST_DOMAIN: None,
            DIGITAL_OCEAN_SPACES_ACCESS_ID: None,
            DIGITAL_OCEAN_SPACES_SECRET_ID: None,
            DIGITAL_OCEAN_TOKEN: None,
            DISCORD_API_URL: None,
            EKS_ACCESS_CIDR_BLOCKS: None,
            GITHUB_ACCESS_TOKEN: None,
            HTTP_LISTEN_ON: None,
            LETS_ENCRYPT_EMAIL_REPORT: None,
            LIB_ROOT_DIR: None,
            QOVERY_AGENT_CONTROLLER_TOKEN: None,
            QOVERY_API_URL: None,
            QOVERY_ENGINE_CONTROLLER_TOKEN: None,
            QOVERY_NATS_URL: None,
            QOVERY_NATS_USERNAME: None,
            QOVERY_NATS_PASSWORD: None,
            QOVERY_SSH_USER: None,
            RUST_LOG: None,
            SCALEWAY_ACCESS_KEY: None,
            SCALEWAY_DEFAULT_PROJECT_ID: None,
            SCALEWAY_SECRET_KEY: None,
            SCALEWAY_DEFAULT_REGION: None,
            TERRAFORM_AWS_ACCESS_KEY_ID: None,
            TERRAFORM_AWS_SECRET_ACCESS_KEY: None,
            TERRAFORM_AWS_REGION: None,
        };

        let vault_config = match Self::get_vault_config() {
            Ok(vault_config) => vault_config,
            Err(_) => {
                warn!("Empty config is returned as no VAULT connection can be established. If not not expected, check your environment variables");
                return empty_secrets;
            }
        };

        let client = match hashicorp_vault::Client::new(vault_config.address, vault_config.token) {
            Ok(x) => x,
            Err(e) => {
                println!("error: wasn't able to contact Vault server. {:?}", e);
                return empty_secrets;
            }
        };
        let res: Result<FuncTestsSecrets, _> = client.get_custom_secret(secret_name);
        match res {
            Ok(x) => x,
            Err(_) => {
                println!("Couldn't connect to Vault, check your connectivity");
                empty_secrets
            }
        }
    }

    fn select_secret(name: &str, vault_fallback: Option<String>) -> Option<String> {
        match env::var_os(&name) {
            Some(x) => Some(x.into_string().unwrap()),
            None if vault_fallback.is_some() => vault_fallback,
            None => None,
        }
    }

    fn get_all_secrets() -> FuncTestsSecrets {
        let secrets = Self::get_secrets_from_vault();

        FuncTestsSecrets {
            AWS_ACCESS_KEY_ID: Self::select_secret("AWS_ACCESS_KEY_ID", secrets.AWS_ACCESS_KEY_ID),
            AWS_DEFAULT_REGION: Self::select_secret("AWS_DEFAULT_REGION", secrets.AWS_DEFAULT_REGION),
            AWS_SECRET_ACCESS_KEY: Self::select_secret("AWS_SECRET_ACCESS_KEY", secrets.AWS_SECRET_ACCESS_KEY),
            BIN_VERSION_FILE: Self::select_secret("BIN_VERSION_FILE", secrets.BIN_VERSION_FILE),
            CLOUDFLARE_DOMAIN: Self::select_secret("CLOUDFLARE_DOMAIN", secrets.CLOUDFLARE_DOMAIN),
            CLOUDFLARE_ID: Self::select_secret("CLOUDFLARE_ID", secrets.CLOUDFLARE_ID),
            CLOUDFLARE_TOKEN: Self::select_secret("CLOUDFLARE_TOKEN", secrets.CLOUDFLARE_TOKEN),
            CUSTOM_TEST_DOMAIN: Self::select_secret("CUSTOM_TEST_DOMAIN", secrets.CUSTOM_TEST_DOMAIN),
            DEFAULT_TEST_DOMAIN: Self::select_secret("DEFAULT_TEST_DOMAIN", secrets.DEFAULT_TEST_DOMAIN),
            DIGITAL_OCEAN_SPACES_ACCESS_ID: Self::select_secret(
                "DIGITAL_OCEAN_SPACES_ACCESS_ID",
                secrets.DIGITAL_OCEAN_SPACES_ACCESS_ID,
            ),
            DIGITAL_OCEAN_SPACES_SECRET_ID: Self::select_secret(
                "DIGITAL_OCEAN_SPACES_SECRET_ID",
                secrets.DIGITAL_OCEAN_SPACES_SECRET_ID,
            ),
            DIGITAL_OCEAN_TOKEN: Self::select_secret("DIGITAL_OCEAN_TOKEN", secrets.DIGITAL_OCEAN_TOKEN),
            DISCORD_API_URL: Self::select_secret("DISCORD_API_URL", secrets.DISCORD_API_URL),
            EKS_ACCESS_CIDR_BLOCKS: Self::select_secret("EKS_ACCESS_CIDR_BLOCKS", secrets.EKS_ACCESS_CIDR_BLOCKS),
            GITHUB_ACCESS_TOKEN: Self::select_secret("GITHUB_ACCESS_TOKEN", secrets.GITHUB_ACCESS_TOKEN),
            HTTP_LISTEN_ON: Self::select_secret("HTTP_LISTEN_ON", secrets.HTTP_LISTEN_ON),
            LETS_ENCRYPT_EMAIL_REPORT: Self::select_secret(
                "LETS_ENCRYPT_EMAIL_REPORT",
                secrets.LETS_ENCRYPT_EMAIL_REPORT,
            ),
            LIB_ROOT_DIR: Self::select_secret("LIB_ROOT_DIR", secrets.LIB_ROOT_DIR),
            QOVERY_AGENT_CONTROLLER_TOKEN: Self::select_secret(
                "QOVERY_AGENT_CONTROLLER_TOKEN",
                secrets.QOVERY_AGENT_CONTROLLER_TOKEN,
            ),
            QOVERY_API_URL: Self::select_secret("QOVERY_API_URL", secrets.QOVERY_API_URL),
            QOVERY_ENGINE_CONTROLLER_TOKEN: Self::select_secret(
                "QOVERY_ENGINE_CONTROLLER_TOKEN",
                secrets.QOVERY_ENGINE_CONTROLLER_TOKEN,
            ),
            QOVERY_NATS_URL: Self::select_secret("QOVERY_NATS_URL", secrets.QOVERY_NATS_URL),
            QOVERY_NATS_USERNAME: Self::select_secret("QOVERY_NATS_USERNAME", secrets.QOVERY_NATS_USERNAME),
            QOVERY_NATS_PASSWORD: Self::select_secret("QOVERY_NATS_PASSWORD", secrets.QOVERY_NATS_PASSWORD),
            QOVERY_SSH_USER: Self::select_secret("QOVERY_SSH_USER", secrets.QOVERY_SSH_USER),
            RUST_LOG: Self::select_secret("RUST_LOG", secrets.RUST_LOG),
            SCALEWAY_ACCESS_KEY: Self::select_secret("SCALEWAY_ACCESS_KEY", secrets.SCALEWAY_ACCESS_KEY),
            SCALEWAY_DEFAULT_PROJECT_ID: Self::select_secret(
                "SCALEWAY_DEFAULT_PROJECT_ID",
                secrets.SCALEWAY_DEFAULT_PROJECT_ID,
            ),
            SCALEWAY_SECRET_KEY: Self::select_secret("SCALEWAY_SECRET_KEY", secrets.SCALEWAY_SECRET_KEY),
            SCALEWAY_DEFAULT_REGION: Self::select_secret("SCALEWAY_DEFAULT_REGION", secrets.SCALEWAY_DEFAULT_REGION),
            TERRAFORM_AWS_ACCESS_KEY_ID: Self::select_secret(
                "TERRAFORM_AWS_ACCESS_KEY_ID",
                secrets.TERRAFORM_AWS_ACCESS_KEY_ID,
            ),
            TERRAFORM_AWS_SECRET_ACCESS_KEY: Self::select_secret(
                "TERRAFORM_AWS_SECRET_ACCESS_KEY",
                secrets.TERRAFORM_AWS_SECRET_ACCESS_KEY,
            ),
            TERRAFORM_AWS_REGION: Self::select_secret("TERRAFORM_AWS_REGION", secrets.TERRAFORM_AWS_REGION),
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

fn kubernetes_config_path(
    provider_kind: Kind,
    context: Context,
    workspace_directory: &str,
    kubernetes_cluster_id: &str,
    secrets: FuncTestsSecrets,
) -> Result<String, SimpleError> {
    let kubernetes_config_bucket_name = format!("qovery-kubeconfigs-{}", kubernetes_cluster_id);
    let kubernetes_config_object_key = format!("{}.yaml", kubernetes_cluster_id);
    let kubernetes_config_file_path = format!("{}/kubernetes_config_{}", workspace_directory, kubernetes_cluster_id);

    let _ = get_kubernetes_config_file(
        provider_kind,
        context,
        kubernetes_config_bucket_name,
        kubernetes_config_object_key,
        kubernetes_config_file_path.clone(),
        secrets.clone(),
    )?;

    Ok(kubernetes_config_file_path)
}

fn get_kubernetes_config_file<P>(
    provider_kind: Kind,
    context: Context,
    kubernetes_config_bucket_name: String,
    kubernetes_config_object_key: String,
    file_path: P,
    secrets: FuncTestsSecrets,
) -> Result<fs::File, SimpleError>
where
    P: AsRef<Path>,
{
    // return the file if it already exists and should use cache
    let _ = match fs::File::open(file_path.as_ref()) {
        Ok(f) => return Ok(f),
        Err(_) => {}
    };

    let file_content_result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
        let file_content = match provider_kind {
            Kind::Aws => {
                let access_key_id = secrets.clone().AWS_ACCESS_KEY_ID.unwrap();
                let secret_access_key = secrets.clone().AWS_SECRET_ACCESS_KEY.unwrap();

                aws_s3_get_object(
                    access_key_id.as_str(),
                    secret_access_key.as_str(),
                    kubernetes_config_bucket_name.as_str(),
                    kubernetes_config_object_key.as_str(),
                )
            }
            Kind::Do => todo!(),
            Kind::Scw => {
                // TODO(benjaminch): refactor all of this properly
                let access_key_id = secrets.clone().SCALEWAY_ACCESS_KEY.unwrap();
                let secret_access_key = secrets.clone().SCALEWAY_SECRET_KEY.unwrap();
                let region = Region::from_str(secrets.clone().SCALEWAY_DEFAULT_REGION.unwrap().as_str()).unwrap();
                let project_id = secrets.clone().SCALEWAY_DEFAULT_PROJECT_ID.unwrap();

                let configuration = scaleway_api_rs::apis::configuration::Configuration {
                    api_key: Some(scaleway_api_rs::apis::configuration::ApiKey {
                        key: secret_access_key.to_string(),
                        prefix: None,
                    }),
                    ..scaleway_api_rs::apis::configuration::Configuration::default()
                };

                let clusters_res = block_on(scaleway_api_rs::apis::clusters_api::list_clusters(
                    &configuration,
                    region.to_string().as_str(),
                    None,
                    Some(project_id.as_str()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                ));

                if let Err(e) = clusters_res {
                    let message = format!("error while trying to get clusters, error: {}", e.to_string());
                    error!("{}", message);

                    return OperationResult::Retry(SimpleError::new(SimpleErrorKind::Other, Some(message.as_str())));
                }

                let clusters = clusters_res.unwrap();

                if clusters.clusters.is_none() {
                    let message = "error while trying to get clusters, error: no clusters found";
                    error!("{}", message);

                    return OperationResult::Retry(SimpleError::new(SimpleErrorKind::Other, Some(message)));
                }

                let clusters = clusters.clusters.unwrap();
                let expected_test_server_tag = format!("ClusterId={}", SCW_TEST_CLUSTER_ID);

                for cluster in clusters.iter() {
                    if cluster.tags.is_some() {
                        for tag in cluster.tags.as_ref().unwrap().iter() {
                            if tag.as_str() == expected_test_server_tag.as_str() {
                                return match block_on(scaleway_api_rs::apis::clusters_api::get_cluster_kube_config(
                                    &configuration,
                                    region.as_str(),
                                    cluster.id.as_ref().unwrap().as_str(),
                                )) {
                                    Ok(res) => {
                                        return OperationResult::Ok(
                                            base64::decode(res.content.unwrap())
                                                .unwrap()
                                                .to_str()
                                                .unwrap()
                                                .to_string(),
                                        );
                                    }
                                    Err(e) => {
                                        let message =
                                            format!("error while trying to get clusters, error: {}", e.to_string());
                                        error!("{}", message);

                                        return OperationResult::Retry(SimpleError::new(
                                            SimpleErrorKind::Other,
                                            Some(message.as_str()),
                                        ));
                                    }
                                };
                            }
                        }
                    }
                }

                Err(SimpleError::new(SimpleErrorKind::Other, Some("Test cluster not found")))
            }
        };

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

    let mut kubernetes_config_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(file_path.as_ref())?;
    let _ = kubernetes_config_file.write_all(file_content.as_bytes())?;
    // removes warning kubeconfig is (world/group) readable
    let mut perms = fs::metadata(file_path.as_ref())?.permissions();
    perms.set_readonly(false);
    fs::set_permissions(file_path.as_ref(), perms)?;
    Ok(kubernetes_config_file)
}

type KubernetesCredentials<'a> = Vec<(&'a str, &'a str)>;

fn get_cloud_provider_credentials<'a>(provider_kind: Kind, secrets: &'a FuncTestsSecrets) -> KubernetesCredentials<'a> {
    match provider_kind {
        Kind::Aws => vec![
            (AWS_ACCESS_KEY_ID, secrets.AWS_ACCESS_KEY_ID.as_ref().unwrap().as_str()),
            (
                AWS_SECRET_ACCESS_KEY,
                secrets.AWS_SECRET_ACCESS_KEY.as_ref().unwrap().as_str(),
            ),
        ],
        Kind::Do => todo!(),
        Kind::Scw => vec![
            (
                SCALEWAY_ACCESS_KEY,
                secrets.SCALEWAY_ACCESS_KEY.as_ref().unwrap().as_str(),
            ),
            (
                SCALEWAY_SECRET_KEY,
                secrets.SCALEWAY_SECRET_KEY.as_ref().unwrap().as_str(),
            ),
            (
                SCALEWAY_DEFAULT_PROJECT_ID,
                secrets.SCALEWAY_DEFAULT_PROJECT_ID.as_ref().unwrap().as_str(),
            ),
        ],
    }
}

fn aws_s3_get_object(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
    object_key: &str,
) -> Result<String, SimpleError> {
    let local_path = format!("/tmp/{}", object_key); // FIXME: change hardcoded /tmp/

    // gets an aws s3 object using aws-cli
    // used as a failover when rusoto_s3 acts up
    let s3_url = format!("s3://{}/{}", bucket_name, object_key);

    qovery_engine::cmd::utilities::exec(
        "aws",
        vec!["s3", "cp", &s3_url, &local_path],
        &vec![
            (AWS_ACCESS_KEY_ID, access_key_id),
            (AWS_SECRET_ACCESS_KEY, secret_access_key),
        ],
    )?;

    let s = fs::read_to_string(&local_path)?;

    Ok(s)
}

pub fn is_pod_restarted_env(
    provider_kind: Kind,
    context: Context,
    kube_cluster_id: &str,
    environment_check: Environment,
    pod_to_check: &str,
    secrets: FuncTestsSecrets,
) -> (bool, String) {
    let namespace_name = format!(
        "{}-{}",
        &environment_check.project_id.clone(),
        &environment_check.id.clone(),
    );

    let kubernetes_config =
        kubernetes_config_path(provider_kind.clone(), context, "/tmp", kube_cluster_id, secrets.clone());

    match kubernetes_config {
        Ok(path) => {
            let restarted_database = cmd::kubectl::kubectl_exec_get_number_of_restart(
                path.as_str(),
                namespace_name.clone().as_str(),
                pod_to_check,
                get_cloud_provider_credentials(provider_kind.clone(), &secrets.clone()),
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

pub fn get_pods(
    provider_kind: Kind,
    context: Context,
    environment_check: Environment,
    pod_to_check: &str,
    kube_cluster_id: &str,
    secrets: FuncTestsSecrets,
) -> Result<KubernetesList<KubernetesPod>, SimpleError> {
    let namespace_name = format!(
        "{}-{}",
        &environment_check.project_id.clone(),
        &environment_check.id.clone(),
    );

    let kubernetes_config =
        kubernetes_config_path(provider_kind.clone(), context, "/tmp", kube_cluster_id, secrets.clone());

    cmd::kubectl::kubectl_exec_get_pod(
        kubernetes_config.unwrap().as_str(),
        namespace_name.clone().as_str(),
        pod_to_check,
        get_cloud_provider_credentials(provider_kind.clone(), &secrets.clone()),
    )
}

pub fn execution_id() -> String {
    Utc::now()
        .to_rfc3339()
        .replace(":", "-")
        .replace(".", "-")
        .replace("+", "-")
}

// avoid test collisions
pub fn generate_cluster_id(region: &str) -> String {
    let check_if_running_on_gitlab_env_var = "CI_PROJECT_TITLE";

    // if running on CI, generate an ID
    match env::var_os(check_if_running_on_gitlab_env_var) {
        None => {}
        Some(_) => return generate_id(),
    };

    match gethostname::gethostname().into_string() {
        // shrink to 15 chars in order to avoid resources name issues
        Ok(mut current_name) => {
            let mut shrink_size = 15;

            // override cluster id
            current_name = match env::var_os("custom_cluster_id") {
                None => current_name,
                Some(x) => x.into_string().unwrap(),
            };

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
            // note ensure you use only lowercase  (uppercase are not allowed in lot of AWS resources)
            format!("{}-{}", final_name.to_lowercase(), region.to_lowercase())
        }
        _ => generate_id(),
    }
}
