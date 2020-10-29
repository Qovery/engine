#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;

use std::borrow::Borrow;
use std::fs::File;
use std::io::{BufRead, BufReader, Error, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::sleep;
use std::time::Duration;
use std::{env, thread};
use std::{fs, io, process};

use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Sender};
use dirs::home_dir;
use nats::tls::{Identity, TlsConnector, TlsConnectorBuilder};
use nats::{Connection, Subscription};
use retry::delay::Fibonacci;
use retry::OperationResult;
use serde::{Deserialize, Serialize};

use qovery_engine::cmd;
use qovery_engine::models::Context;
use qovery_engine_shared::{subject, Mode};
use qovery_engine_task_manager::models::{
    CheckTaskOrderRequest, CheckTaskOrderResponse, CheckTaskRunningResponse, Request, Response,
};
use qovery_engine_task_manager::task_manager::{
    InternalTask, PreRun, State, Status, Task, TaskManager,
};
use qovery_engine_task_manager::tasks::{EnvironmentTask, InfrastructureTask};

use crate::constants::ASCII_BANNER;
use crate::custom_error::{EngineInitError, ErrorKind};
use crate::TaskSelector::{Environment, Infrastructure};

mod constants;
mod custom_error;

const CORE_TASK_STATUS_SUBJECT: &str = "core.task.status";
const CORE_PING_SUBJECT: &str = "core.ping";
const ENGINE_TASK_RUNNING_CHECK_SUBJECT: &str = "engine.task_running_check";
const ENGINE_TASK_ORDER_EXECUTION_CHECK_SUBJECT: &str = "engine.task_order_execution_check";

enum TaskSelector {
    Infrastructure(&'static str),
    Environment(&'static str),
}

#[derive(Serialize, Deserialize)]
struct StatusResponse {
    id: String,
    created_at: DateTime<Utc>,
    status: Status,
}

impl StatusResponse {
    fn new(id: String, status: Status) -> Self {
        StatusResponse {
            id,
            created_at: Utc::now(),
            status,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Ping {
    created_at: DateTime<Utc>,
    engine_started_at: DateTime<Utc>,
    engine_name: String,
    // TODO add stats? deployments: { total, total_successes, total_failed ...}
}

impl Ping {
    fn new(engine_started_at: DateTime<Utc>, engine_name: &str) -> Self {
        Ping {
            created_at: Utc::now(),
            engine_started_at,
            engine_name: engine_name.to_string(),
        }
    }
}

fn subject_name(mode: &Mode, task_selector: &TaskSelector) -> String {
    subject(
        mode,
        match task_selector {
            TaskSelector::Infrastructure(s) => s,
            TaskSelector::Environment(s) => s,
        },
    )
}

/// check that the same task is not running on another instance of Q-engine.
/// we use the task.group_id() to determine if it is the case
fn is_the_same_task_running(task: &dyn Task, nc: Connection, mode: Mode) -> PreRun {
    let subject_name = subject(&mode, ENGINE_TASK_RUNNING_CHECK_SUBJECT);
    let sub = match nc.request_multi(subject_name.as_str(), task.group_id()) {
        Ok(sub) => sub,
        Err(_) => {
            error!(
                "can't check that the task '{}' with group id '{}' is running or not,\
             then act like it was already running to prevent critical outage",
                task.id(),
                task.group_id()
            );
            return PreRun::NoAndQueueTail;
        }
    };

    for msg in sub.next() {
        let is_task_running =
            match serde_json::from_slice::<CheckTaskRunningResponse>(msg.data.as_slice()) {
                Ok(check_task) => check_task.is_running,
                Err(err) => panic!(err),
            };

        msg.respond(Response::new(None).as_json_string());

        if is_task_running {
            warn!(
                "task with group id {} and id {} is already running",
                task.group_id(),
                task.id()
            );
            return PreRun::NoAndQueueTail;
        }
    }

    info!(
        "task with group id {} and id {} is not running",
        task.group_id(),
        task.id()
    );

    PreRun::Yes
}

fn listen_for_task_running_check_events(
    task_manager: Arc<Mutex<TaskManager>>,
    nc: &Connection,
    mode: &Mode,
) -> Result<Subscription, Error> {
    let subject_name = subject(&mode, ENGINE_TASK_RUNNING_CHECK_SUBJECT);
    let sub = nc.subscribe(subject_name.as_str())?;
    info!("subscribe to {}", subject_name.as_str());

    sub.clone().with_handler(move |msg| {
        let group_id = String::from_utf8(msg.data.clone()).unwrap();

        let is_running = match task_manager.lock() {
            Ok(tm) => match tm.get_task_status_by_group_id(&group_id) {
                Some(status) => match status.status {
                    State::DeploymentInProgress
                    | State::PauseInProgress
                    | State::DeleteInProgress => true,
                    _ => false,
                },
                None => false,
            },
            Err(_) => {
                // if there is a lock error, we prefer to delay the deployment to not take any risk
                true
            }
        };

        let _ = msg.respond(CheckTaskRunningResponse::new(is_running).as_json_string());

        Ok(())
    });

    Ok(sub)
}

/// check if the current task is the next one to run
/// ask to other Engine if they have a task that must be launched sooner
/// this is a way to lazily order tasks  
fn is_the_next_task_to_run(task: &dyn Task, nc: Connection, mode: Mode) -> PreRun {
    let subject_name = subject(&mode, ENGINE_TASK_ORDER_EXECUTION_CHECK_SUBJECT);
    let request =
        CheckTaskOrderRequest::new(task.group_id().to_string(), task.created_at().clone());
    let sub = match nc.request_multi(subject_name.as_str(), request.as_json_string()) {
        Ok(sub) => sub,
        Err(_) => {
            error!(
                "can't check that the task '{}' with group id '{}' is the next task or not,\
             then act like it was not the next task to prevent critical outage",
                task.id(),
                task.group_id()
            );
            return PreRun::NoAndQueueTail;
        }
    };

    for msg in sub.next() {
        let is_first_place =
            match serde_json::from_slice::<CheckTaskOrderResponse>(msg.data.as_slice()) {
                Ok(check_task) => check_task.is_first_place,
                Err(err) => panic!(err),
            };

        msg.respond(Response::new(None).as_json_string());

        if !is_first_place {
            warn!(
                "task with group id {} and id {} is not at the first place",
                task.group_id(),
                task.id()
            );
            return PreRun::NoAndQueueTail;
        }
    }

    info!(
        "task with group id {} and id {} is at the first place",
        task.group_id(),
        task.id()
    );

    PreRun::Yes
}

fn listen_for_task_order_execution_check(
    task_manager: Arc<Mutex<TaskManager>>,
    nc: &Connection,
    mode: &Mode,
) -> Result<Subscription, Error> {
    let subject_name = subject(&mode, ENGINE_TASK_ORDER_EXECUTION_CHECK_SUBJECT);
    let sub = nc.subscribe(subject_name.as_str())?;
    info!("subscribe to {}", subject_name.as_str());

    sub.clone().with_handler(move |msg| {
        let req = match serde_json::from_slice::<CheckTaskOrderRequest>(msg.data.as_slice()) {
            Ok(req) => req,
            Err(err) => {
                error!("{:?}", err);
                return Ok(());
            }
        };

        let is_first_place = match task_manager.lock() {
            Ok(tm) => {
                // compare the date of the current task to other tasks with the same group id.
                // if the current task has lower date than other tasks with same group id, it means that it is good to be run 👍
                // if the current task has higher date than other tasks with same group id, it means that it is not good to be run 👎
                match tm.get_task_status_by_group_id(&req.group_id) {
                    Some(status) => &req.created_at < &status.context.task_created_at,
                    None => true,
                }
            }
            Err(_) => {
                // if there is a lock error, we prefer to delay the deployment to not take any risk
                false
            }
        };

        let _ = msg.respond(CheckTaskOrderResponse::new(is_first_place).as_json_string());

        Ok(())
    });

    Ok(sub)
}

fn listen_for_events(
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_tcp_socket: Option<String>,
    task_selector: TaskSelector,
    nc: &Connection,
    mode: &Mode,
    tx: Sender<Box<dyn Task>>,
) -> Result<Subscription, Error> {
    let subject_name = subject_name(mode, &task_selector);
    let sub = nc.queue_subscribe(subject_name.as_str(), subject_name.as_str())?;
    info!("subscribe to {}", subject_name.as_str());

    let nc = nc.clone();
    let sub_1 = sub.clone();
    let mode = mode.clone();
    let ts_str = match task_selector {
        Infrastructure(_) => "infrastructure",
        Environment(_) => "environment",
    };

    thread::Builder::new()
        .name(format!("nats-event-receiver-{}", ts_str))
        .spawn(move || {
            let nc = nc.clone();

            loop {
                for msg in sub.next() {
                    debug!("{}", msg);

                    let nc_1 = nc.clone();
                    let mode = mode.clone();

                    // call before to run the current task to check that the same task is not running on another engine app
                    // check is_the_same_task_running and is_the_next_task_to_run functions to know more about the details
                    let pre_run_callback = Box::new(move |task: &dyn Task| {
                        PreRun::add(
                            is_the_same_task_running(task, nc_1.clone(), mode.clone()),
                            is_the_next_task_to_run(task, nc_1.clone(), mode.clone()),
                        )
                    });

                    match serde_json::from_slice::<Request>(msg.data.as_slice()) {
                        Ok(req) => {
                            let context = Context::new(
                                req.id.as_str(),
                                workspace_root_dir.as_str(),
                                lib_root_dir.as_str(),
                                docker_tcp_socket.clone(),
                                req.metadata.clone(),
                            );

                            tx.send(match task_selector {
                                TaskSelector::Infrastructure(_) => Box::new(
                                    InfrastructureTask::new(context, req, pre_run_callback),
                                ),
                                TaskSelector::Environment(_) => {
                                    Box::new(EnvironmentTask::new(context, req, pre_run_callback))
                                }
                            });
                            msg.respond(Response::new(None).as_json_string());
                        }
                        Err(err) => {
                            error!("{}", msg);
                            error!(
                                "receiving request but JSON decoding error occurred: {:?}",
                                err
                            );
                            msg.respond(Response::new(Some(err.to_string())).as_json_string());
                        }
                    };
                }
            }
        });

    Ok(sub_1)
}

/// Notify the core server that this engine exists and is running
/// if the server does not respond - then retry 10 times (with fibonacci retry) -
/// if it does not respond after all attempts, then gracefully restart the service.
fn watchdog(name: String, nc: Connection, sig_term_tx: Sender<bool>) {
    thread::Builder::new().name("watchdog".to_string()).spawn(move || {
        let engine_started_at = Utc::now();

        loop {
            let ping_res = retry::retry(Fibonacci::from_millis(3000).take(10), || {
                let ping = Ping::new(engine_started_at, name.as_str());
                let json = serde_json::to_string(&ping);

                let res = nc.request_timeout(
                    CORE_PING_SUBJECT,
                    json.unwrap().as_bytes(),
                    Duration::from_secs(5),
                );

                match res {
                    Ok(_) => OperationResult::Ok(0),
                    _ => {
                        warn!("ping failed (subject: {}), let's retry to ping the core server in a few seconds", CORE_PING_SUBJECT);
                        OperationResult::Retry(0)
                    }
                }
            });

            match ping_res {
                Ok(_) => {
                    debug!("ping OK!");
                    sleep(Duration::from_secs(600));
                } // ping every 10 minutes
                _ => {
                    error!("--------------------------------------------------");
                    error!("ping KO!! What's wrong? Let's shutdown the service");
                    error!("--------------------------------------------------");
                    sig_term_tx.send(true);
                }
            }
        }
    });
}

pub fn check_libs_directory(path: String) -> Result<(), EngineInitError> {
    match fs::read_dir(path) {
        Ok(out) => {
            let is_empty = out.take(1).count() == 0;
            match is_empty {
                true => {
                    return Err(EngineInitError::Regular(ErrorKind::LibsDirEmpty));
                }
                false => Ok(()),
            }
        }
        Err(e) => return Err(EngineInitError::Regular(ErrorKind::LibsPathsMissing)),
    }
}

fn check_if_file_exist(path: &String) -> bool {
    let path = Path::new(path);
    path.exists()
}

// check_versions_from will check (in file given in parameter) binaries versions
// will assert an error if used version installed is not not the same than written in file
fn check_versions_from(path: String) -> Result<(), EngineInitError> {
    // please append this vector if you want to test more binaries
    let bin_to_check = ["terraform"];
    // read line by line the version file
    if let Ok(lines) = read_lines(path) {
        for line in lines {
            if let Ok(bin) = line {
                // put in lowercase and split the BINARY_VERSION to BINARY
                let lowercase = bin.to_lowercase();
                let binary_name = lowercase.split("_").next().unwrap_or("");
                // check if the binary need to be tested
                if bin_to_check.contains(&binary_name) {
                    let result_cmd = cmd::utilities::run_version_command_for(&binary_name);
                    let version = lowercase.split("=").last().unwrap_or("").replace("\"", "");
                    match result_cmd.contains(&version) {
                        false => return Err(EngineInitError::Regular(ErrorKind::BinVersion)),
                        _ => info!(
                            "{} is on right version {}",
                            binary_name.to_string(),
                            version
                        ),
                    };
                }
            }
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

pub fn main() -> io::Result<()> {
    println!("{}", ASCII_BANNER);
    env_logger::init();

    let organization = env::var("ORGANIZATION");
    let cloud_provider = env::var("CLOUD_PROVIDER");
    let version_file = env::var("BIN_VERSION_FILE").expect("BIN_VERSION_FILE is mandatory");
    let region = env::var("REGION");
    let nats_server = env::var("NATS_SERVER").expect("NATS_SERVER is mandatory");
    let lib_root_dir = env::var("LIB_ROOT_DIR").unwrap_or("lib".to_string());
    let docker_host = env::var("DOCKER_HOST").ok();
    let workspace_root_dir = env::var("WORKSPACE_ROOT_DIR").unwrap_or(format!(
        "{}/.qovery-workspace",
        home_dir().unwrap().to_str().unwrap()
    ));

    info!(
        "running from current directory: {}",
        env::current_dir().unwrap().to_str().unwrap()
    );

    info!("lib root dir: {}/", lib_root_dir.as_str());
    info!("workspace root dir: {}", workspace_root_dir.as_str());

    match check_libs_directory(lib_root_dir.clone()) {
        Ok(e) => info!("Libs directory is not empty"),
        Err(e) => {
            error!("Error while initializing the Engine {}", e);
            process::exit(1);
        }
    }
    //checking if version file exist
    match check_if_file_exist(&version_file) {
        true => info!("Version file is accessible"),
        _ => {
            error!("Error while initializing the Engine, version file is not accessible");
            process::exit(1);
        }
    }

    // check all binaries version from version file
    match check_versions_from(version_file) {
        Ok(()) => info!("Binaries versions are checked"),
        Err(e) => {
            error!("Error while initializing the Engine {}", e);
            process::exit(1);
        }
    }

    match &docker_host {
        Some(docker_host) => info!("docker host: {}", docker_host),
        None => info!("docker host is not set"),
    };

    let mode = if organization.is_ok() && cloud_provider.is_ok() && region.is_ok() {
        let org = organization.unwrap().clone();
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

    match env::var("DEPLOY_FROM_FILE") {
        Err(_) => using_nats_server(
            nats_server,
            workspace_root_dir,
            lib_root_dir,
            docker_host,
            mode,
        ),
        Ok(deploy_from_file) => using_json_path_parameter(
            deploy_from_file,
            workspace_root_dir,
            lib_root_dir,
            docker_host,
        ),
    }
}

// the engine can be launch using a json file given in parameter
pub fn using_json_path_parameter(
    deploy_from_file: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<String>,
) -> Result<(), Error> {
    let pre_run_callback = Box::new(|task: &dyn Task| PreRun::Yes);

    // check if file json config file exist
    match check_if_file_exist(&deploy_from_file) {
        false => {
            error!("{} : No such file or directory", deploy_from_file);
            process::exit(1);
        }
        true => info! {"Using {} configuration file", deploy_from_file},
    }
    let (tx_task, rx_task) = unbounded::<Box<dyn Task>>();
    let task_manager = Arc::new(Mutex::new(TaskManager::new()));
    let file = File::open(deploy_from_file)?;
    let reader = BufReader::new(file);
    let json_from_file: Result<Request, _> = serde_json::from_reader(reader);
    match json_from_file {
        Ok(req) => {
            let context = Context::new(
                req.id.as_str(),
                workspace_root_dir.as_str(),
                lib_root_dir.as_str(),
                docker_host,
                req.metadata.clone(),
            );

            tx_task.send(Box::new(InfrastructureTask::new(
                context,
                req,
                pre_run_callback,
            )));
        }
        _ => error!("Error parsing the json conf file given in parameter"),
    }
    loop {
        let task = rx_task.recv().unwrap();
        task_manager.lock().unwrap().add_task(task);
        task_manager.lock().unwrap().run();
    }
    Ok(())
}

// the engine can be autonomous using the nats server to receive actions
pub fn using_nats_server(
    nats_server: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<String>,
    mode: Mode,
) -> Result<(), Error> {
    info!("NATS server: {}", nats_server.as_str());

    let name = match &mode {
        Mode::Local => "qovery-engine-app.local".to_string(),
        Mode::Cloud(organization, cloud_provider, region) => format!(
            "qovery-engine-app.{}.{}.{}",
            organization, cloud_provider, region
        ),
    };

    info!("NATS client name: {}", name.as_str());

    //let mut f = File::open("certs/ca.pem").unwrap();
    //let mut f_content = String::new();
    //f.read_to_string(&mut f_content);

    let tls_connector = TlsConnector::builder()
        //.add_root_certificate(nats::tls::Certificate::from_pem(f_content.as_bytes()).unwrap())
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .unwrap();

    info!("connect to the NATS server...");

    let nc = nats::Options::new()
        .with_name(name.as_str())
        //.tls_connector(tls_connector) // FIXME
        .connect(nats_server.as_str())?;

    info!("connection to the NATS server established");

    let nc_1 = nc.clone();

    let (tx_task, rx_task) = unbounded::<Box<dyn Task>>();
    let (tx_quit, rx_quit) = unbounded::<bool>();

    let task_manager = Arc::new(Mutex::new(TaskManager::new()));
    let t1_task_manager = task_manager.clone();

    thread::Builder::new()
        .name("tm-task-adder".to_string())
        .spawn(move || {
            let rx_status = t1_task_manager.lock().unwrap().run();

            thread::Builder::new()
                .name("t-status-core-updater".to_string())
                .spawn(move || {
                    let rx_status = rx_status.unwrap();
                    let nc = nc_1;

                    loop {
                        // send back the message to a topic: E.g core.task.status
                        // json: {"status": {"kind": "Failed", "message": "blablabla"}, "id": "abc", "created_at": "<datetime>"}
                        match rx_status.recv().unwrap() {
                            Ok(internal_task) => {
                                let sr = StatusResponse::new(
                                    internal_task.task.id().to_string(),
                                    internal_task.status,
                                );

                                let json_result = serde_json::to_string(&sr);
                                let json = json_result.unwrap();

                                debug!("send through NATS StatusResponse: {}", json.as_str());
                                let _ = nc.publish(CORE_TASK_STATUS_SUBJECT, json.as_bytes());
                            }
                            Err(err) => error!("{:?}", err),
                        };
                    }
                });

            let task_manager_is_terminated_rx = t1_task_manager.lock().unwrap().is_terminated();

            thread::Builder::new()
                .name("tm-ta-quit-handler".to_string())
                .spawn(move || {
                    // waiting for sig term to quit gracefully by waiting that there is no remaining tasks to execute.
                    let _ = task_manager_is_terminated_rx.recv();
                    tx_quit.send(true);
                });

            loop {
                let task = rx_task.recv().unwrap();
                t1_task_manager.lock().unwrap().add_task(task);
            }
        });

    let _ = listen_for_task_running_check_events(task_manager.clone(), &nc, &mode)?;

    let infrastructure_sub = listen_for_events(
        workspace_root_dir.clone(),
        lib_root_dir.clone(),
        docker_host.clone(),
        Infrastructure("infrastructure"),
        &nc,
        &mode,
        tx_task.clone(),
    )?;

    let environment_sub = listen_for_events(
        workspace_root_dir,
        lib_root_dir,
        docker_host,
        Environment("environment"),
        &nc,
        &mode,
        tx_task.clone(),
    )?;

    let (sig_term_tx, sig_term_rx) = unbounded::<bool>();

    // ping pong
    watchdog(name.clone(), nc.clone(), sig_term_tx.clone());

    thread::Builder::new()
        .name("sigterm-dispatcher".to_string())
        .spawn(move || {
            let _ = sig_term_rx.recv();
            warn!("Termination signal received - graceful termination in progress...");
            // unsubscribe listeners
            // do not unsubscribe "task_running_check_sub - it must be alive during the whole tasks completion"
            let _ = infrastructure_sub.unsubscribe();
            info!("unsubscribe from infrastructure subject");
            let _ = environment_sub.unsubscribe();
            info!("unsubscribe from environment subject");
            task_manager.lock().unwrap().stop();
            info!("request to TaskManager to stop receiving new tasks");
        });

    ctrlc::set_handler(move || {
        sig_term_tx.send(true);
    })
    .expect("Error setting Ctrl-C (SIGTERM) handler");

    info!("server started and listening for incoming requests");
    let _ = rx_quit.recv();
    // if released then quit

    warn!("end of execution");
    Ok(())
}
