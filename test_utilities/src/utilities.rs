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

use passwords::PasswordGenerator;
use qovery_engine::cloud_provider::digitalocean::kubernetes::doks_api::get_do_kubeconfig_by_cluster_name;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use retry::delay::Fibonacci;
use retry::OperationResult;
use std::env;
use std::fs;
use std::str::FromStr;
use tracing::{info, warn};

use crate::scaleway::{
    SCW_MANAGED_DATABASE_DISK_TYPE, SCW_MANAGED_DATABASE_INSTANCE_TYPE, SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
    SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
};
use hashicorp_vault;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::cmd;
use qovery_engine::constants::{
    AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, DIGITAL_OCEAN_SPACES_ACCESS_ID, DIGITAL_OCEAN_SPACES_SECRET_ID,
    DIGITAL_OCEAN_TOKEN, SCALEWAY_ACCESS_KEY, SCALEWAY_DEFAULT_PROJECT_ID, SCALEWAY_SECRET_KEY,
};
use qovery_engine::io_models::{Context, Database, DatabaseKind, DatabaseMode, EnvironmentRequest, Features, Metadata};
use retry::Error::Operation;
use serde::{Deserialize, Serialize};

extern crate time;
use crate::digitalocean::{
    DO_MANAGED_DATABASE_DISK_TYPE, DO_MANAGED_DATABASE_INSTANCE_TYPE, DO_SELF_HOSTED_DATABASE_DISK_TYPE,
    DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
};
use qovery_engine::cmd::command::QoveryCommand;
use qovery_engine::cmd::docker::Docker;
use qovery_engine::cmd::kubectl::{kubectl_get_pvc, kubectl_get_svc};
use qovery_engine::cmd::structs::{KubernetesList, KubernetesPod, PVC, SVC};
use qovery_engine::errors::CommandError;
use qovery_engine::io_models::DatabaseMode::MANAGED;
use qovery_engine::logger::{Logger, StdIoLogger};
use qovery_engine::models::scaleway::ScwZone;
use qovery_engine::runtime::block_on;
use qovery_engine::utilities::to_short_id;
use time::Instant;
use url::Url;

pub fn context(organization_id: &str, cluster_id: &str) -> Context {
    let organization_id = organization_id.to_string();
    let cluster_id = cluster_id.to_string();
    let execution_id = execution_id();
    let home_dir = env::var("WORKSPACE_ROOT_DIR").unwrap_or_else(|_| home_dir().unwrap().to_str().unwrap().to_string());
    let lib_root_dir = env::var("LIB_ROOT_DIR").expect("LIB_ROOT_DIR is mandatory");
    let docker_host = env::var("DOCKER_HOST").map(|x| Url::parse(&x).unwrap()).ok();
    let docker = Docker::new(docker_host.clone()).expect("Can't init docker");

    let metadata = Metadata {
        dry_run_deploy: Option::from(env::var_os("dry_run_deploy").is_some()),
        resource_expiration_in_seconds: {
            // set a custom ttl as environment variable for manual tests
            match env::var_os("ttl") {
                Some(ttl) => {
                    let ttl_converted: u32 = ttl.into_string().unwrap().parse().unwrap();
                    Some(ttl_converted)
                }
                None => Some(10800),
            }
        },
        forced_upgrade: Option::from(env::var_os("forced_upgrade").is_some()),
        disable_pleco: Some(true),
    };

    let enabled_features = vec![Features::LogsHistory, Features::MetricsHistory];

    Context::new(
        organization_id,
        cluster_id,
        execution_id,
        home_dir,
        lib_root_dir,
        true,
        docker_host,
        enabled_features,
        Option::from(metadata),
        docker,
    )
}

pub fn logger() -> Box<dyn Logger> {
    Box::new(StdIoLogger::new())
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(non_snake_case)]
pub struct FuncTestsSecrets {
    pub AWS_ACCESS_KEY_ID: Option<String>,
    pub AWS_DEFAULT_REGION: Option<String>,
    pub AWS_SECRET_ACCESS_KEY: Option<String>,
    pub AWS_TEST_CLUSTER_ID: Option<String>,
    pub AWS_EC2_TEST_CLUSTER_ID: Option<String>,
    pub AWS_EC2_TEST_CLUSTER_DOMAIN: Option<String>,
    pub AWS_TEST_ORGANIZATION_ID: Option<String>,
    pub AWS_EC2_DEFAULT_CLUSTER_ID: Option<String>,
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
    pub DIGITAL_OCEAN_TEST_CLUSTER_ID: Option<String>,
    pub DIGITAL_OCEAN_TEST_ORGANIZATION_ID: Option<String>,
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
    pub SCALEWAY_TEST_CLUSTER_ID: Option<String>,
    pub SCALEWAY_TEST_ORGANIZATION_ID: Option<String>,
    pub TERRAFORM_AWS_ACCESS_KEY_ID: Option<String>,
    pub TERRAFORM_AWS_SECRET_ACCESS_KEY: Option<String>,
    pub TERRAFORM_AWS_REGION: Option<String>,
    pub QOVERY_GRPC_URL: Option<String>,
    pub QOVERY_CLUSTER_SECRET_TOKEN: Option<String>,
    pub QOVERY_CLUSTER_JWT_TOKEN: Option<String>,
    pub QOVERY_DNS_API_URL: Option<String>,
    pub QOVERY_DNS_API_KEY: Option<String>,
    pub QOVERY_DNS_DOMAIN: Option<String>,
}

struct VaultConfig {
    address: String,
    token: String,
}

impl Default for FuncTestsSecrets {
    fn default() -> Self {
        Self::new()
    }
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
                    "VAULT_ADDR environment variable is missing".to_string(),
                ))
            }
        };

        let vault_token = match env::var_os("VAULT_TOKEN") {
            Some(x) => x.into_string().unwrap(),
            None => {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    "VAULT_TOKEN environment variable is missing".to_string(),
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
            AWS_TEST_CLUSTER_ID: None,
            AWS_EC2_TEST_CLUSTER_ID: None,
            AWS_EC2_TEST_CLUSTER_DOMAIN: None,
            AWS_TEST_ORGANIZATION_ID: None,
            AWS_EC2_DEFAULT_CLUSTER_ID: None,
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
            DIGITAL_OCEAN_TEST_CLUSTER_ID: None,
            DIGITAL_OCEAN_TEST_ORGANIZATION_ID: None,
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
            SCALEWAY_TEST_CLUSTER_ID: None,
            SCALEWAY_TEST_ORGANIZATION_ID: None,
            TERRAFORM_AWS_ACCESS_KEY_ID: None,
            TERRAFORM_AWS_SECRET_ACCESS_KEY: None,
            TERRAFORM_AWS_REGION: None,
            QOVERY_GRPC_URL: None,
            QOVERY_CLUSTER_SECRET_TOKEN: None,
            QOVERY_CLUSTER_JWT_TOKEN: None,
            QOVERY_DNS_API_URL: None,
            QOVERY_DNS_API_KEY: None,
            QOVERY_DNS_DOMAIN: None,
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
            AWS_TEST_ORGANIZATION_ID: Self::select_secret("AWS_TEST_ORGANIZATION_ID", secrets.AWS_TEST_ORGANIZATION_ID),
            AWS_TEST_CLUSTER_ID: Self::select_secret("AWS_TEST_CLUSTER_ID", secrets.AWS_TEST_CLUSTER_ID),
            AWS_EC2_TEST_CLUSTER_ID: Self::select_secret("AWS_EC2_TEST_CLUSTER_ID", secrets.AWS_EC2_TEST_CLUSTER_ID),
            AWS_EC2_TEST_CLUSTER_DOMAIN: Self::select_secret(
                "AWS_EC2_TEST_CLUSTER_DOMAIN",
                secrets.AWS_EC2_TEST_CLUSTER_DOMAIN,
            ),
            AWS_EC2_DEFAULT_CLUSTER_ID: Self::select_secret(
                "AWS_EC2_DEFAULT_CLUSTER_ID",
                secrets.AWS_EC2_DEFAULT_CLUSTER_ID,
            ),
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
            DIGITAL_OCEAN_TEST_ORGANIZATION_ID: Self::select_secret(
                "DIGITAL_OCEAN_TEST_ORGANIZATION_ID",
                secrets.DIGITAL_OCEAN_TEST_ORGANIZATION_ID,
            ),
            DIGITAL_OCEAN_TEST_CLUSTER_ID: Self::select_secret(
                "DIGITAL_OCEAN_TEST_CLUSTER_ID",
                secrets.DIGITAL_OCEAN_TEST_CLUSTER_ID,
            ),
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
            SCALEWAY_TEST_ORGANIZATION_ID: Self::select_secret(
                "SCALEWAY_TEST_ORGANIZATION_ID",
                secrets.SCALEWAY_TEST_ORGANIZATION_ID,
            ),
            SCALEWAY_TEST_CLUSTER_ID: Self::select_secret("SCALEWAY_TEST_CLUSTER_ID", secrets.SCALEWAY_TEST_CLUSTER_ID),
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
            QOVERY_CLUSTER_JWT_TOKEN: Self::select_secret("QOVERY_CLUSTER_JWT_TOKEN", secrets.QOVERY_CLUSTER_JWT_TOKEN),
            QOVERY_DNS_API_URL: Self::select_secret("QOVERY_DNS_API_URL", secrets.QOVERY_DNS_API_URL),
            QOVERY_DNS_API_KEY: Self::select_secret("QOVERY_DNS_API_KEY", secrets.QOVERY_DNS_API_KEY),
            QOVERY_DNS_DOMAIN: Self::select_secret("QOVERYDNS_DOMAIN", secrets.QOVERY_DNS_DOMAIN),
        }
    }
}

pub fn build_platform_local_docker(context: &Context, logger: Box<dyn Logger>) -> LocalDocker {
    LocalDocker::new(context.clone(), "oxqlm3r99vwcmvuj", "qovery-local-docker", logger).unwrap()
}

pub fn init() -> Instant {
    let ci_var = "CI";

    let _ = match env::var_os(ci_var) {
        Some(_) => tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_current_span(true)
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

pub fn engine_run_test<T>(test: T)
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
        let rand_string: Vec<u8> = thread_rng().sample_iter(Alphanumeric).take(15).collect();
        let rand_string = String::from_utf8(rand_string).unwrap();
        if rand_string.chars().next().unwrap().is_alphabetic() {
            uuid = rand_string.to_lowercase();
            break;
        }
    }
    uuid
}

pub fn generate_password(provider_kind: Kind, db_mode: DatabaseMode) -> String {
    // core special chars set: !#$%&*+-=?_
    // we will keep only those and exclude others
    let forbidden_chars = vec![
        '"', '\'', '(', ')', ',', '.', '/', ':', ';', '<', '>', '@', '[', '\\', ']', '^', '`', '{', '|', '}', '~',
    ];

    let allow_using_symbols = provider_kind == Kind::Scw && db_mode == MANAGED;
    if !allow_using_symbols {
        return generate_id();
    };

    let pg = PasswordGenerator::new()
        .length(32)
        .numbers(true)
        .lowercase_letters(true)
        .uppercase_letters(true)
        .symbols(allow_using_symbols)
        .spaces(false)
        .exclude_similar_characters(true)
        .strict(true);

    let mut password = pg.generate_one().expect("error while trying to generate a password");

    if allow_using_symbols {
        for forbidden_char in forbidden_chars {
            password = password.replace(forbidden_char, "%");
        }
    }

    password
}

pub fn check_all_connections(env: &EnvironmentRequest) -> Vec<bool> {
    let mut checking: Vec<bool> = Vec::with_capacity(env.routers.len());

    for router_to_test in &env.routers {
        let path_to_test = format!("https://{}{}", &router_to_test.default_domain, &router_to_test.routes[0].path);

        checking.push(curl_path(path_to_test.as_str()));
    }
    checking
}

fn curl_path(path: &str) -> bool {
    let mut easy = Easy::new();
    easy.url(path).unwrap();
    let res = easy.perform();
    match res {
        Ok(_) => true,

        Err(e) => {
            println!("TEST Error : while trying to call {}", e);
            false
        }
    }
}

pub fn kubernetes_config_path(
    context: Context,
    provider_kind: Kind,
    workspace_directory: &str,
    secrets: FuncTestsSecrets,
) -> Result<String, CommandError> {
    let kubernetes_config_bucket_name = format!("qovery-kubeconfigs-{}", context.cluster_id());
    let kubernetes_config_object_key = format!("{}.yaml", context.cluster_id());
    let kubernetes_config_file_path = format!("{}/kubernetes_config_{}", workspace_directory, context.cluster_id());

    let _ = get_kubernetes_config_file(
        context,
        provider_kind,
        kubernetes_config_bucket_name,
        kubernetes_config_object_key,
        kubernetes_config_file_path.clone(),
        secrets,
    )?;

    Ok(kubernetes_config_file_path)
}

fn get_kubernetes_config_file<P>(
    context: Context,
    provider_kind: Kind,
    kubernetes_config_bucket_name: String,
    kubernetes_config_object_key: String,
    file_path: P,
    secrets: FuncTestsSecrets,
) -> Result<fs::File, CommandError>
where
    P: AsRef<Path>,
{
    // return the file if it already exists and should use cache
    if let Ok(f) = fs::File::open(file_path.as_ref()) {
        return Ok(f);
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
                let cluster_name = format!("qovery-{}", context.cluster_id());
                let kubeconfig = match get_do_kubeconfig_by_cluster_name(
                    secrets.clone().DIGITAL_OCEAN_TOKEN.unwrap().as_str(),
                    cluster_name.as_str(),
                ) {
                    Ok(kubeconfig) => kubeconfig,
                    Err(e) => return OperationResult::Retry(e),
                };

                match kubeconfig {
                    None => Err(CommandError::new_from_safe_message("No kubeconfig found".to_string())),
                    Some(file_content) => {
                        let _ = "test";
                        Ok(file_content)
                    }
                }
            }
            Kind::Scw => {
                // TODO(benjaminch): refactor all of this properly
                let zone = ScwZone::from_str(secrets.clone().SCALEWAY_DEFAULT_REGION.unwrap().as_str()).unwrap();
                let project_id = secrets.clone().SCALEWAY_DEFAULT_PROJECT_ID.unwrap();
                let secret_access_key = secrets.clone().SCALEWAY_SECRET_KEY.unwrap();

                let configuration = scaleway_api_rs::apis::configuration::Configuration {
                    api_key: Some(scaleway_api_rs::apis::configuration::ApiKey {
                        key: secret_access_key,
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
                    return OperationResult::Retry(CommandError::new(
                        "Error while trying to get clusters".to_string(),
                        Some(e.to_string()),
                        None,
                    ));
                }

                let clusters = clusters_res.unwrap();

                if clusters.clusters.is_none() {
                    return OperationResult::Retry(CommandError::new_from_safe_message(
                        "Error while trying to get clusters".to_string(),
                    ));
                }

                let clusters = clusters.clusters.unwrap();
                let expected_test_server_tag = format!(
                    "ClusterId={}",
                    secrets
                        .SCALEWAY_TEST_CLUSTER_ID
                        .as_ref()
                        .expect("SCALEWAY_TEST_CLUSTER_ID is not set")
                );

                for cluster in clusters.iter() {
                    if cluster.tags.is_some() {
                        for tag in cluster.tags.as_ref().unwrap().iter() {
                            if tag.as_str() == expected_test_server_tag.as_str() {
                                return match block_on(scaleway_api_rs::apis::clusters_api::get_cluster_kube_config(
                                    &configuration,
                                    zone.region().as_str(),
                                    cluster.id.as_ref().unwrap().as_str(),
                                )) {
                                    Ok(res) => OperationResult::Ok(
                                        base64::decode(res.content.unwrap())
                                            .unwrap()
                                            .to_str()
                                            .unwrap()
                                            .to_string(),
                                    ),
                                    Err(e) => {
                                        let message_safe = "Error while trying to get clusters";
                                        OperationResult::Retry(CommandError::new(
                                            message_safe.to_string(),
                                            Some(e.to_string()),
                                            None,
                                        ))
                                    }
                                };
                            }
                        }
                    }
                }

                Err(CommandError::new_from_safe_message("Test cluster not found".to_string()))
            }
        };

        match file_content {
            Ok(file_content) => OperationResult::Ok(file_content),
            Err(err) => OperationResult::Retry(err),
        }
    });

    let file_content = match file_content_result {
        Ok(file_content) => file_content,
        Err(Operation { error, .. }) => return Err(error),
        Err(retry::Error::Internal(msg)) => {
            return Err(CommandError::new_from_safe_message(msg));
        }
    };

    let mut kubernetes_config_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(file_path.as_ref())
        .map_err(|e| CommandError::new("Error opening kubeconfig file.".to_string(), Some(e.to_string()), None))?;
    kubernetes_config_file
        .write_all(file_content.as_bytes())
        .map_err(|_| CommandError::new_from_safe_message("Error while trying to write into file.".to_string()))?;

    // removes warning kubeconfig is (world/group) readable
    let mut perms = fs::metadata(file_path.as_ref())
        .map_err(|_| CommandError::new_from_safe_message("Error while trying to get file metadata.".to_string()))?
        .permissions();
    perms.set_readonly(false);
    fs::set_permissions(file_path.as_ref(), perms)
        .map_err(|_| CommandError::new_from_safe_message("Error while trying to set file permission.".to_string()))?;
    Ok(kubernetes_config_file)
}

type KubernetesCredentials<'a> = Vec<(&'a str, &'a str)>;

fn get_cloud_provider_credentials(provider_kind: Kind, secrets: &FuncTestsSecrets) -> KubernetesCredentials {
    match provider_kind {
        Kind::Aws => vec![
            (AWS_ACCESS_KEY_ID, secrets.AWS_ACCESS_KEY_ID.as_ref().unwrap().as_str()),
            (AWS_SECRET_ACCESS_KEY, secrets.AWS_SECRET_ACCESS_KEY.as_ref().unwrap().as_str()),
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
            (SCALEWAY_ACCESS_KEY, secrets.SCALEWAY_ACCESS_KEY.as_ref().unwrap().as_str()),
            (SCALEWAY_SECRET_KEY, secrets.SCALEWAY_SECRET_KEY.as_ref().unwrap().as_str()),
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
) -> Result<String, CommandError> {
    let local_path = format!("/tmp/{}", object_key); // FIXME: change hardcoded /tmp/

    // gets an aws s3 object using aws-cli
    // used as a failover when rusoto_s3 acts up
    let s3_url = format!("s3://{}/{}", bucket_name, object_key);

    let mut cmd = QoveryCommand::new(
        "aws",
        &["s3", "cp", &s3_url, &local_path],
        &[
            (AWS_ACCESS_KEY_ID, access_key_id),
            (AWS_SECRET_ACCESS_KEY, secret_access_key),
        ],
    );

    cmd.exec()
        .map_err(|err| CommandError::new_from_safe_message(format!("{:?}", err)))?;
    let s = fs::read_to_string(&local_path)
        .map_err(|_| CommandError::new_from_safe_message("Error while trying to read file to string.".to_string()))?;

    Ok(s)
}

pub fn is_pod_restarted_env(
    context: Context,
    provider_kind: Kind,
    environment_check: EnvironmentRequest,
    pod_to_check: &str,
    secrets: FuncTestsSecrets,
) -> (bool, String) {
    let namespace_name = format!(
        "{}-{}",
        to_short_id(&environment_check.project_long_id),
        to_short_id(&environment_check.long_id),
    );

    let kubernetes_config = kubernetes_config_path(context, provider_kind.clone(), "/tmp", secrets.clone());

    match kubernetes_config {
        Ok(path) => {
            let restarted_database = cmd::kubectl::kubectl_exec_get_number_of_restart(
                path.as_str(),
                namespace_name.as_str(),
                pod_to_check,
                get_cloud_provider_credentials(provider_kind, &secrets),
            );
            match restarted_database {
                Ok(count) => match count.trim().eq("0") {
                    true => (true, "0".to_string()),
                    false => (true, count.to_string()),
                },
                _ => (false, "".to_string()),
            }
        }
        Err(_e) => (false, "".to_string()),
    }
}

pub fn get_pods(
    context: Context,
    provider_kind: Kind,
    environment_check: EnvironmentRequest,
    pod_to_check: &str,
    secrets: FuncTestsSecrets,
) -> Result<KubernetesList<KubernetesPod>, CommandError> {
    let namespace_name = format!(
        "{}-{}",
        to_short_id(&environment_check.project_long_id),
        to_short_id(&environment_check.long_id),
    );

    let kubernetes_config = kubernetes_config_path(context, provider_kind.clone(), "/tmp", secrets.clone());

    cmd::kubectl::kubectl_exec_get_pods(
        kubernetes_config.unwrap().as_str(),
        Some(namespace_name.as_str()),
        Some(pod_to_check),
        get_cloud_provider_credentials(provider_kind, &secrets),
    )
}

pub fn execution_id() -> String {
    Utc::now()
        .to_rfc3339()
        .replace(':', "-")
        .replace('.', "-")
        .replace('+', "-")
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

            // flag such name to ease deletion later on
            current_name = format!("fixed-{}", current_name);

            // avoid out of bounds issue
            if current_name.chars().count() < shrink_size {
                shrink_size = current_name.chars().count()
            }

            let mut final_name = (&current_name[..shrink_size]).to_string();
            // do not end with a non alphanumeric char
            while !final_name.chars().last().unwrap().is_alphanumeric() {
                shrink_size -= 1;
                final_name = (&current_name[..shrink_size]).to_string();
            }
            // note ensure you use only lowercase  (uppercase are not allowed in lot of AWS resources)
            format!("{}-{}", final_name.to_lowercase(), region.to_lowercase())
        }
        _ => generate_id(),
    }
}

pub fn get_pvc(
    context: Context,
    provider_kind: Kind,
    environment_check: EnvironmentRequest,
    secrets: FuncTestsSecrets,
) -> Result<PVC, CommandError> {
    let namespace_name = format!(
        "{}-{}",
        to_short_id(&environment_check.project_long_id),
        to_short_id(&environment_check.long_id),
    );

    let kubernetes_config = kubernetes_config_path(context, provider_kind.clone(), "/tmp", secrets.clone());

    match kubernetes_config {
        Ok(path) => {
            match kubectl_get_pvc(
                path.as_str(),
                namespace_name.as_str(),
                get_cloud_provider_credentials(provider_kind, &secrets),
            ) {
                Ok(pvc) => Ok(pvc),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

pub fn get_svc(
    context: Context,
    provider_kind: Kind,
    environment_check: EnvironmentRequest,
    secrets: FuncTestsSecrets,
) -> Result<SVC, CommandError> {
    let namespace_name = format!(
        "{}-{}",
        to_short_id(&environment_check.project_long_id),
        to_short_id(&environment_check.long_id),
    );

    let kubernetes_config = kubernetes_config_path(context, provider_kind.clone(), "/tmp", secrets.clone());

    match kubernetes_config {
        Ok(path) => {
            match kubectl_get_svc(
                path.as_str(),
                namespace_name.as_str(),
                get_cloud_provider_credentials(provider_kind, &secrets),
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
            true => format!("{}-dns", to_short_id(&db.long_id)),
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

pub struct DBInfos {
    pub db_port: u16,
    pub db_name: String,
    pub app_commit: String,
    pub app_env_vars: BTreeMap<String, String>,
}

pub fn db_infos(
    db_kind: DatabaseKind,
    db_id: String,
    database_mode: DatabaseMode,
    database_username: String,
    database_password: String,
    db_fqdn: String,
) -> DBInfos {
    match db_kind {
        DatabaseKind::Mongodb => {
            let database_port = 27017;
            let database_db_name = db_id;
            let database_uri = format!(
                "mongodb://{}:{}@{}:{}/{}",
                database_username, database_password, db_fqdn, database_port, database_db_name
            );
            DBInfos {
                db_port: database_port,
                db_name: database_db_name.to_string(),
                app_commit: "da5dd2b58b78576921373fcb4d4bddc796a804a8".to_string(),
                app_env_vars: btreemap! {
                    "IS_DOCUMENTDB".to_string() => base64::encode((database_mode == MANAGED).to_string()),
                    "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string() => base64::encode(db_fqdn),
                    "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string() => base64::encode(database_uri),
                    "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string() => base64::encode(database_port.to_string()),
                    "MONGODB_DBNAME".to_string() => base64::encode(database_db_name),
                    "QOVERY_DATABASE_TESTING_DATABASE_USERNAME".to_string() => base64::encode(database_username),
                    "QOVERY_DATABASE_TESTING_DATABASE_PASSWORD".to_string() => base64::encode(database_password),
                },
            }
        }
        DatabaseKind::Mysql => {
            let database_port = 3306;
            let database_db_name = db_id;
            DBInfos {
                db_port: database_port,
                db_name: database_db_name.to_string(),
                app_commit: "42f6553b6be617f954f903e01236e225bbb9f468".to_string(),
                app_env_vars: btreemap! {
                    "MYSQL_HOST".to_string() => base64::encode(db_fqdn),
                    "MYSQL_PORT".to_string() => base64::encode(database_port.to_string()),
                    "MYSQL_DBNAME".to_string()   => base64::encode(database_db_name),
                    "MYSQL_USERNAME".to_string() => base64::encode(database_username),
                    "MYSQL_PASSWORD".to_string() => base64::encode(database_password),
                },
            }
        }
        DatabaseKind::Postgresql => {
            let database_port = 5432;
            let database_db_name = if database_mode == MANAGED {
                "postgres".to_string()
            } else {
                db_id
            };
            DBInfos {
                db_port: database_port,
                db_name: database_db_name.to_string(),
                app_commit: "61c7a9b55a085229583b6a394dd168a4159dfd09".to_string(),
                app_env_vars: btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_db_name),
                     "PG_HOST".to_string() => base64::encode(db_fqdn),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username),
                     "PG_PASSWORD".to_string() => base64::encode(database_password),
                },
            }
        }
        DatabaseKind::Redis => {
            let database_port = 6379;
            let database_db_name = db_id;
            DBInfos {
                db_port: database_port,
                db_name: database_db_name,
                app_commit: "e4b1162741ce162b834b68498e43bf60f0f58cbe".to_string(),
                app_env_vars: btreemap! {
                "IS_ELASTICCACHE".to_string() => base64::encode((database_mode == MANAGED).to_string()),
                "REDIS_HOST".to_string()      => base64::encode(db_fqdn),
                "REDIS_PORT".to_string()      => base64::encode(database_port.to_string()),
                "REDIS_USERNAME".to_string()  => base64::encode(database_username),
                "REDIS_PASSWORD".to_string()  => base64::encode(database_password),
                },
            }
        }
    }
}

pub fn db_disk_type(provider_kind: Kind, database_mode: DatabaseMode) -> String {
    match provider_kind {
        Kind::Aws => "gp2",
        Kind::Do => match database_mode {
            MANAGED => DO_MANAGED_DATABASE_DISK_TYPE,
            DatabaseMode::CONTAINER => DO_SELF_HOSTED_DATABASE_DISK_TYPE,
        },
        Kind::Scw => match database_mode {
            MANAGED => SCW_MANAGED_DATABASE_DISK_TYPE,
            DatabaseMode::CONTAINER => SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
        },
    }
    .to_string()
}

pub fn db_instance_type(provider_kind: Kind, db_kind: DatabaseKind, database_mode: DatabaseMode) -> String {
    match provider_kind {
        Kind::Aws => match db_kind {
            DatabaseKind::Mongodb => "db.t3.medium",
            DatabaseKind::Mysql => "db.t3.micro",
            DatabaseKind::Postgresql => "db.t3.micro",
            DatabaseKind::Redis => "cache.t3.micro",
        },
        Kind::Do => match database_mode {
            MANAGED => DO_MANAGED_DATABASE_INSTANCE_TYPE,
            DatabaseMode::CONTAINER => DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
        },
        Kind::Scw => match database_mode {
            MANAGED => SCW_MANAGED_DATABASE_INSTANCE_TYPE,
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
