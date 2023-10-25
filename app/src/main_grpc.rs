#![allow(clippy::too_many_arguments)]

#[macro_use]
extern crate prometheus;
#[macro_use]
extern crate tracing;
extern crate core;

use std::net::TcpStream;
use std::path::Path;

use chrono::Utc;
use clap::Parser;
use std::convert::TryFrom;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{env, thread};
use std::{io, process};

use dirs::home_dir;
use dotenv::dotenv;
use futures_util::future::select;
use futures_util::pin_mut;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::{DeleteParams, ListParams};
use kube::Api;
use qovery_engine::cmd::docker;
use retry::delay::Fixed;
use retry::OperationResult;
use tokio::signal::unix::SignalKind;
use tracing::error;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::{prelude::*, EnvFilter};
use url::Url;
use uuid::Uuid;
use warp::http::Uri;

use crate::constants::ASCII_BANNER;
use crate::deployment_manager::DeploymentManager;
use crate::grpc::engine::{DeploymentInfo, DeploymentType};
use crate::grpc::qovery_api::GrpcCoreServiceApi;
use crate::grpc::GrpcEngineClient;
use crate::models::TaskSelector;
use crate::utils::{check_libs_directory, check_versions_from};
use qovery_engine::cmd::docker::Docker;
use qovery_engine::engine_task::environment_task::EnvironmentTask;
use qovery_engine::engine_task::infrastructure_task::InfrastructureTask;
use qovery_engine::engine_task::Task;
use qovery_engine::errors::{CommandError, EngineError};
use qovery_engine::events::{
    EngineEvent, EnvironmentStep, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter,
};
use qovery_engine::io_models::engine_request::{EnvironmentEngineRequest, InfrastructureEngineRequest};
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;

mod constants;
mod custom_error;
mod deployment_manager;
mod engine_helper;
mod grpc;
mod logger;
mod metrics;
mod models;
mod tokio_utils;
mod utils;

pub type CloudProvider = String;
pub type Region = String;
pub type Organization = String;

#[derive(Clone, Debug)]
pub enum Mode {
    Local,
    Cloud(Organization, CloudProvider, Region),
}

fn to_engine_task(
    msg: String,
    workspace_root_dir: &str,
    lib_root_dir: &str,
    docker: Arc<Docker>,
    task_selector: &TaskSelector,
    grpc_client: &GrpcEngineClient,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
) -> Result<Arc<dyn Task>, serde_json::Error> {
    let mk_task = || -> Result<Arc<dyn Task>, serde_json::Error> {
        match task_selector {
            TaskSelector::Infrastructure(_) => {
                let request = serde_json::from_slice::<InfrastructureEngineRequest>(msg.as_bytes())?;
                let qovery_api = Box::new(GrpcCoreServiceApi::new(
                    request.deployment_jwt_token.clone(),
                    grpc_client.clone(),
                ));
                Ok(Arc::new(InfrastructureTask::new(
                    request,
                    workspace_root_dir.to_string(),
                    lib_root_dir.to_string(),
                    docker,
                    logger,
                    metrics_registry,
                    qovery_api,
                )))
            }
            TaskSelector::Environment(_) => {
                let request = serde_json::from_slice::<EnvironmentEngineRequest>(msg.as_bytes())?;
                let qovery_api = Box::new(GrpcCoreServiceApi::new(
                    request.deployment_jwt_token.clone(),
                    grpc_client.clone(),
                ));
                Ok(Arc::new(EnvironmentTask::new(
                    request,
                    workspace_root_dir.to_string(),
                    lib_root_dir.to_string(),
                    docker,
                    logger,
                    metrics_registry,
                    qovery_api,
                )))
            }
        }
    };

    match mk_task() {
        Ok(task) => Ok(task),
        Err(err) => {
            error!("{}", msg);
            error!("receiving request but JSON decoding error occurred: {:?}", err);
            Err(err)
        }
    }
}

/// Engine made by Qovery. Use grpc to connect to engine gateway and receive tasks to execute.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Name of this engine. Used to identify resources created by this engine (i.e: remote builder)
    #[arg(long, default_value = "qovery-engine", env = "ENGINE_NAME")]
    engine_name: String,

    /// If the engine should build docker images locally, or spawn builder in kube
    #[arg(long, default_value_t = false, env = "BUILDER_KUBE_ENABLED")]
    builder_kube_enabled: bool,

    /// Supported architectures for the image builder.
    #[arg(long, default_value = "AMD64", num_args = 1.., value_delimiter = ',', env = "BUILDER_CPU_ARCHITECTURES")]
    builder_cpu_architectures: Vec<docker::Architecture>,

    /// If kube builder enabled, in which namespace to create the builder
    #[arg(long, default_value = "qovery", env = "BUILDER_NAMESPACE")]
    builder_namespace: String,

    /// Listening address:port of the http server (used for healthcheck, metrics)
    #[arg(long, default_value = "[::]:8080", env = "HTTP_LISTEN_ON")]
    http_listen_on: String,

    /// Deployment type engine is going to execute. Can be "ENVIRONMENT" or "INFRASTRUCTURE"
    #[arg(long, default_value = "ENVIRONMENT", env = "DEPLOYMENT_TYPE")]
    deployment_type: String,

    /// Location of the binaries version file
    #[arg(long, env = "BIN_VERSION_FILE")]
    version_file: String,

    /// Path where to find the lib directory
    #[arg(long, default_value = "lib", env = "LIB_ROOT_DIR")]
    lib_root_dir: String,

    /// Cluster id (uuid) of the cluster where the engine is running
    #[arg(long, env = "CLUSTER_ID")]
    cluster_id: Uuid,

    /// Jwt token of the cluster, in order to authenticate engine to the engine gateway
    #[arg(long, env = "CLUSTER_JWT_TOKEN")]
    cluster_jwt_token: String,

    /// Url location of the engine grpc gateway
    #[arg(long, env = "GRPC_SERVER")]
    grpc_server: String,

    /// Url of the docker socket location
    #[arg(long, default_value = None, env = "DOCKER_HOST")]
    docker_host: Option<Url>,

    /// Workspace root directory path
    #[arg(long, default_value_t = home_dir().unwrap().to_string_lossy().into_owned(), env = "WORKSPACE_ROOT_DIR")]
    workspace_root_dir: String,
}

pub fn main() -> io::Result<()> {
    // Load env variable from .env file
    dotenv().ok();
    let mut cli: Cli = Cli::parse();
    cli.grpc_server = if !cli.grpc_server.starts_with("http") {
        format!("https://{}", cli.grpc_server)
    } else {
        cli.grpc_server
    };

    println!("{}", ASCII_BANNER);

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

    let grpc_server = Uri::try_from(&cli.grpc_server).expect("Invalid URI for GRPC_SERVER");

    let should_shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_callback = {
        let should_shutdown = should_shutdown.clone();

        async move {
            info!("WAITING for program to receive ctrl+c or sigterm");
            let ctrl_c = tokio::signal::ctrl_c();
            let mut sigterm_s = tokio::signal::unix::signal(SignalKind::terminate()).unwrap();
            let sigterm = sigterm_s.recv();

            pin_mut!(ctrl_c);
            pin_mut!(sigterm);
            let _ = select(ctrl_c, sigterm).await;
            warn!("STOPPING received ctrl+c/sigterm signal. We are going to wait for the current deployment to finish before shutting down");
            should_shutdown.store(true, Ordering::Relaxed);
        }
    };
    tokio_utils::launch_task(shutdown_callback);
    tokio_utils::launch_http_server(&cli.http_listen_on, should_shutdown.clone());

    info!(
        "running from current directory: {}",
        env::current_dir().unwrap().to_str().unwrap()
    );
    info!("lib root dir: {}/", cli.lib_root_dir.as_str());
    info!("workspace root dir: {}", cli.workspace_root_dir.as_str());

    match check_libs_directory(cli.lib_root_dir.clone()) {
        Ok(_) => info!("Libs directory is not empty"),
        Err(e) => {
            error!("Error while initializing the Engine {}", e);
            process::exit(1);
        }
    }

    //checking if version file exist
    match Path::new(&cli.version_file).exists() {
        true => info!("Version file is accessible"),
        _ => {
            error!("Error while initializing the Engine, version file is not accessible");
            process::exit(1);
        }
    }

    // check all binaries version from version file
    match check_versions_from(&cli.version_file) {
        Ok(()) => info!("Binaries versions are checked"),
        Err(e) => {
            error!("Error while initializing the Engine {}", e);

            process::exit(1);
        }
    }

    // ensure docker host is reachable to avoid error like: ERROR: Cannot connect to the Docker daemon at tcp://0.0.0.0:2375. Is the docker daemon running?
    // docker daemon is slower to start than the engine
    let disable_check_env_var = "IGNORE_DOCKER_HOST_CHECK";
    match &cli.docker_host {
        Some(docker_host) => {
            info!("docker host: {}", docker_host);
            let ignore_docker_host_check = match env::var(disable_check_env_var) {
                Ok(x) if x == *"true" => {
                    info!("ignoring docker host check");
                    true
                }
                _ => false,
            };

            if docker_host.scheme() == "tcp" && !ignore_docker_host_check {
                let docker_hostname = docker_host.host_str().unwrap_or("unkown_host");
                let docker_port = docker_host.port().unwrap_or(2375);
                let docker_address = format!("{docker_hostname}:{docker_port}");

                let result = retry::retry(Fixed::from(Duration::from_secs(2)).take(300), || {
                    match TcpStream::connect(docker_address.as_str()) {
                        Ok(_) => OperationResult::Ok(()),
                        Err(err) => {
                            info!("waiting for docker host to be reachable: {}", &err);
                            OperationResult::Retry(format!("docker host not yet reachable...{err}"))
                        }
                    }
                });

                match result {
                    Err(retry::Error { error, .. }) => {
                        error!(
                            "docker host is not reachable, disable the check with {} is you need: {}",
                            disable_check_env_var, error
                        );
                        process::exit(1)
                    }
                    Ok(_) => info!("docker host is reachable"),
                }
            }
        }
        None => info!("docker host is not set"),
    };

    let docker = if cli.builder_kube_enabled {
        let builder_prefix = "builder-".to_string();
        tokio_utils::launch_task(dead_builder_reaper(cli.builder_namespace.clone(), builder_prefix.clone()));
        Docker::new_with_kube_builder(
            cli.docker_host,
            &cli.builder_cpu_architectures,
            cli.builder_namespace.clone(),
            builder_prefix,
            vec![],
        )
        .expect("Can't init docker builder")
    } else {
        Docker::new_with_local_builder(cli.docker_host).expect("Can't init docker builder")
    };
    let docker = Arc::new(docker);

    let task_selector = if cli.deployment_type == "ENVIRONMENT" {
        TaskSelector::Environment("environment")
    } else {
        TaskSelector::Infrastructure("infrastructure")
    };

    let task_executor = async move {
        // Connect and check we are allowed to do request
        // If we are not allowed, we let the task die in order to be restarted
        info!("Connecting to GRPC server: {:?}", grpc_server);
        let mut engine_client = grpc::new_engine_client(grpc_server, &cli.cluster_id, &cli.cluster_jwt_token)
            .await
            .expect("Can't connect to engine gateway");
        engine_client
            .is_authorized(())
            .await
            .expect("Engine can't connect to gateway");

        let payload_to_engine_task = move |payload: String,
                                           deployment_info: &DeploymentInfo,
                                           grpc_client: &GrpcEngineClient,
                                           logger: Box<dyn Logger>,
                                           metrics_registry: Box<dyn MetricsRegistry>|
              -> Result<Arc<dyn Task>, EngineEvent> {
            let ret = to_engine_task(
                payload,
                &cli.workspace_root_dir,
                &cli.lib_root_dir,
                docker.clone(),
                &task_selector,
                grpc_client,
                logger,
                metrics_registry,
            );

            match ret {
                Ok(task) => Ok(task),
                Err(err) => {
                    let execution_id = deployment_info.execution_id.clone();
                    error!("Error while creating task for {}: {}", execution_id, err);
                    let event_details = EventDetails::new(
                        None,
                        QoveryIdentifier::new(Uuid::parse_str(&deployment_info.organization_id).unwrap_or_default()),
                        QoveryIdentifier::new(Uuid::parse_str(&deployment_info.cluster_id).unwrap_or_default()),
                        execution_id.to_string(),
                        if deployment_info.r#type == DeploymentType::Environment as i32 {
                            Stage::Environment(EnvironmentStep::Cancelled)
                        } else {
                            Stage::Infrastructure(InfrastructureStep::CannotProcessRequest)
                        },
                        Transmitter::TaskManager(Uuid::default(), String::from("task-manager")),
                    );
                    let msg =
                        format!("Engine received an invalid deployment request for execution_id = {execution_id}");
                    let message = EventMessage::new_from_safe(msg.to_string());
                    let err = EngineEvent::Error(
                        EngineError::new_invalid_engine_payload(
                            event_details.clone(),
                            msg.as_str(),
                            Some(CommandError::new(msg.clone(), Some(format!("{err}")), None)),
                        ),
                        Some(message),
                    );
                    Err(err)
                }
            }
        };

        let mngr =
            DeploymentManager::new(&task_selector, engine_client, should_shutdown, Box::new(payload_to_engine_task));
        mngr.run().await
    };

    let task_executor_h = tokio_utils::launch_task(task_executor);
    while !task_executor_h.is_finished() {
        thread::sleep(Duration::from_secs(1));
    }

    Ok(())
}

#[instrument]
async fn dead_builder_reaper(builder_namespace: String, builder_prefix: String) -> Result<(), kube::Error> {
    async fn run_reaper(
        deployments_api: &Api<Deployment>,
        max_allowed_lifetime: chrono::Duration,
        builder_name: &str,
    ) -> Result<(), kube::Error> {
        let deployments = deployments_api.list(&ListParams::default()).await?;
        let to_delete: Vec<String> = deployments
            .items
            .into_iter()
            .filter_map(|deployment| {
                let deployment_name = deployment.metadata.name.unwrap_or_default();

                if deployment_name.starts_with(builder_name)
                    && Utc::now() - deployment.metadata.creation_timestamp.unwrap_or(Time(Utc::now())).0
                        >= max_allowed_lifetime
                {
                    Some(deployment_name)
                } else {
                    None
                }
            })
            .collect();

        for deployment_name in to_delete {
            warn!("Deleting dead builder {}", deployment_name);
            let _ = deployments_api
                .delete(&deployment_name, &DeleteParams::background())
                .await;
        }

        Ok(())
    }

    let client = kube::Client::try_default().await?;
    let deployments_api: Api<Deployment> = Api::namespaced(client, &builder_namespace);
    let max_allowed_lifetime = chrono::Duration::hours(6);

    loop {
        info!("Running dead builder reaper for namespace: {builder_namespace} with max allowed lifetime of {max_allowed_lifetime}");
        if let Err(err) = run_reaper(&deployments_api, max_allowed_lifetime, &builder_prefix).await {
            error!("Error while reaping dead builders: {}", err);
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
