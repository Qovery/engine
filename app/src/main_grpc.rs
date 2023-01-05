#![allow(clippy::too_many_arguments, dead_code)]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate prometheus;
#[macro_use]
extern crate tracing;
extern crate core;

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::Path;

use std::convert::TryFrom;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use std::{env, thread};
use std::{fs, io, process};

use dirs::home_dir;
use dotenv::dotenv;
use futures_util::{pin_mut, stream, Stream, StreamExt};
use qovery_engine::events::io::EngineEvent as EngineEventIo;
use retry::delay::Fixed;
use retry::OperationResult;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::{JoinError, JoinHandle};
use tonic::Code;
use tracing::error;
use tracing_subscriber::{fmt::time::ChronoUtc, prelude::*, EnvFilter};
use url::Url;
use uuid::Uuid;
use warp::http::Uri;

use qovery_engine::cmd;
use qovery_engine::cmd::docker::Docker;
use qovery_engine::engine_task::environment_task::EnvironmentTask;
use qovery_engine::engine_task::infrastructure_task::InfrastructureTask;
use qovery_engine::engine_task::Task;
use qovery_engine::events::EngineEvent;
use qovery_engine::io_models::engine_request::{EnvironmentEngineRequest, InfrastructureEngineRequest};
use qovery_engine::logger::{Logger, StdIoLogger};
use utils::Mode;

use crate::constants::ASCII_BANNER;
use crate::custom_error::ErrorKind::BinVersion;
use crate::custom_error::{EngineInitError, ErrorKind};
use crate::grpc::engine::{
    engine_message_rx, engine_message_tx, DeploymentInfo, DeploymentRequest, DeploymentType, EngineMessageTx,
};
use crate::grpc::GrpcEngineClient;
use crate::logger::composite_logger::CompositeLogger;
use crate::metrics::METRICS_NB_RUNNING_TASKS;

use crate::models::TaskSelector;

mod constants;
mod custom_error;
mod grpc;
mod logger;
mod metrics;
mod models;
mod tokio_utils;
mod utils;

fn to_engine_task(
    msg: String,
    workspace_root_dir: &str,
    lib_root_dir: &str,
    docker_tcp_socket: &Option<Url>,
    docker: Docker,
    task_selector: &TaskSelector,
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
                Ok(Box::new(EnvironmentTask::new(
                    request,
                    workspace_root_dir.to_string(),
                    lib_root_dir.to_string(),
                    docker_tcp_socket.clone(),
                    docker,
                    logger,
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

pub fn check_libs_directory(path: String) -> Result<(), EngineInitError> {
    match fs::read_dir(path) {
        Ok(out) => {
            let is_empty = out.take(1).count() == 0;
            match is_empty {
                true => Err(EngineInitError::Regular(ErrorKind::LibsDirEmpty)),
                false => Ok(()),
            }
        }
        Err(_) => Err(EngineInitError::Regular(ErrorKind::LibsPathsMissing)),
    }
}

// check_versions_from will check (in file given in parameter) binaries versions
// will assert an error if used version installed is not not the same than written in file
fn check_versions_from(path: &str) -> Result<(), EngineInitError> {
    // please append this vector if you want to test more binaries
    let bin_to_check = ["terraform"];

    let lines: Vec<String> = read_lines(path)
        .map_err(|err| {
            error!("{}", err);
            EngineInitError::Regular(BinVersion)
        })?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|err| {
            error!("{}", err);
            EngineInitError::Regular(BinVersion)
        })?;

    // read line by line the version file
    for line in lines.iter() {
        // put in lowercase and split the BINARY_VERSION to BINARY
        let lowercase = line.to_lowercase();
        //TODO FIX Do not parse correctly binary names in bin_versions. It should split at = instead of _
        //Modify bin_version format and edit the parsing
        let binary_name = lowercase.split('_').next().unwrap_or("");

        // check if the binary need to be tested
        if bin_to_check.contains(&binary_name) {
            let result_cmd = cmd::command::run_version_command_for(binary_name);
            let version = lowercase.split('=').last().unwrap_or("").replace('"', "");

            if !result_cmd.contains(&version) {
                return Err(EngineInitError::Regular(BinVersion));
            }

            info!("{} is on right version {}", binary_name.to_string(), version);
        }
    }

    Ok(())
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(BufReader::new(file).lines())
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
    let organization = env::var("ORGANIZATION");
    let cloud_provider = env::var("CLOUD_PROVIDER");
    let deployment_type = env::var("DEPLOYMENT_TYPE");
    let version_file = env::var("BIN_VERSION_FILE").expect("BIN_VERSION_FILE is mandatory");
    let region = env::var("REGION");
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

    let mode = if let (Ok(org), Ok(cp), Ok(r)) = (organization, cloud_provider, region) {
        info!("starting in cloud mode");
        info!("organization: {}", org.as_str());
        info!("cloud provider: {}", cp.as_str());
        info!("region: {}", r.as_str());
        Mode::Cloud(org, cp, r)
    } else {
        info!("starting in local mode");
        Mode::Local
    };

    let task_selector = match mode {
        Mode::Local => {
            if deployment_type.map_or(false, |deployment_type| deployment_type == "ENVIRONMENT") {
                TaskSelector::Environment("environment")
            } else {
                TaskSelector::Infrastructure("infrastructure")
            }
        }
        Mode::Cloud(_, _, _) => TaskSelector::Environment("environment"),
    };

    let task = async move {
        info!("Connecting to GRPC server: {:?}", grpc_server);
        let engine_client = grpc::new_engine_client(grpc_server, &cluster_id, &cluster_jwt_token)
            .await
            .unwrap();
        let mut current_deployment = DeploymentHandle::new();

        loop {
            let _ = listen_for_new_deployments(
                logger.clone(),
                engine_client.clone(),
                &mut current_deployment,
                workspace_root_dir.clone(),
                lib_root_dir.clone(),
                docker_host.clone(),
                docker.clone(),
                task_selector,
            )
            .await;
        }
    };

    let handle = tokio_utils::launch_task(task);
    while !handle.is_finished() {
        thread::sleep(Duration::from_secs(1));
    }

    Ok(())
}

struct DeploymentHandle {
    deployment_info: DeploymentInfo,
    #[allow(clippy::type_complexity)]
    task: Option<(Arc<Box<dyn Task>>, JoinHandle<()>)>,
    waker: Option<Waker>,
    tx: UnboundedSender<EngineEvent>,
    rx: Option<Box<UnboundedReceiver<EngineEvent>>>,
    rx_old: Option<Box<UnboundedReceiver<EngineEvent>>>,
}

impl DeploymentHandle {
    pub fn new() -> Self {
        let (engine_tx, engine_rx) = mpsc::unbounded_channel::<EngineEvent>();
        METRICS_NB_RUNNING_TASKS.set(0);
        Self {
            deployment_info: Default::default(),
            task: None,
            waker: None,
            tx: engine_tx,
            rx: Some(Box::new(engine_rx)),
            rx_old: None,
        }
    }

    pub fn set_current_task(&mut self, task: Box<dyn Task>, deployment_info: DeploymentInfo) {
        let task = Arc::new(task);
        let task_handle = tokio_utils::launch_blocking_task({
            let task = task.clone();
            move || {
                task.run();
            }
        });

        self.task = Some((task, task_handle));
        self.deployment_info = deployment_info;
        self.rx_old = None; // to drop the old receiver
        METRICS_NB_RUNNING_TASKS.inc();
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    pub fn get_task(&self) -> Option<&dyn Task> {
        self.task.as_ref().map(|t| &**t.0)
    }

    pub fn remove_task(&mut self) {
        METRICS_NB_RUNNING_TASKS.dec();
        self.task = None;
        let (tx, rx) = mpsc::unbounded_channel::<EngineEvent>();
        self.tx = tx;
        self.rx_old = self.rx.take();
        self.rx = Some(Box::new(rx));
    }

    pub fn get_deployment_info(&self) -> Option<&DeploymentInfo> {
        if self.task.is_some() {
            Some(&self.deployment_info)
        } else {
            None
        }
    }

    pub fn is_task_terminated(&self) -> bool {
        if let Some(task) = &self.task {
            task.1.is_finished()
        } else {
            true
        }
    }

    // This one is tricky/hacky due to the unsafe.
    // The stream we give back must be 'static due to tonic/grpc requirements.
    // and we want to be able to resume on error (i.e: cnx loss) and keep messages even if the grpc stream fails
    // We can't move the receiver part(non clonable) into the stream because we will lose the ability to retrieve it and lose pending messages.
    // We can't use an Arc because receiver.recv() need a mutable reference, and we can't use a Mutex because it's not Send.
    // So we must give a static mutable reference to the stream to conserve ownership of the receiver part.
    // For that we leak memory and re-create directly it directly to still drop it at some point.
    // We must ensure the boxed receiver lives as long as the stream, which works in our case because we process only 1 deployment at a time.
    pub fn get_message_channel(
        &mut self,
    ) -> (
        UnboundedSender<EngineEvent>,
        impl Stream<Item = EngineMessageTx> + Send + 'static,
    ) {
        let engine_rx_static: &'static mut UnboundedReceiver<_> = Box::leak(self.rx.take().unwrap());
        let engine_rx = unsafe { Box::from_raw(engine_rx_static) };
        self.rx = Some(engine_rx);

        let msg_to_upstream = stream::unfold(engine_rx_static, |event_rx| async move {
            match event_rx.recv().await {
                Some(engine_event) => {
                    let event_io = EngineEventIo::from(engine_event);
                    let grpc_message = EngineMessageTx {
                        message: Some(engine_message_tx::Message::Log(
                            serde_json::to_string(&event_io).unwrap_or_default(),
                        )),
                    };
                    Some((grpc_message, event_rx))
                }
                None => None,
            }
        });
        (self.tx.clone(), msg_to_upstream)
    }
}

impl Future for DeploymentHandle {
    type Output = Result<(), JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        return match this.task.as_mut() {
            Some(handle) => Pin::new(&mut handle.1).poll(cx),
            None => {
                this.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        };
    }
}

// the engine can be autonomous using the nats server to receive actions
async fn listen_for_new_deployments(
    logger: Box<dyn Logger>,
    mut engine_client: GrpcEngineClient,
    mut current_deployment: &mut DeploymentHandle,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<Url>,
    docker: Docker,
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
    let deployment_info = if let Some(deployment_info) = current_deployment.get_deployment_info() {
        info!("Resuming deployment for: {:?}", deployment_info);
        deployment_info.clone()
    } else {
        loop {
            match engine_client.get_new_deployment(deployment_type.clone()).await {
                Ok(deployment_info) => {
                    info!("Got new deployment for: {:?}", deployment_info);
                    break deployment_info.into_inner();
                }
                Err(err) => {
                    if err.code() == Code::NotFound {
                        info!("No deployment found, waiting for a new one");
                    } else {
                        error!("Error while getting new deployment: {}", err);
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    };

    // Now we retrieved a deployment, claim it and execute it
    let (engine_tx, msg_stream) = current_deployment.get_message_channel();
    let logger_for_task = CompositeLogger::new(vec![logger.clone(), Box::new(engine_tx)]);

    let msg_stream = stream::iter(vec![EngineMessageTx {
        message: Some(engine_message_tx::Message::DeploymentRequest(deployment_info.clone())),
    }])
    .chain(msg_stream);

    let msg_stream = match engine_client.exec_deployment(msg_stream).await {
        Ok(upstream_msg) => upstream_msg.into_inner(),
        Err(err) => {
            if err.code() != Code::NotFound {
                error!("Error while getting new deployment: {}", err);
            }
            // Task is terminated, and the server refused to accept message for it, we can remove it
            if current_deployment.is_task_terminated() {
                current_deployment.remove_task();
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
            return Err(err.into());
        }
    };

    pin_mut!(msg_stream);
    loop {
        tokio::select! {
            biased;

            _ = &mut current_deployment => {
                // Current deployment finished
                info!("Deployment terminated for: {:?}", deployment_info);
                // current_deployment.remove_task();
                // break;
                continue;
            }

            msg = msg_stream.next() => match msg {
                None => {
                    info!("Upstream stream closed");
                    break;
                }
                Some(Ok(msg)) => {
                    match msg.request {
                        Some(engine_message_rx::Request::DeploymentRequest(payload)) => {
                            info!("Received new deployment request: {}", payload);
                            let task = to_engine_task(
                                payload,
                                &workspace_root_dir,
                                &lib_root_dir,
                                &docker_host,
                                docker.clone(),
                                &task_selector,
                                logger_for_task.clone_dyn(),
                            )
                            .unwrap();

                            current_deployment.set_current_task(task, deployment_info.clone());
                        }
                        Some(engine_message_rx::Request::DeploymentCancel(_)) => {
                            if let Some(task) = current_deployment.get_task() {
                                let _ = task.cancel();
                            }
                        }
                        None => {
                            error!("Invalid payload received from grpc server. Update the protobuf !");
                        }
                    }
                    // Record last received message
                    current_deployment.deployment_info.last_message_id = msg.message_id;
                },
                Some(Err(e)) => {
                    error!("error while receiving message from grpc server: {}", e);
                    break;
                }
            }

        }
    }

    Ok(())
}
