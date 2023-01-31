#![allow(clippy::too_many_arguments)]

#[macro_use]
extern crate tracing;
extern crate core;

use std::fs::File;
use std::io::{BufReader, Error};
use std::path::Path;

use std::env;
use std::{io, process};

use dirs::home_dir;
use dotenv::dotenv;
use tracing::error;
use tracing_subscriber::{fmt::time::ChronoUtc, prelude::*, EnvFilter};
use url::Url;
use uuid::Uuid;

use qovery_engine::cmd::docker::Docker;
use qovery_engine::engine_task::core_service_api::FakeCoreServiceApi;
use qovery_engine::engine_task::environment_task::EnvironmentTask;
use qovery_engine::engine_task::infrastructure_task::InfrastructureTask;
use qovery_engine::engine_task::Task;
use qovery_engine::io_models::engine_request::{EnvironmentEngineRequest, InfrastructureEngineRequest};
use qovery_engine::logger::{Logger, StdIoLogger};

use crate::constants::ASCII_BANNER;
use crate::logger::composite_logger::CompositeLogger;

use crate::models::TaskSelector;
use crate::utils::{check_libs_directory, check_versions_from};

mod constants;
mod custom_error;
mod logger;
mod models;
mod utils;

pub fn generate_id() -> u32 {
    Uuid::new_v4().as_fields().0
}

pub fn main() -> io::Result<()> {
    println!("{}", ASCII_BANNER);

    // Load env variable from .env file
    dotenv().ok();

    // Init tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .fmt_fields(
            tracing_subscriber::fmt::format::debug_fn(|writer, field, value| write!(writer, "{}: {:?}", field, value))
                .delimited(", "),
        )
        .with_ansi(true)
        .with_timer(ChronoUtc::with_format("%Y-%m-%dT%H:%M:%SZ".to_string()))
        .init();

    let engine_id = env::var("ID").unwrap_or_else(|_| generate_id().to_string());
    let version_file = env::var("BIN_VERSION_FILE").expect("BIN_VERSION_FILE is mandatory");
    let test_cluster_env_var = env::var("TEST_CLUSTER");
    let lib_root_dir = env::var("LIB_ROOT_DIR").unwrap_or_else(|_| "lib".to_string());
    let docker_host = env::var("DOCKER_HOST").map(|val| Url::parse(&val).unwrap()).ok();
    let workspace_root_dir =
        env::var("WORKSPACE_ROOT_DIR").unwrap_or_else(|_| home_dir().unwrap().to_string_lossy().into_owned());

    let logger: Box<dyn Logger> = Box::new(CompositeLogger::new(vec![Box::new(StdIoLogger::new())]));

    info!("engine id: {}", engine_id.as_str());
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

    let docker = Docker::new(docker_host.clone()).expect("Can't init docker builder");
    match env::var("DEPLOY_FROM_FILE_KIND") {
        Ok(value) => match value.as_str() {
            "infra" => using_json_path_parameter(
                logger,
                env::var("DEPLOY_FROM_FILE").expect("missing DEPLOY_FROM_FILE variable"),
                workspace_root_dir,
                lib_root_dir,
                test_cluster,
                TaskSelector::Infrastructure(""),
                docker_host,
                docker,
            ),
            "env" => using_json_path_parameter(
                logger,
                env::var("DEPLOY_FROM_FILE").expect("missing DEPLOY_FROM_FILE variable"),
                workspace_root_dir,
                lib_root_dir,
                test_cluster,
                TaskSelector::Environment(""),
                docker_host,
                docker,
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
    docker_host: Option<Url>,
    docker: Docker,
) -> Result<(), Error> {
    // check if file json config file exist
    if !Path::new(&deploy_from_file).exists() {
        error!("{} : No such file or directory", deploy_from_file);
        process::exit(1);
    }
    info!("Using {} configuration file", deploy_from_file);

    let file = BufReader::new(File::open(deploy_from_file)?);

    let task: Box<dyn Task> = match deployment_type {
        TaskSelector::Environment(_) => {
            let mut req: EnvironmentEngineRequest = serde_json::from_reader(file)
                .map_err(|err| {
                    error!("Impossible to parse json file: {}", err);
                    process::exit(1);
                })
                .unwrap();
            req.test_cluster = test_cluster;
            Box::new(EnvironmentTask::new(
                req,
                workspace_root_dir,
                lib_root_dir,
                docker_host,
                docker,
                logger,
                Box::new(FakeCoreServiceApi {}),
            ))
        }
        TaskSelector::Infrastructure(_) => {
            let mut req: InfrastructureEngineRequest = serde_json::from_reader(file)
                .map_err(|err| {
                    error!("Impossible to parse json file: {}", err);
                    process::exit(1);
                })
                .unwrap();
            req.test_cluster = test_cluster;
            Box::new(InfrastructureTask::new(
                req,
                workspace_root_dir,
                lib_root_dir,
                docker_host,
                docker,
                logger,
            ))
        }
    };

    task.run();
    Ok(())
}
