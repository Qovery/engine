extern crate base64;
extern crate bstr;
extern crate passwords;
extern crate scaleway_api_rs;
extern crate time;

use base64::Engine;
use base64::engine::general_purpose;
use chrono::Utc;
use curl::easy::Easy;
use dirs::home_dir;
use dotenv::dotenv;
use gethostname;
use hashicorp_vault;
use passwords::PasswordGenerator;
use reqwest::header;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use std::io::{Error, ErrorKind};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{env, io};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use url::Url;
use uuid::Uuid;

use crate::helpers::azure::AZURE_SELF_HOSTED_DATABASE_DISK_TYPE;
use crate::helpers::common::{DEFAULT_QUICK_RESOURCE_TTL_IN_SECONDS, DEFAULT_RESOURCE_TTL_IN_SECONDS};
use crate::helpers::gcp::GCP_SELF_HOSTED_DATABASE_DISK_TYPE;
use crate::helpers::scaleway::{
    SCW_MANAGED_DATABASE_DISK_TYPE, SCW_MANAGED_DATABASE_INSTANCE_TYPE, SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
};
use qovery_engine::cmd;
use qovery_engine::cmd::docker::Docker;
use qovery_engine::cmd::kubectl::{kubectl_get_pvc, kubectl_get_svc};
use qovery_engine::cmd::structs::{KubernetesList, KubernetesPod, PVC, SVC};
use qovery_engine::constants::{
    AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, SCW_ACCESS_KEY, SCW_DEFAULT_PROJECT_ID, SCW_SECRET_KEY,
};
use qovery_engine::engine_task::qovery_api::{EngineServiceType, StaticQoveryApi};
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::environment::models::aws::AwsStorageType;
use qovery_engine::environment::models::database::DatabaseInstanceType;
use qovery_engine::environment::report::obfuscation_service::{ObfuscationService, StdObfuscationService};
use qovery_engine::errors::CommandError;
use qovery_engine::events::{EnvironmentStep, EventDetails, Stage, Transmitter};
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::build_platform::local_docker::LocalDocker;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::aws::database_instance_type::AwsDatabaseInstanceType;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::io_models::context::{Context, Features, Metadata};
use qovery_engine::io_models::database::{DatabaseKind, DatabaseMode};
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::logger::{Logger, StdIoLogger};
use qovery_engine::metrics_registry::{MetricsRegistry, StdMetricsRegistry};
use qovery_engine::msg_publisher::StdMsgPublisher;

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

fn context(organization_id: Uuid, cluster_id: Uuid, ttl: u32, kind: Option<KKind>) -> Context {
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
        is_first_cluster_deployment: Some(false),
    };
    // todo(pmavro): temporary remove metrics while implementing them
    // let mut enabled_features = vec![Features::LogsHistory, Features::MetricsHistory];
    let mut enabled_features = vec![Features::LogsHistory];
    if let Some(kkind) = kind {
        if kkind == KKind::Eks {
            enabled_features.push(Features::Grafana)
        }
    }
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
        Arc::new(StaticQoveryApi { versions }),
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

pub fn context_for_cluster(organization_id: Uuid, cluster_id: Uuid, kind: Option<KKind>) -> Context {
    let mut ctx = context(organization_id, cluster_id, 14400, kind);
    ctx.update_is_first_cluster_deployment(true);
    ctx
}

pub fn context_for_ec2(organization_id: Uuid, cluster_id: Uuid) -> Context {
    context(organization_id, cluster_id, DEFAULT_RESOURCE_TTL_IN_SECONDS, None)
}

pub fn context_for_resource(organization_id: Uuid, cluster_id: Uuid) -> Context {
    context(organization_id, cluster_id, DEFAULT_QUICK_RESOURCE_TTL_IN_SECONDS, None)
}

pub fn logger() -> Box<dyn Logger> {
    Box::new(StdIoLogger::new())
}

pub fn metrics_registry() -> Box<dyn MetricsRegistry> {
    Box::new(StdMetricsRegistry::new(Box::new(StdMsgPublisher::new())))
}

pub fn obfuscation_service() -> Box<dyn ObfuscationService> {
    Box::new(StdObfuscationService::new(vec![]))
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(non_snake_case)]
pub struct FuncTestsSecrets {
    pub AWS_ACCESS_KEY_ID: Option<String>,
    pub AWS_DEFAULT_REGION: Option<String>,
    pub AWS_TEST_KUBECONFIG_b64: Option<String>,
    pub AWS_SECRET_ACCESS_KEY: Option<String>,
    pub AWS_SESSION_TOKEN: Option<String>,
    pub AWS_TEST_CLUSTER_ID: Option<String>,
    pub AWS_TEST_CLUSTER_LONG_ID: Option<Uuid>,
    pub AWS_TEST_ORGANIZATION_ID: Option<String>,
    pub AWS_TEST_ORGANIZATION_LONG_ID: Option<Uuid>,
    pub AWS_TEST_CLUSTER_REGION: Option<String>,
    pub AZURE_STORAGE_ACCOUNT: Option<String>,
    pub AZURE_STORAGE_ACCESS_KEY: Option<String>,
    pub AZURE_TENANT_ID: Option<String>,
    pub AZURE_CLIENT_ID: Option<String>,
    pub AZURE_CLIENT_SECRET: Option<String>,
    pub AZURE_SUBSCRIPTION_ID: Option<String>,
    pub AZURE_TEST_KUBECONFIG_b64: Option<String>,
    pub BIN_VERSION_FILE: Option<String>,
    pub CLOUDFLARE_DOMAIN: Option<String>,
    pub CLOUDFLARE_ID: Option<String>,
    pub CLOUDFLARE_TOKEN: Option<String>,
    pub CUSTOM_TEST_DOMAIN: Option<String>,
    pub DEFAULT_TEST_DOMAIN: Option<String>,
    pub DISCORD_API_URL: Option<String>,
    pub GCP_CREDENTIALS: Option<String>,
    pub GCP_PROJECT_NAME: Option<String>,
    pub GCP_TEST_ORGANIZATION_ID: Option<String>,
    pub GCP_TEST_ORGANIZATION_LONG_ID: Option<Uuid>,
    pub GCP_TEST_CLUSTER_ID: Option<String>,
    pub GCP_TEST_CLUSTER_LONG_ID: Option<Uuid>,
    pub GCP_DEFAULT_REGION: Option<String>,
    pub GCP_TEST_KUBECONFIG_b64: Option<String>,
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
    pub SCALEWAY_TEST_KUBECONFIG_b64: Option<String>,
    pub TERRAFORM_AWS_ACCESS_KEY_ID: Option<String>,
    pub TERRAFORM_AWS_SECRET_ACCESS_KEY: Option<String>,
    pub TERRAFORM_AWS_REGION: Option<String>,
    pub TERRAFORM_AWS_BUCKET: Option<String>,
    pub TERRAFORM_AWS_DYNAMODB_TABLE: Option<String>,
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
                ));
            }
        };

        let vault_token = match env::var_os("VAULT_TOKEN") {
            Some(x) => x.into_string().unwrap(),
            None => {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    "VAULT_TOKEN environment variable is missing".to_string(),
                ));
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
            AWS_TEST_KUBECONFIG_b64: None,
            AWS_SECRET_ACCESS_KEY: None,
            AWS_SESSION_TOKEN: None,
            AWS_TEST_CLUSTER_ID: None,
            AWS_TEST_CLUSTER_LONG_ID: None,
            AWS_TEST_ORGANIZATION_ID: None,
            AWS_TEST_ORGANIZATION_LONG_ID: None,
            AWS_TEST_CLUSTER_REGION: None,
            AZURE_STORAGE_ACCOUNT: None,
            AZURE_STORAGE_ACCESS_KEY: None,
            AZURE_CLIENT_ID: None,
            AZURE_CLIENT_SECRET: None,
            AZURE_SUBSCRIPTION_ID: None,
            AZURE_TENANT_ID: None,
            AZURE_TEST_KUBECONFIG_b64: None,
            BIN_VERSION_FILE: None,
            CLOUDFLARE_DOMAIN: None,
            CLOUDFLARE_ID: None,
            CLOUDFLARE_TOKEN: None,
            CUSTOM_TEST_DOMAIN: None,
            DEFAULT_TEST_DOMAIN: None,
            DISCORD_API_URL: None,
            GCP_CREDENTIALS: None,
            GCP_PROJECT_NAME: None,
            GCP_TEST_ORGANIZATION_ID: None,
            GCP_TEST_ORGANIZATION_LONG_ID: None,
            GCP_TEST_CLUSTER_ID: None,
            GCP_TEST_CLUSTER_LONG_ID: None,
            GCP_DEFAULT_REGION: None,
            GCP_TEST_KUBECONFIG_b64: None,
            GITHUB_ACCESS_TOKEN: None,
            HTTP_LISTEN_ON: None,
            LETS_ENCRYPT_EMAIL_REPORT: None,
            LIB_ROOT_DIR: None,
            QOVERY_AGENT_CONTROLLER_TOKEN: None,
            QOVERY_API_URL: None,
            QOVERY_ENGINE_CONTROLLER_TOKEN: None,
            QOVERY_SSH_USER: None,
            RUST_LOG: None,
            SCALEWAY_DEFAULT_PROJECT_ID: None,
            SCALEWAY_ACCESS_KEY: None,
            SCALEWAY_SECRET_KEY: None,
            SCALEWAY_DEFAULT_REGION: None,
            SCALEWAY_TEST_CLUSTER_ID: None,
            SCALEWAY_TEST_CLUSTER_LONG_ID: None,
            SCALEWAY_TEST_ORGANIZATION_ID: None,
            SCALEWAY_TEST_ORGANIZATION_LONG_ID: None,
            SCALEWAY_TEST_CLUSTER_REGION: None,
            SCALEWAY_TEST_KUBECONFIG_b64: None,
            TERRAFORM_AWS_ACCESS_KEY_ID: None,
            TERRAFORM_AWS_SECRET_ACCESS_KEY: None,
            TERRAFORM_AWS_REGION: None,
            TERRAFORM_AWS_BUCKET: None,
            TERRAFORM_AWS_DYNAMODB_TABLE: None,
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
                warn!(
                    "Empty config is returned as no VAULT connection can be established. If not not expected, check your environment variables"
                );
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
            AWS_TEST_KUBECONFIG_b64: Self::select_secret(
                "AWS_TEST_KUBECONFIG",
                String::from_utf8(
                    general_purpose::STANDARD
                        .decode(secrets.AWS_TEST_KUBECONFIG_b64.as_ref().unwrap())
                        .unwrap(),
                )
                .ok(),
            ),
            AWS_SECRET_ACCESS_KEY: Self::select_secret("AWS_SECRET_ACCESS_KEY", secrets.AWS_SECRET_ACCESS_KEY),
            AWS_SESSION_TOKEN: Self::select_secret("AWS_SESSION_TOKEN", secrets.AWS_SESSION_TOKEN),
            AWS_TEST_ORGANIZATION_ID: Self::select_secret("AWS_TEST_ORGANIZATION_ID", secrets.AWS_TEST_ORGANIZATION_ID),
            AWS_TEST_ORGANIZATION_LONG_ID: Self::select_secret(
                "AWS_TEST_ORGANIZATION_LONG_ID",
                secrets.AWS_TEST_ORGANIZATION_LONG_ID,
            ),
            AWS_TEST_CLUSTER_REGION: Self::select_secret("AWS_TEST_CLUSTER_REGION", secrets.AWS_TEST_CLUSTER_REGION),
            AWS_TEST_CLUSTER_ID: Self::select_secret("AWS_TEST_CLUSTER_ID", secrets.AWS_TEST_CLUSTER_ID),
            AWS_TEST_CLUSTER_LONG_ID: Self::select_secret("AWS_TEST_CLUSTER_LONG_ID", secrets.AWS_TEST_CLUSTER_LONG_ID),
            AZURE_STORAGE_ACCOUNT: Self::select_secret("AZURE_STORAGE_ACCOUNT", secrets.AZURE_STORAGE_ACCOUNT),
            AZURE_STORAGE_ACCESS_KEY: Self::select_secret("AZURE_STORAGE_ACCESS_KEY", secrets.AZURE_STORAGE_ACCESS_KEY),
            AZURE_CLIENT_ID: Self::select_secret("AZURE_CLIENT_ID", secrets.AZURE_CLIENT_ID),
            AZURE_CLIENT_SECRET: Self::select_secret("AZURE_CLIENT_SECRET", secrets.AZURE_CLIENT_SECRET),
            AZURE_TENANT_ID: Self::select_secret("AZURE_TENANT_ID", secrets.AZURE_TENANT_ID),
            AZURE_SUBSCRIPTION_ID: Self::select_secret("AZURE_SUBSCRIPTION_ID", secrets.AZURE_SUBSCRIPTION_ID),
            AZURE_TEST_KUBECONFIG_b64: Self::select_secret(
                "AZURE_TEST_KUBECONFIG",
                String::from_utf8(
                    general_purpose::STANDARD
                        .decode(secrets.AZURE_TEST_KUBECONFIG_b64.as_ref().unwrap())
                        .unwrap(),
                )
                .ok(),
            ),
            BIN_VERSION_FILE: Self::select_secret("BIN_VERSION_FILE", secrets.BIN_VERSION_FILE),
            CLOUDFLARE_DOMAIN: Self::select_secret("CLOUDFLARE_DOMAIN", secrets.CLOUDFLARE_DOMAIN),
            CLOUDFLARE_ID: Self::select_secret("CLOUDFLARE_ID", secrets.CLOUDFLARE_ID),
            CLOUDFLARE_TOKEN: Self::select_secret("CLOUDFLARE_TOKEN", secrets.CLOUDFLARE_TOKEN),
            CUSTOM_TEST_DOMAIN: Self::select_secret("CUSTOM_TEST_DOMAIN", secrets.CUSTOM_TEST_DOMAIN),
            DEFAULT_TEST_DOMAIN: Self::select_secret("DEFAULT_TEST_DOMAIN", secrets.DEFAULT_TEST_DOMAIN),
            DISCORD_API_URL: Self::select_secret("DISCORD_API_URL", secrets.DISCORD_API_URL),
            GCP_CREDENTIALS: Self::select_secret("GCP_CREDENTIALS", secrets.GCP_CREDENTIALS),
            GCP_PROJECT_NAME: Self::select_secret("GCP_PROJECT_NAME", secrets.GCP_PROJECT_NAME),
            GCP_DEFAULT_REGION: Self::select_secret("GCP_DEFAULT_REGION", secrets.GCP_DEFAULT_REGION),
            GCP_TEST_ORGANIZATION_ID: Self::select_secret("GCP_TEST_ORGANIZATION_ID", secrets.GCP_TEST_ORGANIZATION_ID),
            GCP_TEST_ORGANIZATION_LONG_ID: Self::select_secret(
                "GCP_TEST_ORGANIZATION_LONG_ID",
                secrets.GCP_TEST_ORGANIZATION_LONG_ID,
            ),
            GCP_TEST_CLUSTER_ID: Self::select_secret("GCP_TEST_CLUSTER_ID", secrets.GCP_TEST_CLUSTER_ID),
            GCP_TEST_CLUSTER_LONG_ID: Self::select_secret("GCP_TEST_CLUSTER_LONG_ID", secrets.GCP_TEST_CLUSTER_LONG_ID),
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
            SCALEWAY_TEST_KUBECONFIG_b64: Self::select_secret(
                "SCALEWAY_TEST_KUBECONFIG",
                String::from_utf8(
                    general_purpose::STANDARD
                        .decode(secrets.SCALEWAY_TEST_KUBECONFIG_b64.as_ref().unwrap())
                        .unwrap(),
                )
                .ok(),
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
            TERRAFORM_AWS_BUCKET: Self::select_secret("TERRAFORM_AWS_BUCKET", secrets.TERRAFORM_AWS_BUCKET),
            TERRAFORM_AWS_DYNAMODB_TABLE: Self::select_secret(
                "TERRAFORM_AWS_DYNAMODB_TABLE",
                secrets.TERRAFORM_AWS_DYNAMODB_TABLE,
            ),
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
            GCP_TEST_KUBECONFIG_b64: Self::select_secret(
                "GCP_TEST_KUBECONFIG",
                String::from_utf8(
                    general_purpose::STANDARD
                        .decode(secrets.GCP_TEST_KUBECONFIG_b64.as_ref().unwrap())
                        .unwrap(),
                )
                .ok(),
            ),
        }
    }
}

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(
        context.clone(),
        Uuid::new_v4(),
        "qovery-local-docker",
        Box::<StdMetricsRegistry>::default(),
    )
    .unwrap()
}

pub fn init() -> Instant {
    let ci_var = "CI";

    dotenv().ok();
    let _ = match env::var_os(ci_var) {
        Some(_) => tracing_subscriber::fmt()
            .json()
            .compact()
            .with_max_level(tracing::Level::INFO)
            .try_init(),
        None => {
            if env::var_os("RUST_LOG").is_none() {
                unsafe { env::set_var("RUST_LOG", "INFO") }
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
    info!("{} seconds for test {}", elapsed.as_secs_f64(), test_name);
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
        .symbols(db_mode == DatabaseMode::MANAGED)
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

type KubernetesCredentials = Vec<(String, String)>;

fn get_cloud_provider_credentials(provider_kind: Kind) -> KubernetesCredentials {
    let secrets = FuncTestsSecrets::new();
    match provider_kind {
        Kind::Aws => vec![
            (
                AWS_ACCESS_KEY_ID.to_string(),
                secrets
                    .AWS_ACCESS_KEY_ID
                    .expect("AWS_ACCESS_KEY_ID is not set in secrets"),
            ),
            (
                AWS_SECRET_ACCESS_KEY.to_string(),
                secrets
                    .AWS_SECRET_ACCESS_KEY
                    .expect("AWS_SECRET_ACCESS_KEY is not set in secrets"),
            ),
        ],
        Kind::Scw => vec![
            (
                SCW_ACCESS_KEY.to_string(),
                secrets
                    .SCALEWAY_ACCESS_KEY
                    .expect("SCALEWAY_ACCESS_KEY is not set in secrets"),
            ),
            (
                SCW_SECRET_KEY.to_string(),
                secrets
                    .SCALEWAY_SECRET_KEY
                    .expect("SCALEWAY_SECRET_KEY is not set in secrets"),
            ),
            (
                SCW_DEFAULT_PROJECT_ID.to_string(),
                secrets
                    .SCALEWAY_DEFAULT_PROJECT_ID
                    .expect("SCALEWAY_DEFAULT_PROJECT_ID is not set in secrets"),
            ),
        ],
        Kind::Azure => vec![],
        Kind::Gcp => vec![],
        Kind::OnPremise => vec![],
    }
}

pub fn is_pod_restarted_env(
    infra_ctx: &InfrastructureContext,
    provider_kind: Kind,
    environment_check: &EnvironmentRequest,
    service_id: &Uuid,
) -> (bool, String) {
    let kubeconfig = infra_ctx.kubernetes().kubeconfig_local_file_path();

    let restarted_database = cmd::kubectl::kubectl_exec_get_number_of_restart(
        kubeconfig,
        environment_check.kube_name.as_str(),
        service_id,
        get_cloud_provider_credentials(provider_kind)
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect(),
    );
    match restarted_database {
        Ok(count) => match count.trim().eq("0") {
            true => (true, "0".to_string()),
            false => (true, count.to_string()),
        },
        _ => (false, "".to_string()),
    }
}

pub fn get_pods(
    infra_ctx: &InfrastructureContext,
    provider_kind: Kind,
    environment_check: &EnvironmentRequest,
    service_id: &Uuid,
) -> Result<KubernetesList<KubernetesPod>, CommandError> {
    cmd::kubectl::kubectl_exec_get_pods(
        infra_ctx.kubernetes().kubeconfig_local_file_path(),
        Some(environment_check.kube_name.as_str()),
        Some(&format!("qovery.com/service-id={}", service_id)),
        get_cloud_provider_credentials(provider_kind)
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect(),
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

#[cfg(test)]
pub fn get_pvc(
    infra_ctx: &InfrastructureContext,
    provider_kind: Kind,
    environment_check: &EnvironmentRequest,
) -> Result<PVC, CommandError> {
    kubectl_get_pvc(
        infra_ctx.kubernetes().kubeconfig_local_file_path(),
        &environment_check.kube_name,
        get_cloud_provider_credentials(provider_kind)
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect(),
    )
}

#[cfg(test)]
pub fn get_svc(
    infra_ctx: &InfrastructureContext,
    provider_kind: Kind,
    environment_check: EnvironmentRequest,
) -> Result<SVC, CommandError> {
    kubectl_get_svc(
        infra_ctx.kubernetes().kubeconfig_local_file_path(),
        &environment_check.kube_name,
        get_cloud_provider_credentials(provider_kind)
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect(),
    )
}

pub struct DBInfos {
    pub db_port: u16,
    pub db_name: String,
    pub app_commit: String,
    pub app_env_vars: BTreeMap<String, VariableInfo>,
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
                app_commit: "5884401a4151f29e29718f1dc6635ff25c5f2e18".to_string(),
                app_env_vars: btreemap! {
                    "IS_DOCUMENTDB".to_string() => VariableInfo { value: general_purpose::STANDARD.encode((database_mode == DatabaseMode::MANAGED).to_string()), is_secret:false},
                    "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(db_fqdn), is_secret:false},
                    "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_uri), is_secret:false},
                    "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_port.to_string()), is_secret:false},
                    "MONGODB_DBNAME".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_db_name), is_secret:false},
                    "QOVERY_DATABASE_TESTING_DATABASE_USERNAME".to_string() =>VariableInfo { value:  general_purpose::STANDARD.encode(database_username), is_secret:false},
                    "QOVERY_DATABASE_TESTING_DATABASE_PASSWORD".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_password), is_secret:false},
                },
            }
        }
        DatabaseKind::Mysql => {
            let database_port = 3306;
            let database_db_name = db_id;
            DBInfos {
                db_port: database_port,
                db_name: database_db_name.to_string(),
                app_commit: "ef8df03b56d942424dc4943ffb9d8d69431e72bb".to_string(),
                app_env_vars: btreemap! {
                    "MYSQL_HOST".to_string() =>VariableInfo { value: general_purpose::STANDARD.encode(db_fqdn), is_secret:false},
                    "MYSQL_PORT".to_string() => VariableInfo { value:general_purpose::STANDARD.encode(database_port.to_string()), is_secret:false},
                    "MYSQL_DBNAME".to_string()   => VariableInfo { value:general_purpose::STANDARD.encode(database_db_name), is_secret:false},
                    "MYSQL_USERNAME".to_string() => VariableInfo { value:general_purpose::STANDARD.encode(database_username), is_secret:false},
                    "MYSQL_PASSWORD".to_string() => VariableInfo { value:general_purpose::STANDARD.encode(database_password), is_secret:false},
                },
            }
        }
        DatabaseKind::Postgresql => {
            let database_port = 5432;
            let database_db_name = if database_mode == DatabaseMode::MANAGED {
                "postgres".to_string()
            } else {
                db_id
            };
            DBInfos {
                db_port: database_port,
                db_name: database_db_name.to_string(),
                app_commit: "f379e5b937c743adf96f9484956260da170bb93c".to_string(),
                app_env_vars: btreemap! {
                     "PG_DBNAME".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_db_name), is_secret:false},
                     "PG_HOST".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(db_fqdn), is_secret:false},
                     "PG_PORT".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_port.to_string()), is_secret:false},
                     "PG_USERNAME".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_username), is_secret:false},
                     "PG_PASSWORD".to_string() => VariableInfo { value: general_purpose::STANDARD.encode(database_password), is_secret:false},
                },
            }
        }
        DatabaseKind::Redis => {
            let database_port = 6379;
            let database_db_name = db_id;
            DBInfos {
                db_port: database_port,
                db_name: database_db_name,
                app_commit: "c8dd8b57a4ebafabc860f0b948f881dad5ab632e".to_string(),
                app_env_vars: btreemap! {
                "IS_ELASTICCACHE".to_string() => VariableInfo { value: general_purpose::STANDARD.encode((database_mode == DatabaseMode::MANAGED && database_username == "default").to_string()), is_secret:false},
                "REDIS_HOST".to_string()      => VariableInfo { value: general_purpose::STANDARD.encode(db_fqdn), is_secret:false},
                "REDIS_PORT".to_string()      =>VariableInfo { value:  general_purpose::STANDARD.encode(database_port.to_string()), is_secret:false},
                "REDIS_USERNAME".to_string()  => VariableInfo { value: general_purpose::STANDARD.encode(database_username), is_secret:false},
                "REDIS_PASSWORD".to_string()  =>VariableInfo { value:  general_purpose::STANDARD.encode(database_password), is_secret:false},
                },
            }
        }
    }
}

pub fn db_disk_type(provider_kind: Kind, database_mode: DatabaseMode) -> String {
    match provider_kind {
        Kind::Aws => match database_mode {
            DatabaseMode::MANAGED => AwsStorageType::GP2.to_cloud_provider_format().to_string(),
            DatabaseMode::CONTAINER => AwsStorageType::GP2.to_k8s_storage_class(),
        },
        Kind::Azure => match database_mode {
            DatabaseMode::MANAGED => todo!(),
            DatabaseMode::CONTAINER => AZURE_SELF_HOSTED_DATABASE_DISK_TYPE.to_k8s_storage_class(),
        },
        Kind::Scw => match database_mode {
            DatabaseMode::MANAGED => SCW_MANAGED_DATABASE_DISK_TYPE,
            DatabaseMode::CONTAINER => SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
        }
        .to_string(),
        Kind::Gcp => match database_mode {
            DatabaseMode::MANAGED => todo!(),
            DatabaseMode::CONTAINER => GCP_SELF_HOSTED_DATABASE_DISK_TYPE.to_k8s_storage_class(),
        },
        Kind::OnPremise => todo!(),
    }
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
            DatabaseMode::MANAGED => Some(Box::new(SCW_MANAGED_DATABASE_INSTANCE_TYPE)),
            DatabaseMode::CONTAINER => None,
        },
        Kind::Azure => None, // TODO: once managed DB is implemented
        Kind::Gcp => None,   // TODO: once managed DB is implemented
        Kind::OnPremise => todo!(),
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

pub enum TcpCheckErrors {
    DomainNotResolvable,
    PortNotOpen,
    UnknownError,
}

pub enum TcpCheckSource<'a> {
    SocketAddr(SocketAddr),
    DnsName(&'a str),
}

pub fn check_tcp_port_is_open(address: &TcpCheckSource, port: u16) -> Result<(), TcpCheckErrors> {
    let timeout = Duration::from_secs(1);

    let ip = match address {
        TcpCheckSource::SocketAddr(x) => *x,
        TcpCheckSource::DnsName(x) => {
            let address = format!("{x}:{port}");
            match address.to_socket_addrs() {
                Ok(x) => {
                    let ips: Vec<SocketAddr> = x.collect();
                    ips[0]
                }
                Err(_) => return Err(TcpCheckErrors::DomainNotResolvable),
            }
        }
    };

    match std::net::TcpStream::connect_timeout(&ip, timeout) {
        Ok(_) => Ok(()),
        Err(_) => Err(TcpCheckErrors::PortNotOpen),
    }
}

pub fn check_udp_port_is_open(address: &str, port: u16) -> io::Result<bool> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let full_address = format!("{}:{}", address, port);
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;

    socket.send_to(b"qovery", full_address)?;

    // Attempt to receive a response
    let mut buf = [0; 512];
    match socket.recv_from(&mut buf) {
        Ok(_) => Ok(true),                                            // A response was received, port is open
        Err(ref e) if e.kind() == ErrorKind::WouldBlock => Ok(false), // Timeout, port is closed
        Err(e) => Err(e),                                             // An actual error occurred
    }
}
