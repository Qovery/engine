extern crate base64;
extern crate bstr;
extern crate passwords;
extern crate scaleway_api_rs;

use bstr::ByteSlice;
use chrono::Utc;
use curl::easy::Easy;
use dirs::home_dir;
use gethostname;
use std::collections::BTreeMap;
use std::io::{Error, ErrorKind, Write};
use std::path::Path;
use std::str::FromStr;

use passwords::PasswordGenerator;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use retry::delay::Fibonacci;
use retry::OperationResult;
use std::env;
use std::fs;
use tracing::{error, info, span, warn, Level};
use tracing_subscriber;

use crate::scaleway::{
    delete_environment as scw_delete, deploy_environment as scw_deploy, SCW_KUBE_TEST_CLUSTER_ID,
    SCW_MANAGED_DATABASE_DISK_TYPE, SCW_MANAGED_DATABASE_INSTANCE_TYPE, SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
    SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE, SCW_TEST_ZONE,
};
use hashicorp_vault;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::scaleway::application::Zone;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::cmd;
use qovery_engine::constants::{
    AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, DIGITAL_OCEAN_SPACES_ACCESS_ID, DIGITAL_OCEAN_SPACES_SECRET_ID,
    DIGITAL_OCEAN_TOKEN, SCALEWAY_ACCESS_KEY, SCALEWAY_DEFAULT_PROJECT_ID, SCALEWAY_SECRET_KEY,
};
use qovery_engine::error::{SimpleError, SimpleErrorKind};
use qovery_engine::models::{
    Action, Clone2, Context, Database, DatabaseKind, DatabaseMode, Environment, EnvironmentAction, Features, Metadata,
};
use serde::{Deserialize, Serialize};
extern crate time;
use crate::aws::{delete_environment as aws_delete, deploy_environment as aws_deploy, AWS_KUBE_TEST_CLUSTER_ID};
use crate::digitalocean::{
    delete_environment as do_delete, deploy_environment as do_deploy, DO_KUBE_TEST_CLUSTER_ID,
    DO_MANAGED_DATABASE_DISK_TYPE, DO_MANAGED_DATABASE_INSTANCE_TYPE, DO_SELF_HOSTED_DATABASE_DISK_TYPE,
    DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE, DO_TEST_REGION,
};
use qovery_engine::cloud_provider::digitalocean::application::Region;
use qovery_engine::cmd::kubectl::{kubectl_get_pvc, kubectl_get_svc};
use qovery_engine::cmd::structs::{KubernetesList, KubernetesPod};
use qovery_engine::cmd::structs::{KubernetesList, KubernetesPod, SVCItem, PVC, SVC};
use qovery_engine::cmd::structs::{KubernetesList, KubernetesPod, PVC, SVC};
use qovery_engine::models::DatabaseMode::MANAGED;
use qovery_engine::object_storage::spaces::Spaces;
use qovery_engine::object_storage::ObjectStorage;
use qovery_engine::runtime::block_on;
use qovery_engine::transaction::TransactionResult;
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
    pub DIGITAL_OCEAN_DEFAULT_REGION: Option<String>,
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
    pub QOVERY_GRPC_URL: Option<String>,
    pub QOVERY_CLUSTER_SECRET_TOKEN: Option<String>,
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
            DIGITAL_OCEAN_DEFAULT_REGION: None,
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
            QOVERY_GRPC_URL: None,
            QOVERY_CLUSTER_SECRET_TOKEN: None,
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
            DIGITAL_OCEAN_DEFAULT_REGION: Self::select_secret(
                "DIGITAL_OCEAN_DEFAULT_REGION",
                secrets.DIGITAL_OCEAN_DEFAULT_REGION,
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
            QOVERY_GRPC_URL: Self::select_secret("QOVERY_GRPC_URL", secrets.QOVERY_GRPC_URL),
            QOVERY_CLUSTER_SECRET_TOKEN: Self::select_secret(
                "QOVERY_CLUSTER_SECRET_TOKEN",
                secrets.QOVERY_CLUSTER_SECRET_TOKEN,
            ),
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

pub fn generate_password(allow_using_symbols: bool) -> String {
    // core special chars set: !#$%&*+-=?_
    // we will keep only those and exclude others
    let forbidden_chars = vec![
        '"', '\'', '(', ')', ',', '.', '/', ':', ';', '<', '>', '@', '[', '\\', ']', '^', '`', '{', '|', '}', '~',
    ];
    let pg = PasswordGenerator::new()
        .length(32)
        .numbers(true)
        .lowercase_letters(true)
        .uppercase_letters(true)
        .symbols(allow_using_symbols)
        .spaces(false)
        .exclude_similar_characters(true)
        .strict(true);

    let mut password = pg
        .generate_one()
        .expect("error while trying to generate a password")
        .to_string();

    if allow_using_symbols {
        for forbidden_char in forbidden_chars {
            password = password.replace(forbidden_char, "%");
        }
    }

    password
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
    workspace_directory: &str,
    kubernetes_cluster_id: &str,
    secrets: FuncTestsSecrets,
) -> Result<String, SimpleError> {
    let kubernetes_config_bucket_name = format!("qovery-kubeconfigs-{}", kubernetes_cluster_id);
    let kubernetes_config_object_key = format!("{}.yaml", kubernetes_cluster_id);
    let kubernetes_config_file_path = format!("{}/kubernetes_config_{}", workspace_directory, kubernetes_cluster_id);

    let _ = get_kubernetes_config_file(
        provider_kind,
        kubernetes_config_bucket_name,
        kubernetes_config_object_key,
        kubernetes_config_file_path.clone(),
        secrets.clone(),
    )?;

    Ok(kubernetes_config_file_path)
}

fn get_kubernetes_config_file<P>(
    provider_kind: Kind,
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
            Kind::Do => {
                let region_raw = secrets
                    .DIGITAL_OCEAN_DEFAULT_REGION
                    .as_ref()
                    .expect(&"DIGITAL_OCEAN_DEFAULT_REGION should be set".to_string())
                    .to_string();

                match Region::from_str(region_raw.as_str()) {
                    Ok(region) => {
                        let spaces = Spaces::new(
                            context(),
                            "fake".to_string(),
                            "fake".to_string(),
                            secrets
                                .DIGITAL_OCEAN_SPACES_ACCESS_ID
                                .as_ref()
                                .expect(&"DIGITAL_OCEAN_SPACES_ACCESS_ID should be set".to_string())
                                .to_string(),
                            secrets
                                .DIGITAL_OCEAN_SPACES_SECRET_ID
                                .as_ref()
                                .expect(&"DIGITAL_OCEAN_SPACES_SECRET_ID should be set".to_string())
                                .to_string(),
                            region,
                        );

                        match spaces.get(
                            kubernetes_config_bucket_name.as_str(),
                            kubernetes_config_object_key.as_str(),
                            false,
                        ) {
                            Ok((_, mut file)) => {
                                let mut content = String::new();
                                match file.read_to_string(&mut content) {
                                    Ok(_) => Ok(content),
                                    Err(e) => {
                                        let message = format!("error while trying to read file, error: {}", e);
                                        error!("{}", message);

                                        Err(SimpleError::new(SimpleErrorKind::Other, Some(message)))
                                    }
                                }
                            }
                            Err(e) => {
                                let message = format!(
                                    "error while trying to get kubeconfig from spaces, error: {:?}",
                                    e.message,
                                );
                                error!("{}", message);

                                Err(SimpleError::new(SimpleErrorKind::Other, e.message))
                            }
                        }
                    }
                    Err(_) => {
                        let message = format!("`{}` is not a valid region", region_raw);
                        error!("{}", message);

                        Err(SimpleError::new(SimpleErrorKind::Other, Some(message)))
                    }
                }
            }
            Kind::Scw => {
                // TODO(benjaminch): refactor all of this properly
                let zone = Zone::from_str(secrets.clone().SCALEWAY_DEFAULT_REGION.unwrap().as_str()).unwrap();
                let project_id = secrets.clone().SCALEWAY_DEFAULT_PROJECT_ID.unwrap();
                let secret_access_key = secrets.clone().SCALEWAY_SECRET_KEY.unwrap();

                let configuration = scaleway_api_rs::apis::configuration::Configuration {
                    api_key: Some(scaleway_api_rs::apis::configuration::ApiKey {
                        key: secret_access_key.to_string(),
                        prefix: None,
                    }),
                    ..scaleway_api_rs::apis::configuration::Configuration::default()
                };

                let clusters_res = block_on(scaleway_api_rs::apis::clusters_api::list_clusters(
                    &configuration,
                    zone.region().to_string().as_str(),
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
                let expected_test_server_tag = format!("ClusterId={}", SCW_KUBE_TEST_CLUSTER_ID);

                for cluster in clusters.iter() {
                    if cluster.tags.is_some() {
                        for tag in cluster.tags.as_ref().unwrap().iter() {
                            if tag.as_str() == expected_test_server_tag.as_str() {
                                match block_on(scaleway_api_rs::apis::clusters_api::get_cluster_kube_config(
                                    &configuration,
                                    zone.region().as_str(),
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

fn get_cloud_provider_credentials(provider_kind: Kind, secrets: &FuncTestsSecrets) -> KubernetesCredentials {
    match provider_kind {
        Kind::Aws => vec![
            (AWS_ACCESS_KEY_ID, secrets.AWS_ACCESS_KEY_ID.as_ref().unwrap().as_str()),
            (
                AWS_SECRET_ACCESS_KEY,
                secrets.AWS_SECRET_ACCESS_KEY.as_ref().unwrap().as_str(),
            ),
        ],
        Kind::Do => vec![
            (
                DIGITAL_OCEAN_TOKEN,
                secrets
                    .DIGITAL_OCEAN_TOKEN
                    .as_ref()
                    .expect("DIGITAL_OCEAN_TOKEN is not set"),
            ),
            (
                DIGITAL_OCEAN_SPACES_ACCESS_ID,
                secrets
                    .DIGITAL_OCEAN_SPACES_ACCESS_ID
                    .as_ref()
                    .expect("DIGITAL_OCEAN_SPACES_ACCESS_ID is not set"),
            ),
            (
                DIGITAL_OCEAN_SPACES_SECRET_ID,
                secrets
                    .DIGITAL_OCEAN_SPACES_SECRET_ID
                    .as_ref()
                    .expect("DIGITAL_OCEAN_SPACES_SECRET_ID is not set"),
            ),
        ],
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

    let kubernetes_config = kubernetes_config_path(provider_kind.clone(), "/tmp", kube_cluster_id, secrets.clone());

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

    let kubernetes_config = kubernetes_config_path(provider_kind.clone(), "/tmp", kube_cluster_id, secrets.clone());

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

pub fn get_pvc(
    provider_kind: Kind,
    kube_cluster_id: &str,
    environment_check: Environment,
    secrets: FuncTestsSecrets,
) -> Result<PVC, SimpleError> {
    let namespace_name = format!(
        "{}-{}",
        &environment_check.project_id.clone(),
        &environment_check.id.clone(),
    );

    let kubernetes_config = kubernetes_config_path(provider_kind.clone(), "/tmp", kube_cluster_id, secrets.clone());

    match kubernetes_config {
        Ok(path) => {
            match kubectl_get_pvc(
                path.as_str(),
                namespace_name.clone().as_str(),
                get_cloud_provider_credentials(provider_kind.clone(), &secrets.clone()),
            ) {
                Ok(pvc) => Ok(pvc),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

pub fn get_svc(
    provider_kind: Kind,
    kube_cluster_id: &str,
    environment_check: Environment,
    secrets: FuncTestsSecrets,
) -> Result<SVC, SimpleError> {
    let namespace_name = format!(
        "{}-{}",
        &environment_check.project_id.clone(),
        &environment_check.id.clone(),
    );

    let kubernetes_config = kubernetes_config_path(provider_kind.clone(), "/tmp", kube_cluster_id, secrets.clone());

    match kubernetes_config {
        Ok(path) => {
            match kubectl_get_svc(
                path.as_str(),
                namespace_name.clone().as_str(),
                get_cloud_provider_credentials(provider_kind.clone(), &secrets.clone()),
            ) {
                Ok(pvc) => Ok(pvc),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

pub fn db_fqnd(db: Database) -> String {
    match db.publicly_accessible {
        true => db.fqdn,
        false => match db.mode == MANAGED {
            true => format!("{}-dns", db.id),
            false => match db.kind {
                DatabaseKind::Postgresql => "postgresqlpostgres",
                DatabaseKind::Mysql => "mysqlmysqldatabase",
                DatabaseKind::Mongodb => "mongodbmymongodb",
                DatabaseKind::Redis => "redismyredis-master",
            }
            .to_string(),
        },
    }
}

struct DBInfos {
    db_port: u16,
    db_name: String,
    app_commit: String,
    app_env_vars: BTreeMap<String, String>,
}

fn db_infos(
    db_kind: DatabaseKind,
    database_mode: DatabaseMode,
    database_username: String,
    database_password: String,
    db_fqdn: String,
) -> DBInfos {
    match db_kind {
        DatabaseKind::Mongodb => {
            let database_port = 27017;
            let database_db_name = "my-mongodb".to_string();
            let database_uri = format!(
                "mongodb://{}:{}@{}:{}/{}",
                database_username,
                database_password,
                db_fqdn.clone(),
                database_port,
                database_db_name.clone()
            );
            DBInfos {
                db_port: database_port.clone(),
                db_name: database_db_name.to_string(),
                app_commit: "3fdc7e784c1d98b80446be7ff25e35370306d9a8".to_string(),
                app_env_vars: btreemap! {
                    "IS_DOCUMENTDB".to_string() => base64::encode((database_mode == MANAGED).to_string()),
                    "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string() => base64::encode(db_fqdn.clone()),
                    "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string() => base64::encode(database_uri.clone()),
                    "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string() => base64::encode(database_port.to_string()),
                    "MONGODB_DBNAME".to_string() => base64::encode(database_db_name.clone()),
                    "QOVERY_DATABASE_TESTING_DATABASE_USERNAME".to_string() => base64::encode(database_username.clone()),
                    "QOVERY_DATABASE_TESTING_DATABASE_PASSWORD".to_string() => base64::encode(database_password.clone()),
                },
            }
        }
        DatabaseKind::Mysql => {
            let database_port = 3306;
            let database_db_name = "mysqldatabase".to_string();
            DBInfos {
                db_port: database_port.clone(),
                db_name: database_db_name.to_string(),
                app_commit: "fc8a87b39cdee84bb789893fb823e3e62a1999c0".to_string(),
                app_env_vars: btreemap! {
                    "MYSQL_HOST".to_string() => base64::encode(db_fqdn.clone()),
                    "MYSQL_PORT".to_string() => base64::encode(database_port.to_string()),
                    "MYSQL_DBNAME".to_string()   => base64::encode(database_db_name.clone()),
                    "MYSQL_USERNAME".to_string() => base64::encode(database_username.clone()),
                    "MYSQL_PASSWORD".to_string() => base64::encode(database_password.clone()),
                },
            }
        }
        DatabaseKind::Postgresql => {
            let database_port = 5432;
            let database_db_name = "postgres".to_string();
            DBInfos {
                db_port: database_port.clone(),
                db_name: database_db_name.to_string(),
                app_commit: "c3eda167df49fa9757f281d6f3655ba46287c61d".to_string(),
                app_env_vars: btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_db_name.clone()),
                     "PG_HOST".to_string() => base64::encode(db_fqdn.clone()),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username.clone()),
                     "PG_PASSWORD".to_string() => base64::encode(database_password.clone()),
                },
            }
        }
        DatabaseKind::Redis => {
            let database_port = 6379;
            let database_db_name = "my-redis".to_string();
            DBInfos {
                db_port: database_port.clone(),
                db_name: database_db_name.to_string(),
                app_commit: "80ad41fbe9549f8de8dbe2ca4dd5d23e8ffc92de".to_string(),
                app_env_vars: btreemap! {
                "IS_ELASTICCACHE".to_string() => base64::encode((database_mode == MANAGED).to_string()),
                "REDIS_HOST".to_string()      => base64::encode(db_fqdn.clone()),
                "REDIS_PORT".to_string()      => base64::encode(database_port.to_string()),
                "REDIS_USERNAME".to_string()  => base64::encode(database_username.clone()),
                "REDIS_PASSWORD".to_string()  => base64::encode(database_password.clone()),
                },
            }
        }
    }
}

fn db_disk_type(provider_kind: Kind, database_mode: DatabaseMode) -> String {
    match provider_kind {
        Kind::Aws => "gp2",
        Kind::Do => match database_mode {
            DatabaseMode::MANAGED => DO_MANAGED_DATABASE_DISK_TYPE,
            DatabaseMode::CONTAINER => DO_SELF_HOSTED_DATABASE_DISK_TYPE,
        },
        Kind::Scw => match database_mode {
            DatabaseMode::MANAGED => SCW_MANAGED_DATABASE_DISK_TYPE,
            DatabaseMode::CONTAINER => SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
        },
    }
    .to_string()
}

fn db_instance_type(provider_kind: Kind, db_kind: DatabaseKind, database_mode: DatabaseMode) -> String {
    match provider_kind {
        Kind::Aws => match db_kind {
            DatabaseKind::Mongodb => "db.t3.medium",
            DatabaseKind::Mysql => "db.t2.micro",
            DatabaseKind::Postgresql => "db.t2.micro",
            DatabaseKind::Redis => "cache.t3.micro",
        },
        Kind::Do => match database_mode {
            DatabaseMode::MANAGED => DO_MANAGED_DATABASE_INSTANCE_TYPE,
            DatabaseMode::CONTAINER => DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
        },
        Kind::Scw => match database_mode {
            DatabaseMode::MANAGED => SCW_MANAGED_DATABASE_INSTANCE_TYPE,
            DatabaseMode::CONTAINER => SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
        },
    }
    .to_string()
}

pub fn get_svc_name(db_kind: DatabaseKind, provider_kind: Kind) -> &'static str {
    match db_kind {
        DatabaseKind::Postgresql => match provider_kind {
            Kind::Aws => "postgresqlpostgres",
            _ => "postgresql-postgres",
        },
        DatabaseKind::Mysql => match provider_kind {
            Kind::Aws => "mysqlmysqldatabase",
            _ => "mysql-mysqldatabase",
        },
        DatabaseKind::Mongodb => match provider_kind {
            Kind::Aws => "mongodbmymongodb",
            _ => "mongodb-my-mongodb",
        },
        DatabaseKind::Redis => match provider_kind {
            Kind::Aws => "redismyredis-master",
            _ => "redis-my-redis-master",
        },
    }
}

pub fn test_db(
    context: Context,
    mut environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    db_kind: DatabaseKind,
    provider_kind: Kind,
    database_mode: DatabaseMode,
    is_public: bool,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();
    let context_for_delete = context.clone_not_same_execution_id();

    let app_id = generate_id();
    let database_username = "superuser".to_string();
    let database_password = generate_id();
    let db_kind_str = db_kind.name().to_string();
    let database_host = format!(
        "{}-{}.{}",
        db_kind_str.clone(),
        generate_id(),
        secrets.clone().DEFAULT_TEST_DOMAIN.unwrap()
    );
    let dyn_db_fqdn = match is_public.clone() {
        true => database_host.clone(),
        false => match database_mode.clone() {
            DatabaseMode::MANAGED => format!("{}-dns", app_id.clone()),
            DatabaseMode::CONTAINER => get_svc_name(db_kind.clone(), provider_kind.clone()).to_string(),
        },
    };

    let db_infos = db_infos(
        db_kind.clone(),
        database_mode.clone(),
        database_username.clone(),
        database_password.clone(),
        dyn_db_fqdn.clone(),
    );
    let database_port = db_infos.db_port.clone();
    let database_db_name = db_infos.db_name.clone();
    let storage_size = 10;
    let db_disk_type = db_disk_type(provider_kind.clone(), database_mode.clone());
    let db_instance_type = db_instance_type(provider_kind.clone(), db_kind.clone(), database_mode.clone());
    let db = Database {
        kind: db_kind.clone(),
        action: Action::Create,
        id: app_id.clone(),
        name: database_db_name.clone(),
        version: version.to_string(),
        fqdn_id: format!("{}-{}", db_kind_str.clone(), generate_id()),
        fqdn: database_host.clone(),
        port: database_port.clone(),
        username: database_username.clone(),
        password: database_password.clone(),
        total_cpus: "100m".to_string(),
        total_ram_in_mib: 512,
        disk_size_in_gib: storage_size.clone(),
        database_instance_type: "db.t2.micro".to_string(),
        database_disk_type: db_disk_type,
        activate_high_availability: false,
        activate_backups: false,
        publicly_accessible: is_public.clone(),
        mode: database_mode.clone(),
    };

    environment.databases = vec![db.clone()];

    let app_name = format!("{}-app-{}", db_kind_str.clone(), generate_id());
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.branch = app_name.clone();
            app.commit_id = db_infos.app_commit.clone();
            app.private_port = Some(1234);
            app.dockerfile_path = Some(format!("Dockerfile-{}", version));
            app.environment_vars = db_infos.app_env_vars.clone();
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();
    environment.routers[0].routes[0].application_name = app_name.clone();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment.clone());
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match provider_kind {
        Kind::Aws => match aws_deploy(&context, ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        },
        Kind::Do => match do_deploy(&context, ea, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        },
        Kind::Scw => match scw_deploy(&context, ea, SCW_TEST_ZONE) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        },
    }

    let kube_cluster_id = match provider_kind {
        Kind::Aws => AWS_KUBE_TEST_CLUSTER_ID,
        Kind::Do => DO_KUBE_TEST_CLUSTER_ID,
        Kind::Scw => SCW_KUBE_TEST_CLUSTER_ID,
    };

    match database_mode.clone() {
        DatabaseMode::CONTAINER => {
            match get_pvc(
                provider_kind.clone(),
                kube_cluster_id.clone(),
                environment.clone(),
                secrets.clone(),
            ) {
                Ok(pvc) => assert_eq!(
                    pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                    format!("{}Gi", storage_size)
                ),
                Err(_) => assert!(false),
            };

            match get_svc(
                provider_kind.clone(),
                kube_cluster_id.clone(),
                environment.clone(),
                secrets.clone(),
            ) {
                Ok(svc) => assert_eq!(
                    svc.items
                        .expect("No items in svc")
                        .into_iter()
                        .filter(|svc| svc
                            .metadata
                            .name
                            .contains(get_svc_name(db_kind.clone(), provider_kind.clone()))
                            && &svc.spec.svc_type == "LoadBalancer")
                        .collect::<Vec<SVCItem>>()
                        .len(),
                    match is_public {
                        true => 1,
                        false => 0,
                    }
                ),
                Err(_) => assert!(false),
            };
        }
        DatabaseMode::MANAGED => {
            match get_svc(
                provider_kind.clone(),
                kube_cluster_id.clone(),
                environment.clone(),
                secrets.clone(),
            ) {
                Ok(svc) => {
                    let service = svc
                        .items
                        .expect("No items in svc")
                        .into_iter()
                        .filter(|svc| {
                            svc.metadata.name.contains(format!("{}-dns", app_id.clone()).as_str())
                                && svc.spec.svc_type == "ExternalName"
                        })
                        .collect::<Vec<SVCItem>>();
                    let annotations = &service[0].metadata.annotations;
                    assert_eq!(service.len(), 1);
                    match is_public {
                        true => {
                            assert!(annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                            assert_eq!(annotations["external-dns.alpha.kubernetes.io/hostname"], database_host);
                        }
                        false => assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname")),
                    }
                }
                Err(_) => assert!(false),
            };
        }
    }

    match provider_kind.clone() {
        Kind::Aws => match aws_delete(&context_for_delete, ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        },
        Kind::Do => match do_delete(&context_for_delete, ea_delete, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        },
        Kind::Scw => match scw_delete(&context_for_delete, ea_delete, SCW_TEST_ZONE) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        },
    }

    return test_name.to_string();
}
