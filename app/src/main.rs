#![allow(clippy::too_many_arguments)]

#[macro_use]
extern crate tracing;
extern crate core;

use std::fs::File;
use std::io::{BufReader, Error};
use std::path::Path;

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::{io, process};

use dirs::home_dir;
use dotenv::dotenv;
use tracing::error;
use tracing_subscriber::{EnvFilter, fmt::time::UtcTime, prelude::*};
use url::Url;
use uuid::Uuid;

use qovery_engine::cmd::docker::Docker;
use qovery_engine::engine_task::Task;
use qovery_engine::engine_task::qovery_api::{EngineServiceType, FakeQoveryApi, StaticQoveryApi};
use qovery_engine::environment::task::EnvironmentTask;
use qovery_engine::infrastructure::task::InfrastructureTask;
use qovery_engine::io_models::engine_request::{EnvironmentEngineRequest, InfrastructureEngineRequest};
use qovery_engine::logger::{Logger, StdIoLogger};
use qovery_engine::metrics_registry::MetricsRegistry;

use crate::constants::ASCII_BANNER;
use crate::logger::composite_logger::CompositeLogger;

use crate::models::TaskSelector;
use crate::utils::{check_libs_directory, check_versions_from};
use qovery_engine::metrics_registry::StdMetricsRegistry;
use qovery_engine::msg_publisher::StdMsgPublisher;
use reqwest::header;
use rustls::crypto::CryptoProvider;
use serde::Deserialize;

mod constants;
mod custom_error;
mod logger;
mod models;
mod utils;

pub fn generate_id() -> u32 {
    Uuid::new_v4().as_fields().0
}

pub fn main() -> io::Result<()> {
    println!("{ASCII_BANNER}");

    CryptoProvider::install_default(rustls::crypto::aws_lc_rs::default_provider())
        .expect("Cannot install rustls crypto provider");

    // Load env variable from .env file
    dotenv().ok();

    // Init tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .fmt_fields(
            tracing_subscriber::fmt::format::debug_fn(|writer, field, value| write!(writer, "{field}: {value:?}"))
                .delimited(", "),
        )
        .with_ansi(true)
        .with_thread_names(true)
        .with_timer(UtcTime::rfc_3339())
        .init();

    let engine_tag_version = env::var("ENGINE_TAG_VERSION").unwrap_or_else(|_| "unknown".to_string());
    let engine_id = env::var("ID").unwrap_or_else(|_| generate_id().to_string());
    let version_file = env::var("BIN_VERSION_FILE").expect("BIN_VERSION_FILE is mandatory");
    let test_cluster_env_var = env::var("TEST_CLUSTER");
    let lib_root_dir = env::var("LIB_ROOT_DIR").unwrap_or_else(|_| "lib".to_string());
    let docker_host = env::var("DOCKER_HOST").map(|val| Url::parse(&val).unwrap()).ok();
    let workspace_root_dir =
        env::var("WORKSPACE_ROOT_DIR").unwrap_or_else(|_| home_dir().unwrap().to_string_lossy().into_owned());

    let logger: Box<dyn Logger> = Box::new(CompositeLogger::new(vec![Box::new(StdIoLogger::new())]));
    let metrics_registry = Box::new(StdMetricsRegistry::new(Box::new(StdMsgPublisher::new())));

    info!("engine id: {}", engine_id.as_str());
    info!("engine version : {}", engine_tag_version.as_str());
    info!(
        "running from current directory: {}",
        env::current_dir().unwrap().to_str().unwrap()
    );
    info!("lib root dir: {}/", lib_root_dir.as_str());
    info!("workspace root dir: {}", workspace_root_dir.as_str());

    match check_libs_directory(lib_root_dir.clone()) {
        Ok(_) => info!("Libs directory is not empty"),
        Err(e) => {
            error!("Error while initializing the Engine {}", e);
            process::exit(1);
        }
    }

    //checking if version file exist
    match Path::new(&version_file).exists() {
        true => info!("Version file is accessible"),
        _ => {
            error!("Error while initializing the Engine, version file is not accessible");
            process::exit(1);
        }
    }

    // check all binaries version from version file
    match check_versions_from(&version_file) {
        Ok(()) => info!("Binaries versions are checked"),
        Err(e) => {
            error!("Error while initializing the Engine {}", e);

            process::exit(1);
        }
    }

    // check test_cluster environment variable content
    let test_cluster = match test_cluster_env_var {
        Ok(s) if s == "true" => true,
        Ok(s) if s == "false" => false,
        Ok(_) => {
            error!("Error, unexpected TEST_CLUSTER environment variable content, only true or false are accepted");
            process::exit(1);
        }
        Err(_) => true,
    };

    let docker = Arc::new(Docker::new_with_local_builder(docker_host).expect("Can't init docker builder"));
    match env::var("DEPLOY_FROM_FILE_KIND") {
        Ok(value) => match value.as_str() {
            "infra" => using_json_path_parameter(
                logger,
                env::var("DEPLOY_FROM_FILE").expect("missing DEPLOY_FROM_FILE variable"),
                workspace_root_dir,
                lib_root_dir,
                test_cluster,
                TaskSelector::Infrastructure,
                docker,
                metrics_registry,
            ),
            "env" => using_json_path_parameter(
                logger,
                env::var("DEPLOY_FROM_FILE").expect("missing DEPLOY_FROM_FILE variable"),
                workspace_root_dir,
                lib_root_dir,
                test_cluster,
                TaskSelector::Environment,
                docker,
                metrics_registry,
            ),
            _ => {
                println!("Please set DEPLOY_FROM_FILE_KIND environment file to 'infra' or 'env'");
                process::exit(1);
            }
        },
        _ => {
            println!("Please set DEPLOY_FROM_FILE_KIND environment file to 'infra' or 'env'");
            process::exit(1);
        }
    }
}

// the engine can be launch using a json file given in parameter
pub fn using_json_path_parameter(
    logger: Box<dyn Logger>,
    deploy_from_file: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    test_cluster: bool,
    deployment_type: TaskSelector,
    docker: Arc<Docker>,
    metrics_registry: Box<dyn MetricsRegistry>,
) -> Result<(), Error> {
    // check if file json config file exist
    if !Path::new(&deploy_from_file).exists() {
        error!("{} : No such file or directory", deploy_from_file);
        process::exit(1);
    }
    info!("Using {} configuration file", deploy_from_file);

    let file = BufReader::new(File::open(deploy_from_file)?);

    let task: Box<dyn Task> = match deployment_type {
        TaskSelector::Environment => {
            let mut deserialized_req = serde_json::Deserializer::from_reader(file);
            let mut request: EnvironmentEngineRequest = serde_path_to_error::deserialize(&mut deserialized_req)
                .map_err(|err| {
                    error!("Impossible to parse json file: {}", err);
                    process::exit(1);
                })
                .unwrap();

            request.test_cluster = test_cluster;
            Box::new(EnvironmentTask::new(
                request,
                workspace_root_dir,
                lib_root_dir,
                docker,
                logger,
                metrics_registry,
                Box::new(FakeQoveryApi {}),
                None,
            ))
        }
        TaskSelector::Infrastructure => {
            let mut deserialized_request = serde_json::Deserializer::from_reader(file);
            let mut request: InfrastructureEngineRequest = serde_path_to_error::deserialize(&mut deserialized_request)
                .map_err(|err| {
                    error!("Impossible to parse json file: {}", err);
                    process::exit(1);
                })
                .unwrap();

            request.test_cluster = test_cluster;
            Box::new(InfrastructureTask::new(
                request,
                workspace_root_dir,
                lib_root_dir,
                docker,
                logger,
                metrics_registry,
                Box::new(StaticQoveryApi {
                    versions: get_qovery_app_version("api.qovery.com").unwrap(),
                }),
                None,
            ))
        }
    };

    task.run();
    Ok(())
}

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
