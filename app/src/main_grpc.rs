#![allow(clippy::too_many_arguments)]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate prometheus;
#[macro_use]
extern crate tracing;
extern crate core;

use std::net::TcpStream;
use std::path::Path;

use std::convert::TryFrom;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{env, thread};
use std::{io, process};

use dirs::home_dir;
use dotenv::dotenv;
use futures_util::future::select;
use futures_util::{pin_mut, stream, StreamExt};
use retry::delay::Fixed;
use retry::OperationResult;
use tokio::signal::unix::SignalKind;
use tonic::Code;
use tracing::error;
use tracing_subscriber::{fmt::time::ChronoUtc, prelude::*, EnvFilter};
use url::Url;
use uuid::Uuid;
use warp::http::Uri;

use qovery_engine::cmd::docker::Docker;
use qovery_engine::engine_task::environment_task::EnvironmentTask;
use qovery_engine::engine_task::infrastructure_task::InfrastructureTask;
use qovery_engine::engine_task::Task;
use qovery_engine::errors::EngineError;
use qovery_engine::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage, Stage, Transmitter};
use qovery_engine::io_models::engine_request::{EnvironmentEngineRequest, InfrastructureEngineRequest};
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::logger::{Logger, StdIoLogger};

use crate::constants::ASCII_BANNER;
use crate::deployment_manager::DeploymentManager;
use crate::grpc::engine::{
    engine_message_rx, engine_message_tx, DeploymentInfo, DeploymentRequest, DeploymentType, EngineMessageTx,
};
use crate::grpc::qovery_api::GrpcCoreServiceApi;
use crate::grpc::GrpcEngineClient;
use crate::logger::composite_logger::CompositeLogger;
use crate::models::TaskSelector;
use crate::utils::{check_libs_directory, check_versions_from};

mod constants;
mod custom_error;
mod deployment_manager;
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
    docker_tcp_socket: &Option<Url>,
    docker: Docker,
    task_selector: &TaskSelector,
    grpc_client: &GrpcEngineClient,
    logger: Box<dyn Logger>,
) -> Result<Box<dyn Task>, serde_json::Error> {
    let mk_task = || -> Result<Box<dyn Task>, serde_json::Error> {
        match task_selector {
            TaskSelector::Infrastructure(_) => {
                let request = serde_json::from_slice::<InfrastructureEngineRequest>(msg.as_bytes())?;
                Ok(Box::new(InfrastructureTask::new(
                    request,
                    workspace_root_dir.to_string(),
                    lib_root_dir.to_string(),
                    docker_tcp_socket.clone(),
                    docker,
                    logger,
                )))
            }
            TaskSelector::Environment(_) => {
                let request = serde_json::from_slice::<EnvironmentEngineRequest>(msg.as_bytes())?;
                let qovery_api = Box::new(GrpcCoreServiceApi::new(
                    request.deployment_jwt_token.clone(),
                    grpc_client.clone(),
                ));
                Ok(Box::new(EnvironmentTask::new(
                    request,
                    workspace_root_dir.to_string(),
                    lib_root_dir.to_string(),
                    docker_tcp_socket.clone(),
                    docker,
                    logger,
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
        .with_ansi(false)
        .with_timer(ChronoUtc::with_format("%Y-%m-%dT%H:%M:%SZ".to_string()))
        .init();

    let http_listen_on = env::var("HTTP_LISTEN_ON").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let deployment_type = env::var("DEPLOYMENT_TYPE");
    let version_file = env::var("BIN_VERSION_FILE").expect("BIN_VERSION_FILE is mandatory");
    let cluster_id = Uuid::parse_str(&std::env::var("CLUSTER_ID").expect("Missing Env Var for CLUSTER_ID"))
        .expect("CLUSTER_ID is an invalid uuidV4");
    let cluster_jwt_token = std::env::var("CLUSTER_JWT_TOKEN").expect("Missing Env Var for CLUSTER_JWT_TOKEN");
    let lib_root_dir = env::var("LIB_ROOT_DIR").unwrap_or_else(|_| "lib".to_string());
    let docker_host = env::var("DOCKER_HOST").map(|val| Url::parse(&val).unwrap()).ok();
    let workspace_root_dir =
        env::var("WORKSPACE_ROOT_DIR").unwrap_or_else(|_| home_dir().unwrap().to_string_lossy().into_owned());
    let grpc_server = std::env::var("GRPC_SERVER").expect("Missing Env Var for GRPC_SERVER");
    let grpc_server = if !grpc_server.starts_with("http") {
        format!("https://{}", grpc_server)
    } else {
        grpc_server
    };
    let grpc_server = Uri::try_from(&grpc_server).expect("Invalid URI for GRPC_SERVER");
    let logger = Box::new(StdIoLogger::new());

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

    tokio_utils::launch(http_listen_on.as_str());

    // ensure docker host is reachable to avoid error like: ERROR: Cannot connect to the Docker daemon at tcp://0.0.0.0:2375. Is the docker daemon running?
    // docker daemon is slower to start than the engine
    let disable_check_env_var = "IGNORE_DOCKER_HOST_CHECK";
    match docker_host {
        Some(ref docker_host) => {
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
                let docker_address = format!("{}:{}", docker_hostname, docker_port);

                let result = retry::retry(Fixed::from(Duration::from_secs(2)).take(300), || {
                    match TcpStream::connect(docker_address.as_str()) {
                        Ok(_) => OperationResult::Ok(()),
                        Err(err) => {
                            info!("waiting for docker host to be reachable: {}", &err);
                            OperationResult::Retry(format!("docker host not yet reachable...{}", err))
                        }
                    }
                });

                match result {
                    Err(err) => match err {
                        retry::Error::Operation {
                            error: e,
                            total_delay: _,
                            tries: _,
                        } => {
                            error!(
                                "docker host is not reachable, disable the check with {} is you need: {}",
                                disable_check_env_var, e
                            );
                            process::exit(1)
                        }
                        retry::Error::Internal(err) => {
                            error!("internal error while checking if docker host is not reachable, disable the check with {} is you need: {}", disable_check_env_var, err);
                            process::exit(1)
                        }
                    },
                    Ok(_) => info!("docker host is reachable"),
                }
            }
        }
        None => info!("docker host is not set"),
    };
    let docker = Docker::new(docker_host.clone()).expect("Can't init docker builder");
    let task_selector = if deployment_type.map_or(true, |deployment_type| deployment_type == "ENVIRONMENT") {
        TaskSelector::Environment("environment")
    } else {
        TaskSelector::Infrastructure("infrastructure")
    };

    let should_shutdown = Arc::new(AtomicBool::new(false));
    let task_executor = {
        let should_shutdown = should_shutdown.clone();

        async move {
            // Connect and check we are allowed to do request
            // If we are not allowed, we let the task die in order to be restarted
            info!("Connecting to GRPC server: {:?}", grpc_server);
            let mut engine_client = grpc::new_engine_client(grpc_server, &cluster_id, &cluster_jwt_token)
                .await
                .expect("Can't connect to engine gateway");
            engine_client
                .is_authorized(())
                .await
                .expect("Engine can't connect to gateway");

            let mut current_deployment = DeploymentManager::new();
            let payload_to_engine_task = |payload: String,
                                          grpc_client: &GrpcEngineClient,
                                          logger: Box<dyn Logger>|
             -> Result<Box<dyn Task>, serde_json::Error> {
                to_engine_task(
                    payload,
                    &workspace_root_dir,
                    &lib_root_dir,
                    &docker_host,
                    docker.clone(),
                    &task_selector,
                    grpc_client,
                    logger,
                )
            };

            // Execute deployment until we are asked to be shutdown and no deployment is on-going
            while !(should_shutdown.load(Ordering::Relaxed) && current_deployment.get_current_deployment().is_none()) {
                let ret = fetch_and_exec_deployments(
                    &mut engine_client,
                    &mut current_deployment,
                    payload_to_engine_task,
                    logger.clone(),
                    task_selector,
                )
                .await;

                if let Err(e) = ret {
                    error!("{}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    };

    let shutdown_callback = async move {
        info!("WAITING for program to receive ctrl+c or sigterm");
        let ctrl_c = tokio::signal::ctrl_c();
        let mut sigterm_s = tokio::signal::unix::signal(SignalKind::terminate()).unwrap();
        let sigterm = sigterm_s.recv();

        pin_mut!(ctrl_c);
        pin_mut!(sigterm);
        let _ = select(ctrl_c, sigterm).await;
        info!("STOPPING received ctrl+c/sigterm signal. We are going to wait for the current deployment to finish before shutting down");
        should_shutdown.store(true, Ordering::Relaxed);
    };

    let _ = tokio_utils::launch_task(shutdown_callback);
    let task_executor_h = tokio_utils::launch_task(task_executor);
    while !task_executor_h.is_finished() {
        thread::sleep(Duration::from_secs(1));
    }

    Ok(())
}

async fn fetch_new_deployment(
    engine_client: &mut GrpcEngineClient,
    deployment_request: DeploymentRequest,
) -> Result<DeploymentInfo, anyhow::Error> {
    return match engine_client.get_new_deployment(deployment_request.clone()).await {
        Ok(deployment_info) => {
            let deployment_info = deployment_info.into_inner();
            Ok(deployment_info)
        }
        Err(err) => {
            if err.code() == Code::NotFound {
                Err(anyhow::anyhow!("No deployment found, waiting for a new one"))
            } else {
                Err(anyhow::anyhow!("Error while getting new deployment: {}", err))
            }
        }
    };
}

async fn fetch_and_exec_deployments(
    engine_client: &mut GrpcEngineClient,
    mut current_deployment: &mut DeploymentManager,
    to_engine_task: impl Fn(String, &GrpcEngineClient, Box<dyn Logger>) -> Result<Box<dyn Task>, serde_json::Error>,
    logger: Box<dyn Logger>,
    task_selector: TaskSelector,
) -> Result<(), anyhow::Error> {
    let deployment_type = match task_selector {
        TaskSelector::Infrastructure(_) => DeploymentRequest {
            deployment_type: DeploymentType::Infrastructure as i32,
        },
        TaskSelector::Environment(_) => DeploymentRequest {
            deployment_type: DeploymentType::Environment as i32,
        },
    };

    // If there is no deployment on-going, we loop until we retrieve a new deployment to execute
    // if there is already one deployment it means, the connection broke, and we try to resume the current one
    let deployment_info = if let Some(deployment_info) = current_deployment.get_current_deployment() {
        info!("Resuming deployment for: {:?}", deployment_info);
        deployment_info.clone()
    } else {
        let deployment_info = fetch_new_deployment(engine_client, deployment_type).await?;
        info!("Got new deployment for: {:?}", deployment_info);
        current_deployment.set_current_deployment(deployment_info.clone());
        deployment_info
    };

    // Now we retrieved a deployment, claim it and execute it
    let (engine_tx, msg_stream, mut abort_deployment_tx) = current_deployment.get_message_stream().await;
    let logger_for_task = CompositeLogger::new(vec![logger.clone(), Box::new(engine_tx.clone())]);

    let msg_stream = stream::iter(vec![EngineMessageTx {
        message_id: None,
        message: Some(engine_message_tx::Message::DeploymentRequest(deployment_info.clone())),
    }])
    .chain(msg_stream);

    let msg_stream = match engine_client.exec_deployment(msg_stream).await {
        Ok(upstream_msg) => upstream_msg.into_inner(),
        Err(err) => {
            return match err.code() {
                Code::NotFound => {
                    // Task is terminated, and the server refused to accept message for it, we can remove it
                    while !current_deployment.is_task_terminated() {
                        info!("Current deployment does not exist anymore, but the task is not terminated, waiting for it to finish");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    info!("Task terminated and current deployment does not exist anymore, removing it");
                    current_deployment.remove_current_deployment();

                    Ok(())
                }
                _ => {
                    error!("Error while getting new deployment: {}", err);
                    Err(err.into())
                }
            };
        }
    };
    info!("Connected to gateway, executing deployment task for: {:?}", deployment_info);

    pin_mut!(msg_stream);
    loop {
        tokio::select! {
            biased;

            // If there is no task on-going for this deployment, we wait at max 15sec to receive a new one
            // Before we check if the deployment is still valid
            _ = tokio::time::sleep(Duration::from_secs(15)), if current_deployment.is_task_terminated() => {
                info!("No new message after 15s, assuming deployment is terminated");
                break;
            }

            // We wait for the current executing task to finish
            // We don't put a if to avoid a race condition, if the task is terminated the future is going to never return
            _ = &mut current_deployment => {
                info!("Deployment task terminated");
                current_deployment.remove_task();
                continue;
            }

            // We lost the connection with gateway to forward engine message, trying to reconnect
            _ = abort_deployment_tx.closed() => {
                info!("EngineEvent forwarder to gateway has been close, trying to resume connection");
                break;
            }

            // We wait to receive a new message from the gateway
            // In case of error, we return to try to resume the current deployment.
            // The server will let us know if the deployment is still valid
            msg = msg_stream.next() => match msg {
                Some(Ok(msg)) => {
                    match msg.request {
                        Some(engine_message_rx::Request::DeploymentRequest(payload)) => {
                            info!("Received new deployment task: {}", payload);
                            let task = to_engine_task(
                                payload,
                                engine_client,
                                logger_for_task.clone_dyn(),
                            );

                            match task {
                                Ok(task) => {
                                    current_deployment.set_task(task);
                                }
                                Err(err) => {
                                    error!("Error while creating task: {}", err);
                                    let event_details = EventDetails::new(None,
                                        QoveryIdentifier::new(Uuid::parse_str(&deployment_info.organization_id).unwrap_or_default()),
                                        QoveryIdentifier::new(Uuid::parse_str(&deployment_info.cluster_id).unwrap_or_default()),
                                        deployment_info.execution_id.clone(),
                                        Stage::Environment(EnvironmentStep::Cancelled),
                                        Transmitter::TaskManager(Uuid::default(), String::from("task-manager")),
                                    );
                                    let message = EventMessage::new_from_safe("Engine received an invalid deployment request".to_string());
                                    let err = EngineEvent::Error(EngineError::new_task_cancellation_requested(event_details), Some(message));
                                    let _ = engine_tx.send(err);
                                }
                            }
                        }
                        Some(engine_message_rx::Request::DeploymentCancel(_)) => {
                            info!("Received cancel request: {:?}", msg);
                            if let Some(task) = current_deployment.get_task() {
                                let _ = task.cancel();
                            }
                        }
                        Some(engine_message_rx::Request::Terminated(_)) => {
                            info!("Received terminated message for deployment: {:?}", msg);
                            current_deployment.remove_current_deployment();
                            break;
                        }
                        None => {
                            error!("Invalid payload received from grpc server. Update the protobuf !");
                        }
                    }
                    // We record the last message we received, so in case of cnx loss
                    // we can resume the deployment and restart from the last message
                    current_deployment.set_last_message_id(msg.message_id);
                },

                // Return to try to resume the current deployment
                None => {
                    info!("Upstream stream closed");
                    break;
                }

                // Return to try to resume the current deployment
                Some(Err(e)) => {
                    error!("error while receiving message from grpc server: {}", e);
                    break;
                }
            }
        }
    }

    Ok(())
}
