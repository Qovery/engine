extern crate base64;
extern crate bstr;
extern crate passwords;
extern crate scaleway_api_rs;

use chrono::Utc;
use curl::easy::Easy;
use dirs::home_dir;
use dotenv::dotenv;
use gethostname;
use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use std::io::{Error, ErrorKind};

use passwords::PasswordGenerator;

use std::env;
use std::sync::Arc;
use tracing::{info, warn};

use crate::helpers::scaleway::{
    SCW_MANAGED_DATABASE_DISK_TYPE, SCW_MANAGED_DATABASE_INSTANCE_TYPE, SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
};
use hashicorp_vault;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::cmd;
use qovery_engine::constants::{
    AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, SCW_ACCESS_KEY, SCW_DEFAULT_PROJECT_ID, SCW_SECRET_KEY,
};
use qovery_engine::io_models::database::{DatabaseKind, DatabaseMode};
use reqwest::header;
use serde::{Deserialize, Serialize};

extern crate time;
use qovery_engine::cloud_provider::aws::database_instance_type::AwsDatabaseInstanceType;
use qovery_engine::cmd::docker::Docker;
use qovery_engine::cmd::kubectl::{kubectl_get_pvc, kubectl_get_svc};
use qovery_engine::cmd::structs::{KubernetesList, KubernetesPod, PVC, SVC};
use qovery_engine::engine::InfrastructureContext;
use qovery_engine::engine_task::qovery_api::{EngineServiceType, StaticQoveryApi};
use qovery_engine::errors::CommandError;
use qovery_engine::events::{EnvironmentStep, EventDetails, Stage, Transmitter};
use qovery_engine::io_models::context::{Context, Features, Metadata};
use qovery_engine::io_models::database::DatabaseMode::MANAGED;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::logger::{Logger, StdIoLogger};
use qovery_engine::models::database::DatabaseInstanceType;
use qovery_engine::utilities::to_short_id;
use time::Instant;
use tracing_subscriber::EnvFilter;
use url::Url;
use uuid::Uuid;

pub fn get_qovery_app_version(api_fqdn: &str) -> anyhow::Result<HashMap<EngineServiceType, String>> {
    #[derive(Deserialize)]
    struct QoveryServiceVersion {
        version: String,
    }

    let mut headers = header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    let http = reqwest::blocking::Client::new();

    let services_version = vec![
        (EngineServiceType::Engine, "ENGINE"),
        (EngineServiceType::ShellAgent, "SHELL_AGENT"),
        (EngineServiceType::ClusterAgent, "CLUSTER_AGENT"),
    ]
    .into_iter()
    .flat_map(|(service_type, service_type_name)| {
        let url = format!("https://{api_fqdn}/engine/serviceVersion?serviceType={service_type_name}");
        info!("fetching version : {}", url);

        let payload = http.get(url).headers(headers.clone()).send()?;
        Result::<_, anyhow::Error>::Ok((service_type, payload.json::<QoveryServiceVersion>()?.version))
    })
    .collect();

    Ok(services_version)
}

fn context(organization_id: Uuid, cluster_id: Uuid, ttl: u32) -> Context {
    let execution_id = execution_id();
    let home_dir = env::var("WORKSPACE_ROOT_DIR").unwrap_or_else(|_| home_dir().unwrap().to_str().unwrap().to_string());
    let lib_root_dir = env::var("LIB_ROOT_DIR").expect("LIB_ROOT_DIR is mandatory");
    let docker_host = env::var("DOCKER_HOST").map(|x| Url::parse(&x).unwrap()).ok();
    let docker = Docker::new_with_local_builder(docker_host).expect("Can't init docker");

    let metadata = Metadata {
        dry_run_deploy: Option::from(env::var_os("dry_run_deploy").is_some()),
        resource_expiration_in_seconds: {
            // set a custom ttl as environment variable for manual tests
            match env::var_os("ttl") {
                Some(ttl) => {
                    let ttl_converted: u32 = ttl.into_string().unwrap().parse().unwrap();
                    Some(ttl_converted)
                }
                None => Some(ttl),
            }
        },
        forced_upgrade: Option::from(env::var_os("forced_upgrade").is_some()),
        disable_pleco: Some(true),
        is_first_cluster_deployment: None,
    };
    let enabled_features = vec![Features::LogsHistory, Features::MetricsHistory];
    let secrets = FuncTestsSecrets::new();
    let versions = get_qovery_app_version(&secrets.QOVERY_API_URL.unwrap()).unwrap();

    Context::new(
        organization_id,
        cluster_id,
        execution_id.to_string(),
        home_dir,
        lib_root_dir,
        true,
        enabled_features,
        Option::from(metadata),
        Arc::new(docker),
        Arc::new(Box::new(StaticQoveryApi { versions })),
        EventDetails::new(
            None,
            QoveryIdentifier::new(organization_id),
            QoveryIdentifier::new(cluster_id),
            execution_id,
            Stage::Environment(EnvironmentStep::LoadConfiguration),
            Transmitter::TaskManager(Uuid::new_v4(), "".to_string()),
        ),
    )
}

pub fn context_for_cluster(organization_id: Uuid, cluster_id: Uuid) -> Context {
    context(organization_id, cluster_id, 14400)
}

pub fn context_for_ec2(organization_id: Uuid, cluster_id: Uuid) -> Context {
    context(organization_id, cluster_id, 7200)
}

pub fn context_for_resource(organization_id: Uuid, cluster_id: Uuid) -> Context {
    context(organization_id, cluster_id, 3600)
}

pub fn logger() -> Box<dyn Logger> {
    Box::new(StdIoLogger::new())
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(non_snake_case)]
pub struct FuncTestsSecrets {
    pub AWS_ACCESS_KEY_ID: Option<String>,
    pub AWS_DEFAULT_REGION: Option<String>,
    pub AWS_EC2_DEFAULT_REGION: Option<String>,
    pub AWS_EC2_TEST_MANAGED_REGION: Option<String>,
    pub AWS_EC2_TEST_CONTAINER_REGION: Option<String>,
    pub AWS_EC2_TEST_INSTANCE_REGION: Option<String>,
    pub AWS_EC2_TEST_CLUSTER_REGION: Option<String>,
    pub AWS_SECRET_ACCESS_KEY: Option<String>,
    pub AWS_TEST_CLUSTER_ID: Option<String>,
    pub AWS_EC2_TEST_CLUSTER_ID: Option<String>,
    pub AWS_EC2_TEST_CLUSTER_LONG_ID: Option<Uuid>,
    pub AWS_TEST_CLUSTER_LONG_ID: Option<Uuid>,
    pub AWS_EC2_TEST_CLUSTER_DOMAIN: Option<String>,
    pub AWS_TEST_ORGANIZATION_ID: Option<String>,
    pub AWS_TEST_ORGANIZATION_LONG_ID: Option<Uuid>,
    pub AWS_TEST_CLUSTER_REGION: Option<String>,
    pub AWS_EC2_DEFAULT_CLUSTER_ID: Option<String>,
    pub BIN_VERSION_FILE: Option<String>,
    pub CLOUDFLARE_DOMAIN: Option<String>,
    pub CLOUDFLARE_ID: Option<String>,
    pub CLOUDFLARE_TOKEN: Option<String>,
    pub CUSTOM_TEST_DOMAIN: Option<String>,
    pub DEFAULT_TEST_DOMAIN: Option<String>,
    pub DISCORD_API_URL: Option<String>,
    pub EKS_ACCESS_CIDR_BLOCKS: Option<String>,
    pub GITHUB_ACCESS_TOKEN: Option<String>,
    pub HTTP_LISTEN_ON: Option<String>,
    pub LETS_ENCRYPT_EMAIL_REPORT: Option<String>,
    pub LIB_ROOT_DIR: Option<String>,
    pub QOVERY_AGENT_CONTROLLER_TOKEN: Option<String>,
    pub QOVERY_API_URL: Option<String>,
    pub QOVERY_ENGINE_CONTROLLER_TOKEN: Option<String>,
    pub QOVERY_SSH_USER: Option<String>,
    pub RUST_LOG: Option<String>,
    pub SCALEWAY_DEFAULT_PROJECT_ID: Option<String>,
    pub SCALEWAY_ACCESS_KEY: Option<String>,
    pub SCALEWAY_SECRET_KEY: Option<String>,
    pub SCALEWAY_DEFAULT_REGION: Option<String>,
    pub SCALEWAY_TEST_CLUSTER_ID: Option<String>,
    pub SCALEWAY_TEST_CLUSTER_LONG_ID: Option<Uuid>,
    pub SCALEWAY_TEST_ORGANIZATION_ID: Option<String>,
    pub SCALEWAY_TEST_ORGANIZATION_LONG_ID: Option<Uuid>,
    pub SCALEWAY_TEST_CLUSTER_REGION: Option<String>,
    pub TERRAFORM_AWS_ACCESS_KEY_ID: Option<String>,
    pub TERRAFORM_AWS_SECRET_ACCESS_KEY: Option<String>,
    pub TERRAFORM_AWS_REGION: Option<String>,
    pub QOVERY_GRPC_URL: Option<String>,
    pub ENGINE_SERVER_URL: Option<String>,
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
        dotenv().ok();
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
            AWS_EC2_DEFAULT_REGION: None,
            AWS_EC2_TEST_MANAGED_REGION: None,
            AWS_EC2_TEST_CONTAINER_REGION: None,
            AWS_EC2_TEST_INSTANCE_REGION: None,
            AWS_EC2_TEST_CLUSTER_REGION: None,
            AWS_SECRET_ACCESS_KEY: None,
            AWS_TEST_CLUSTER_ID: None,
            AWS_EC2_TEST_CLUSTER_ID: None,
            AWS_EC2_TEST_CLUSTER_LONG_ID: None,
            AWS_TEST_CLUSTER_LONG_ID: None,
            AWS_EC2_TEST_CLUSTER_DOMAIN: None,
            AWS_TEST_ORGANIZATION_ID: None,
            AWS_TEST_ORGANIZATION_LONG_ID: None,
            AWS_TEST_CLUSTER_REGION: None,
            AWS_EC2_DEFAULT_CLUSTER_ID: None,
            BIN_VERSION_FILE: None,
            CLOUDFLARE_DOMAIN: None,
            CLOUDFLARE_ID: None,
            CLOUDFLARE_TOKEN: None,
            CUSTOM_TEST_DOMAIN: None,
            DEFAULT_TEST_DOMAIN: None,
            DISCORD_API_URL: None,
            EKS_ACCESS_CIDR_BLOCKS: None,
            GITHUB_ACCESS_TOKEN: None,
            HTTP_LISTEN_ON: None,
            LETS_ENCRYPT_EMAIL_REPORT: None,
            LIB_ROOT_DIR: None,
            QOVERY_AGENT_CONTROLLER_TOKEN: None,
            QOVERY_API_URL: None,
            QOVERY_ENGINE_CONTROLLER_TOKEN: None,
            QOVERY_SSH_USER: None,
            RUST_LOG: None,
            SCALEWAY_ACCESS_KEY: None,
            SCALEWAY_DEFAULT_PROJECT_ID: None,
            SCALEWAY_SECRET_KEY: None,
            SCALEWAY_DEFAULT_REGION: None,
            SCALEWAY_TEST_CLUSTER_ID: None,
            SCALEWAY_TEST_CLUSTER_LONG_ID: None,
            SCALEWAY_TEST_ORGANIZATION_ID: None,
            SCALEWAY_TEST_ORGANIZATION_LONG_ID: None,
            SCALEWAY_TEST_CLUSTER_REGION: None,
            TERRAFORM_AWS_ACCESS_KEY_ID: None,
            TERRAFORM_AWS_SECRET_ACCESS_KEY: None,
            TERRAFORM_AWS_REGION: None,
            QOVERY_GRPC_URL: None,
            ENGINE_SERVER_URL: None,
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
                println!("error: wasn't able to contact Vault server. {e:?}");
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

    fn select_secret<T: for<'a> TryFrom<&'a str>>(name: &str, vault_fallback: Option<T>) -> Option<T> {
        match env::var(name) {
            Ok(x) => T::try_from(x.as_str()).ok(),
            Err(_) if vault_fallback.is_some() => vault_fallback,
            Err(_) => None,
        }
    }

    fn get_all_secrets() -> FuncTestsSecrets {
        let secrets = Self::get_secrets_from_vault();

        FuncTestsSecrets {
            AWS_ACCESS_KEY_ID: Self::select_secret("AWS_ACCESS_KEY_ID", secrets.AWS_ACCESS_KEY_ID),
            AWS_DEFAULT_REGION: Self::select_secret("AWS_DEFAULT_REGION", secrets.AWS_DEFAULT_REGION),
            AWS_EC2_DEFAULT_REGION: Self::select_secret("AWS_EC2_DEFAULT_REGION", secrets.AWS_EC2_DEFAULT_REGION),
            AWS_EC2_TEST_MANAGED_REGION: Self::select_secret(
                "AWS_EC2_TEST_MANAGED_REGION",
                secrets.AWS_EC2_TEST_MANAGED_REGION,
            ),
            AWS_EC2_TEST_CONTAINER_REGION: Self::select_secret(
                "AWS_EC2_TEST_CONTAINER_REGION",
                secrets.AWS_EC2_TEST_CONTAINER_REGION,
            ),
            AWS_EC2_TEST_INSTANCE_REGION: Self::select_secret(
                "AWS_EC2_TEST_INSTANCE_REGION",
                secrets.AWS_EC2_TEST_INSTANCE_REGION,
            ),
            AWS_EC2_TEST_CLUSTER_REGION: Self::select_secret(
                "AWS_EC2_TEST_CLUSTER_REGION",
                secrets.AWS_EC2_TEST_CLUSTER_REGION,
            ),
            AWS_SECRET_ACCESS_KEY: Self::select_secret("AWS_SECRET_ACCESS_KEY", secrets.AWS_SECRET_ACCESS_KEY),
            AWS_TEST_ORGANIZATION_ID: Self::select_secret("AWS_TEST_ORGANIZATION_ID", secrets.AWS_TEST_ORGANIZATION_ID),
            AWS_TEST_ORGANIZATION_LONG_ID: Self::select_secret(
                "AWS_TEST_ORGANIZATION_LONG_ID",
                secrets.AWS_TEST_ORGANIZATION_LONG_ID,
            ),
            AWS_TEST_CLUSTER_REGION: Self::select_secret("AWS_TEST_CLUSTER_REGION", secrets.AWS_TEST_CLUSTER_REGION),
            AWS_TEST_CLUSTER_ID: Self::select_secret("AWS_TEST_CLUSTER_ID", secrets.AWS_TEST_CLUSTER_ID),
            AWS_EC2_TEST_CLUSTER_ID: Self::select_secret("AWS_EC2_TEST_CLUSTER_ID", secrets.AWS_EC2_TEST_CLUSTER_ID),
            AWS_EC2_TEST_CLUSTER_LONG_ID: Self::select_secret(
                "AWS_EC2_TEST_CLUSTER_LONG_ID",
                secrets.AWS_EC2_TEST_CLUSTER_LONG_ID,
            ),
            AWS_TEST_CLUSTER_LONG_ID: Self::select_secret("AWS_TEST_CLUSTER_LONG_ID", secrets.AWS_TEST_CLUSTER_LONG_ID),
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
            SCALEWAY_TEST_ORGANIZATION_LONG_ID: Self::select_secret(
                "SCALEWAY_TEST_ORGANIZATION_LONG_ID",
                secrets.SCALEWAY_TEST_ORGANIZATION_LONG_ID,
            ),
            SCALEWAY_TEST_CLUSTER_ID: Self::select_secret("SCALEWAY_TEST_CLUSTER_ID", secrets.SCALEWAY_TEST_CLUSTER_ID),
            SCALEWAY_TEST_CLUSTER_LONG_ID: Self::select_secret(
                "SCALEWAY_TEST_CLUSTER_LONG_ID",
                secrets.SCALEWAY_TEST_CLUSTER_LONG_ID,
            ),
            SCALEWAY_TEST_CLUSTER_REGION: Self::select_secret(
                "SCALEWAY_TEST_CLUSTER_REGION",
                secrets.SCALEWAY_TEST_CLUSTER_REGION,
            ),
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
            ENGINE_SERVER_URL: Self::select_secret("ENGINE_SERVER_URL", secrets.ENGINE_SERVER_URL),
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

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(context.clone(), Uuid::new_v4(), "qovery-local-docker").unwrap()
}

pub fn init() -> Instant {
    let ci_var = "CI";

    dotenv().ok();
    let _ = match env::var_os(ci_var) {
        Some(_) => tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_current_span(true)
            .try_init(),
        None => {
            if env::var_os("RUST_LOG").is_none() {
                env::set_var("RUST_LOG", "INFO")
            }
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::try_from_env("RUST_LOG").unwrap())
                .try_init()
        }
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

pub fn generate_id() -> Uuid {
    Uuid::new_v4()
}

pub fn generate_password(db_mode: DatabaseMode) -> String {
    // core special chars set: !#$%&*+-=?_
    // we will keep only those and exclude others
    let forbidden_chars = vec![
        '"', '\'', '(', ')', ',', '.', '/', ':', ';', '<', '>', '@', '[', '\\', ']', '^', '`', '{', '|', '}', '~', '%',
        '*',
    ];

    let pg = PasswordGenerator::new()
        .length(32)
        .numbers(true)
        .lowercase_letters(true)
        .uppercase_letters(true)
        .symbols(db_mode == MANAGED)
        .spaces(false)
        .exclude_similar_characters(true)
        .strict(true);

    let mut password = pg.generate_one().expect("error while trying to generate a password");

    for forbidden_char in forbidden_chars {
        password = password.replace(forbidden_char, "z");
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
            println!("TEST Error : while trying to call {e}");
            false
        }
    }
}

type KubernetesCredentials<'a> = Vec<(&'a str, &'a str)>;

fn get_cloud_provider_credentials(provider_kind: Kind, secrets: &FuncTestsSecrets) -> KubernetesCredentials {
    match provider_kind {
        Kind::Aws => vec![
            (AWS_ACCESS_KEY_ID, secrets.AWS_ACCESS_KEY_ID.as_ref().unwrap().as_str()),
            (AWS_SECRET_ACCESS_KEY, secrets.AWS_SECRET_ACCESS_KEY.as_ref().unwrap().as_str()),
        ],
        Kind::Scw => vec![
            (SCW_ACCESS_KEY, secrets.SCALEWAY_ACCESS_KEY.as_ref().unwrap().as_str()),
            (SCW_SECRET_KEY, secrets.SCALEWAY_SECRET_KEY.as_ref().unwrap().as_str()),
            (
                SCW_DEFAULT_PROJECT_ID,
                secrets.SCALEWAY_DEFAULT_PROJECT_ID.as_ref().unwrap().as_str(),
            ),
        ],
    }
}

pub fn is_pod_restarted_env(
    infra_ctx: &InfrastructureContext,
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

    let kubeconfig = infra_ctx.kubernetes().get_kubeconfig_file_path();
    assert!(kubeconfig.is_ok());

    match kubeconfig {
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
    infra_ctx: &InfrastructureContext,
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

    let kubeconfig = infra_ctx.kubernetes().get_kubeconfig_file_path();
    assert!(kubeconfig.is_ok());

    cmd::kubectl::kubectl_exec_get_pods(
        kubeconfig.unwrap().as_str(),
        Some(namespace_name.as_str()),
        Some(pod_to_check),
        get_cloud_provider_credentials(provider_kind, &secrets),
    )
}

pub fn execution_id() -> String {
    Utc::now().to_rfc3339().replace([':', '.', '+'], "-")
}

// avoid test collisions
pub fn generate_cluster_id(region: &str) -> Uuid {
    let check_if_running_on_gitlab_env_var = "CI_PROJECT_TITLE";

    // if running on CI, generate an ID
    if env::var_os(check_if_running_on_gitlab_env_var).is_some() {
        let id = generate_id();
        info!("Generated cluster ID: {}", id);
        return id;
    };

    match gethostname::gethostname().into_string() {
        // shrink to 15 chars in order to avoid resources name issues
        Ok(current_name) => {
            let mut bytes: [u8; 16] = [0; 16];
            for byte in current_name.as_bytes() {
                bytes[*byte as usize % 16] = bytes[*byte as usize % 16].wrapping_add(*byte);
            }

            for byte in region.bytes() {
                bytes[byte as usize % 16] = bytes[byte as usize % 16].wrapping_add(byte);
            }
            Uuid::from_bytes(bytes)
        }
        _ => generate_id(),
    }
}

// avoid test collisions
pub fn generate_organization_id(region: &str) -> Uuid {
    let check_if_running_on_gitlab_env_var = "CI_PROJECT_TITLE";

    // if running on CI, generate an ID
    if env::var_os(check_if_running_on_gitlab_env_var).is_some() {
        let id = generate_id();
        info!("Generated organization ID: {}", id);
        return id;
    };

    match gethostname::gethostname().into_string() {
        // shrink to 15 chars in order to avoid resources name issues
        Ok(current_name) => {
            let reversed_name = current_name.as_str().chars().rev().collect::<String>();
            let mut bytes: [u8; 16] = [0; 16];
            for byte in reversed_name.as_bytes() {
                bytes[*byte as usize % 16] = bytes[*byte as usize % 16].wrapping_add(*byte);
            }

            for byte in region.bytes() {
                bytes[byte as usize % 16] = bytes[byte as usize % 16].wrapping_add(byte);
            }
            Uuid::from_bytes(bytes)
        }
        _ => generate_id(),
    }
}

pub fn get_pvc(
    infra_ctx: &InfrastructureContext,
    provider_kind: Kind,
    environment_check: EnvironmentRequest,
    secrets: FuncTestsSecrets,
) -> Result<PVC, CommandError> {
    let namespace_name = format!(
        "{}-{}",
        to_short_id(&environment_check.project_long_id),
        to_short_id(&environment_check.long_id),
    );

    let kubeconfig = infra_ctx.kubernetes().get_kubeconfig_file_path();
    assert!(kubeconfig.is_ok());

    match kubeconfig {
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
        Err(e) => Err(CommandError::new_from_safe_message(e.to_string())),
    }
}

pub fn get_svc(
    infra_ctx: &InfrastructureContext,
    provider_kind: Kind,
    environment_check: EnvironmentRequest,
    secrets: FuncTestsSecrets,
) -> Result<SVC, CommandError> {
    let namespace_name = format!(
        "{}-{}",
        to_short_id(&environment_check.project_long_id),
        to_short_id(&environment_check.long_id),
    );

    let kubeconfig = infra_ctx.kubernetes().get_kubeconfig_file_path();
    assert!(kubeconfig.is_ok());

    match kubeconfig {
        Ok(path) => {
            match kubectl_get_svc(
                path.as_str(),
                namespace_name.as_str(),
                get_cloud_provider_credentials(provider_kind, &secrets),
            ) {
                Ok(svc) => Ok(svc),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(CommandError::new_from_safe_message(e.to_string())),
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
                "mongodb://{database_username}:{database_password}@{db_fqdn}:{database_port}/{database_db_name}"
            );
            DBInfos {
                db_port: database_port,
                db_name: database_db_name.to_string(),
                app_commit: "c5da00d2463061787e5fc2e31e7cd67877fd9881".to_string(),
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
                app_commit: "0c73aac9bbab7f494da1d89a535ed40e668a8ab4".to_string(),
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
                app_commit: "71990e977a60c87034530614607494a96dee2254".to_string(),
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
                app_commit: "d41af121c648cd119a1d7aebecadddc7e8a6e548".to_string(),
                app_env_vars: btreemap! {
                "IS_ELASTICCACHE".to_string() => base64::encode((database_mode == MANAGED && database_username == "default").to_string()),
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
        Kind::Scw => match database_mode {
            MANAGED => SCW_MANAGED_DATABASE_DISK_TYPE,
            DatabaseMode::CONTAINER => SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
        },
    }
    .to_string()
}

pub fn db_instance_type(
    provider_kind: Kind,
    db_kind: DatabaseKind,
    database_mode: DatabaseMode,
) -> Option<Box<dyn DatabaseInstanceType>> {
    match provider_kind {
        Kind::Aws => match db_kind {
            DatabaseKind::Mongodb => Some(Box::new(AwsDatabaseInstanceType::DB_T3_MEDIUM)),
            DatabaseKind::Mysql => Some(Box::new(AwsDatabaseInstanceType::DB_T3_MICRO)),
            DatabaseKind::Postgresql => Some(Box::new(AwsDatabaseInstanceType::DB_T3_MICRO)),
            DatabaseKind::Redis => Some(Box::new(AwsDatabaseInstanceType::CACHE_T3_MICRO)),
        },
        Kind::Scw => match database_mode {
            MANAGED => Some(Box::new(SCW_MANAGED_DATABASE_INSTANCE_TYPE)),
            DatabaseMode::CONTAINER => None,
        },
    }
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
