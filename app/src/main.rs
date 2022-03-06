#![allow(deprecated)]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate prometheus;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate tracing;

use std::fs::File;
use std::io::{BufRead, BufReader, Error};
use std::path::Path;
use std::sync::Arc;

use std::borrow::Borrow;
use std::time::Duration;
use std::{env, thread};
use std::{fs, io, process};

use crossbeam_channel::{unbounded, Sender};
use dirs::home_dir;
use dotenv::dotenv;
use tracing::error;
use tracing_subscriber::{fmt::time::ChronoUtc, prelude::*, EnvFilter};
use uuid::Uuid;

use qovery_engine::cmd;
use qovery_engine::logger::{Logger, StdIoLogger};
use utils::Mode;

use crate::constants::ASCII_BANNER;
use crate::custom_error::ErrorKind::BinVersion;
use crate::custom_error::{EngineInitError, ErrorKind};
use crate::logger::composite_logger::CompositeLogger;
use crate::logger::nats_logger::NatsLogger;

use crate::models::{StatusResponse, TaskSelector};
use crate::nats::{subjects, Connection, Message};
use crate::subjects::Subject;
use crate::task_manager::models::EngineRequest;
use crate::task_manager::task_manager::{Status, Task, TaskManager};
use crate::task_manager::tasks::{EnvironmentTask, InfrastructureTask};
use crate::utils::{log_no_spam_builder, LogErrorOnDrop};

mod constants;
mod custom_error;
mod logger;
mod models;
mod nats;
mod task_manager;
mod utils;
mod webserver;

fn to_engine_task(
    msg: Message,
    workspace_root_dir: &str,
    lib_root_dir: &str,
    docker_tcp_socket: &Option<String>,
    task_selector: &TaskSelector,
    status_sender: Sender<Status>,
) -> Result<Box<dyn Task>, serde_json::Error> {
    let request = match serde_json::from_slice::<EngineRequest>(&msg.data) {
        Ok(req) => req,
        Err(err) => {
            error!("{}", msg);
            error!("receiving request but JSON decoding error occurred: {:?}", err);
            return Err(err);
        }
    };

    let task: Box<dyn Task> = match task_selector {
        TaskSelector::Infrastructure(_) => Box::new(InfrastructureTask::new(
            request,
            status_sender,
            workspace_root_dir.to_string(),
            lib_root_dir.to_string(),
            docker_tcp_socket.clone(),
        )),
        TaskSelector::Environment(_) => Box::new(EnvironmentTask::new(
            request,
            status_sender,
            workspace_root_dir.to_string(),
            lib_root_dir.to_string(),
            docker_tcp_socket.clone(),
        )),
    };

    Ok(task)
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
            let result_cmd = cmd::command::run_version_command_for(&binary_name);
            let version = lowercase.split('=').last().unwrap_or("").replace('"', "");

            if !result_cmd.contains(&version) {
                return Err(EngineInitError::Regular(ErrorKind::BinVersion));
            }

            info!("{} is on right version {}", binary_name.to_string(), version);
        }
    }

    Ok(())
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

fn generate_id() -> u32 {
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
        .with_ansi(false)
        .with_timer(ChronoUtc::with_format("%Y-%m-%dT%H:%M:%SZ".to_string()))
        .init();

    let http_listen_on = env::var("HTTP_LISTEN_ON").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let engine_id = env::var("ID").unwrap_or_else(|_| generate_id().to_string());
    let organization = env::var("ORGANIZATION");
    let cloud_provider = env::var("CLOUD_PROVIDER");
    let deployment_type = env::var("DEPLOYMENT_TYPE");
    let version_file = env::var("BIN_VERSION_FILE").expect("BIN_VERSION_FILE is mandatory");
    let region = env::var("REGION");
    let nats_server = env::var("QOVERY_NATS_URL").expect("QOVERY_NATS_URL is mandatory");
    let nats_login = std::env::var("QOVERY_NATS_USER");
    let nats_password = std::env::var("QOVERY_NATS_PASSWORD");
    let test_cluster_env_var = std::env::var("TEST_CLUSTER");
    let lib_root_dir = env::var("LIB_ROOT_DIR").unwrap_or_else(|_| "lib".to_string());
    let docker_host = env::var("DOCKER_HOST").ok();
    let workspace_root_dir = env::var("WORKSPACE_ROOT_DIR")
        .unwrap_or(format!("{}/.qovery-workspace", home_dir().unwrap().to_str().unwrap()));

    let nats_credentials = match (nats_login, nats_password) {
        (Ok(nats_login), Ok(nats_password)) if !nats_login.is_empty() && !nats_password.is_empty() => {
            Some((nats_login, nats_password))
        }
        (_, _) => None,
    };

    let std_logger = StdIoLogger::new();
    let mut loggers: Vec<Box<dyn Logger>> = vec![Box::new(std_logger.clone())];
    if env::var("DEPLOY_FROM_FILE").is_err() {
        loggers.push(Box::new(NatsLogger::new(
            std_logger,
            Connection::new("engine_logs", nats_server.as_str(), nats_credentials.clone())
                .expect("cannot create NATS connection for engine logs"),
        )));
    };
    let logger = CompositeLogger::new(loggers);

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

    webserver::launch(http_listen_on.as_str());

    match docker_host {
        Some(ref docker_host) => info!("docker host: {}", docker_host),
        None => info!("docker host is not set"),
    };

    let mode = if organization.is_ok() && cloud_provider.is_ok() && region.is_ok() {
        let org = organization.unwrap();
        let cp = cloud_provider.unwrap();
        let r = region.unwrap();

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

    match env::var("DEPLOY_FROM_FILE") {
        Ok(deploy_from_file) => match env::var("DEPLOY_FROM_FILE_KIND") {
            Ok(value) => match value.as_str() {
                "infra" => using_json_path_parameter(
                    Box::new(logger),
                    deploy_from_file,
                    workspace_root_dir,
                    lib_root_dir,
                    test_cluster,
                    TaskSelector::Infrastructure(""),
                    docker_host,
                ),
                "env" => using_json_path_parameter(
                    Box::new(logger),
                    deploy_from_file,
                    workspace_root_dir,
                    lib_root_dir,
                    test_cluster,
                    TaskSelector::Environment(""),
                    docker_host,
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
        },
        _ => using_nats_server(
            Box::new(logger),
            nats_server,
            nats_credentials,
            workspace_root_dir,
            lib_root_dir,
            docker_host,
            mode,
            task_selector,
        ),
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
    docker_host: Option<String>,
) -> Result<(), Error> {
    // check if file json config file exist
    if !Path::new(&deploy_from_file).exists() {
        error!("{} : No such file or directory", deploy_from_file);
        process::exit(1);
    }
    info!("Using {} configuration file", deploy_from_file);

    let file = BufReader::new(File::open(deploy_from_file)?);
    let mut req: EngineRequest = serde_json::from_reader(file)
        .map_err(|err| {
            error!("Impossible to parse json file: {}", err);
            process::exit(1);
        })
        .unwrap();
    req.test_cluster = test_cluster;

    let mut task_manager = TaskManager::new();
    let task: Box<dyn Task> = match deployment_type {
        TaskSelector::Environment(_) => Box::new(EnvironmentTask::new(
            req.clone(),
            task_manager.get_task_status_tx().clone(),
            workspace_root_dir,
            lib_root_dir,
            docker_host,
        )),
        TaskSelector::Infrastructure(_) => Box::new(InfrastructureTask::new(
            req,
            task_manager.get_task_status_tx().clone(),
            workspace_root_dir,
            lib_root_dir,
            docker_host,
        )),
    };

    task_manager.add_task(task);
    let _ = task_manager.run(logger);

    loop {
        std::thread::park();
    }
}

// the engine can be autonomous using the nats server to receive actions
fn using_nats_server(
    logger: Box<dyn Logger>,
    nats_server: String,
    nats_credentials: Option<(String, String)>,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<String>,
    mode: Mode,
    task_selector: TaskSelector,
) -> Result<(), Error> {
    info!("NATS server: {}", nats_server.as_str());

    let engine_name = match &mode {
        Mode::Local => "qovery-engine-app.local".to_string(),
        Mode::Cloud(organization, cloud_provider, region) => {
            format!("qovery-engine-app.{}.{}.{}", organization, cloud_provider, region)
        }
    };

    info!("NATS client name: {}", engine_name.as_str());
    info!("connect to the NATS server...");
    let nc = Connection::new(engine_name.as_str(), nats_server.as_str(), nats_credentials)?;
    info!("connection to the NATS server established");

    let mut tm = TaskManager::new();
    let status_rx = tm.get_task_status_rx().clone();
    tm.run(logger).unwrap();
    let task_manager = Arc::new(tm);

    let _ = {
        let thread_name = "deployment-status-sender";
        let nc = nc.clone();
        let func = move || {
            let _drop_logger = LogErrorOnDrop::new(thread_name);
            loop {
                // send back the message to a topic: E.g core.task.status
                // json: {"status": {"kind": "Failed", "message": "blablabla"}, "id": "abc", "created_at": "<datetime>"}
                match status_rx.recv() {
                    Ok(status) => {
                        let sr = StatusResponse::new(status.context.execution_id.to_string(), status);
                        let json = serde_json::to_string(&sr).unwrap();
                        debug!("send through NATS StatusResponse: {}", json.as_str());
                        let _ = nc
                            .publish(&subjects::CORE_TASK_STATUS, json.as_bytes())
                            .map_err(|err| error!("Cannot publish on {}: {}", subjects::CORE_TASK_STATUS.name, err));
                    }
                    // Other end of the channel is disconnected
                    Err(_) => return,
                };
            }
        };

        thread::Builder::new()
            .name(thread_name.to_string())
            .spawn(func)
            .unwrap()
    };

    let (sig_term_tx, sig_term_rx) = unbounded::<bool>();
    {
        let nc = nc.clone();
        let thread_name = "sigterm-dispatcher".to_string();
        let task_manager = task_manager.clone();
        let _ = thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || {
                let _drop_logger = LogErrorOnDrop::new(thread_name.as_str());
                let _ = sig_term_rx
                    .recv()
                    .map_err(|err| error!("sigterm received with error {}", err));
                warn!("Termination signal received - graceful termination in progress...");

                let _ = nc
                    .drain()
                    .map_err(|err| error!("Cannot drain/unsubscribe {:?}: {}", nc, err));

                info!("Unsubscribed from all subjects");
                task_manager.stop();
                info!("Requested TaskManager to stop receiving new tasks");
            })
            .unwrap();
    }

    // Local engine, does not deploy anything (yet ?)
    if let Mode::Cloud(_, _, _) = mode {
        spawn_task_poller(
            task_manager.clone(),
            nc.clone(),
            task_selector,
            mode.clone(),
            workspace_root_dir.clone(),
            docker_host.clone(),
            lib_root_dir.clone(),
            engine_name.clone(),
            sig_term_tx.clone(),
        );
    }

    // Engine that run on cluster don't need to receive infrastructure requests
    if let Mode::Local = mode {
        spawn_task_poller(
            task_manager.clone(),
            nc.clone(),
            task_selector,
            mode.clone(),
            workspace_root_dir.clone(),
            docker_host.clone(),
            lib_root_dir.clone(),
            engine_name.clone(),
            sig_term_tx.clone(),
        );
    }

    ctrlc::set_handler(move || {
        let _ = sig_term_tx
            .send(true)
            .map_err(|err| error!("Cannot send sigterm signal {}", err));
    })
    .expect("Error setting Ctrl-C (SIGTERM) handler");

    info!("server started and listening for incoming requests");
    task_manager.wait_shutdown();

    warn!("end of execution");
    Ok(())
}

fn spawn_task_poller(
    task_manager: Arc<TaskManager>,
    nats: Connection,
    task_selector: TaskSelector,
    mode: Mode,
    workspace_root_dir: String,
    docker_host: Option<String>,
    lib_root_dir: String,
    engine_name: String,
    sig_term_tx: Sender<bool>,
) {
    let task_name = match task_selector {
        TaskSelector::Infrastructure(name) => name,
        TaskSelector::Environment(name) => name,
    };

    let thread_name = format!("{}-poller", task_name);
    let thread_name_logger = thread_name.clone();
    let subject = nats::subjects::Subject::new(&mode, &task_selector);

    let func = move || {
        let _drop_logger = LogErrorOnDrop::new(thread_name_logger.as_str());
        let mut nb_failure = 0;
        let mut log_request = log_no_spam_builder(format!("Requesting deployment task at {}", subject.name), 5);

        // We abort the engine if we don't manage to have answer from the core after
        // 30 * 10sec = 300 seconds == 5 min
        while nb_failure < 30 {
            // We ask to the Core if there is some tasks/deployment available to process
            log_request();
            let msg = match nats.request_timeout(&subject, engine_name.as_bytes(), Duration::from_secs(10)) {
                Err(err) => {
                    error!("Cannot retrieve deployment tasks from upstream: {}", err);
                    nb_failure += 1;
                    continue;
                }
                Ok(msg) => {
                    nb_failure = 0;
                    msg
                }
            };

            // If msg is null, there is no task available just sleep and retry
            if msg.data == "null".as_bytes() {
                thread::sleep(Duration::from_secs(5));
                continue;
            }

            info!(
                "{}",
                std::str::from_utf8(&msg.data).unwrap_or("Received an invalid utf8 msg from Nats")
            );

            // Convert our nats message into an engine task
            let engine_task = match to_engine_task(
                msg,
                &workspace_root_dir,
                &lib_root_dir,
                &docker_host,
                &task_selector,
                task_manager.get_task_status_tx().clone(),
            ) {
                Ok(task) => task,
                Err(err) => {
                    error!("Cannot converts Nats message payload to an engine task: {}", err);
                    continue;
                }
            };

            let task_id = engine_task.id().to_string();
            let task_cancel_subscription = match nats.subscribe(&Subject::new_for_task_cancel(engine_task.borrow())) {
                Ok(subscription) => {
                    info!("Subscribed on {:?} for task cancellation {}", subscription, &task_id);
                    subscription
                }
                Err(err) => {
                    error!("Cannot subscribe on nats cancellation subject: {}", err);
                    continue;
                }
            };

            // Ask the task manager to process our task
            task_manager.add_task(engine_task);

            // We wait for the task to finish as we don't want the engine to queue them
            loop {
                // Wait to receive a cancel
                if let Ok(_) = task_cancel_subscription.next_timeout(Duration::from_secs(10)) {
                    info!("Engine received cancel notification for task: {}", &task_id);
                    task_manager.cancel_current_task()
                }

                // If the task is finished to be run, go get a new one
                if task_manager.remaining_tasks_to_run() <= 0 {
                    break;
                }
            }
        }

        sig_term_tx.send(true)
    };

    thread::Builder::new()
        .name(thread_name.clone())
        .spawn(func)
        .expect(format!("Cannot spawn thread {}", thread_name).as_str());
}
