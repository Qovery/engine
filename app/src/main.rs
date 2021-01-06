#[macro_use] extern crate log;
#[macro_use] extern crate serde;
#[macro_use] extern crate prometheus;


use std::fs::File;
use std::io::{BufRead, BufReader, Error};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use std::{env, thread};
use std::{fs, io, process};

use chrono::Utc;
use crossbeam_channel::{unbounded, Sender};
use dirs::home_dir;
use nats::{Connection, Message, Subscription};
use qovery_engine::cmd;
use qovery_engine::models::Context;
use retry::delay::Fibonacci;
use retry::OperationResult;
use uuid::Uuid;
use dotenv::dotenv;

use qovery_engine_shared::{subject, Mode};
use qovery_engine_task_manager::models::Request;
use qovery_engine_task_manager::task_manager::{PreRun, State, Task, TaskManager};
use qovery_engine_task_manager::tasks::{EnvironmentTask, InfrastructureTask};

use crate::constants::ASCII_BANNER;
use crate::custom_error::{EngineInitError, ErrorKind};
use crate::models::TaskSelector::{Environment, Infrastructure};
use crate::models::{
    CheckTaskOrderRequest, CheckTaskOrderResponse, CheckTaskRunningResponse,
    GetTaskManagerInfoRequest, GetTaskManagerInfoResponse, Ping, Response, StatusResponse,
    TaskSelector,
};
use std::borrow::Borrow;
use std::panic::resume_unwind;
use qovery_engine_task_manager::LogErrorOnDrop;

mod constants;
mod custom_error;
mod models;
mod webserver;

const CORE_TASK_STATUS_SUBJECT: &str = "core.task.status";
const CORE_PING_SUBJECT: &str = "core.ping";
const ENGINE_TASK_RUNNING_CHECK_SUBJECT: &str = "engine.task_running_check";
const ENGINE_TASK_ORDER_EXECUTION_CHECK_SUBJECT: &str = "engine.task_order_execution_check";
const ENGINE_INCOMING_TASK_SUBJECT: &str = "engine.incoming_task";
const ENGINE_TASK_MANAGER_INFO_SUBJECT: &str = "engine.task_manager_info";

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

        let _ = msg.respond(Response::new(None).as_json_string());

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

        let is_running = match task_manager.try_lock() {
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

        info!(
            "task with group id {} is {} here",
            group_id,
            match is_running {
                true => "running",
                false => "not running",
            }
        );

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

        let _ = msg.respond(Response::new(None).as_json_string());

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

        let is_first_place = match task_manager.try_lock() {
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
                info!("lock error - delay the deployment to not take any risk");
                false
            }
        };

        info!(
            "task with group id {} is {} to be executed here",
            req.group_id,
            match is_first_place {
                true => "in first position",
                false => "not in first position",
            }
        );

        let _ = msg.respond(CheckTaskOrderResponse::new(is_first_place).as_json_string());

        Ok(())
    });

    Ok(sub)
}

fn task_manager_info_subject_name(mode: &Mode) -> String {
    subject(&mode, ENGINE_TASK_MANAGER_INFO_SUBJECT)
}

/// This function listen for task coming from other engines to load balance them.
fn listen_for_incoming_load_balancing_tasks(
    engine_id: &str,
    task_manager: Arc<Mutex<TaskManager>>,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_tcp_socket: Option<String>,
    task_selector: &TaskSelector,
    nc: &Connection,
    mode: &Mode,
    tx: Sender<Box<dyn Task>>,
) -> Result<(Subscription, Subscription), Error> {
    let incoming_task_subject_name = subject(
        &mode,
        &*format!("{}.{}", ENGINE_INCOMING_TASK_SUBJECT, engine_id), // unique subject for each engine instance
    );

    let incoming_task_sub = nc.subscribe(incoming_task_subject_name.as_str())?;
    let incoming_task_sub_1 = incoming_task_sub.clone();
    info!("subscribe to {}", incoming_task_subject_name.as_str());

    let tm_info_subject_name = task_manager_info_subject_name(&mode);
    let tm_info_task_sub = nc.subscribe(tm_info_subject_name.as_str())?;
    let tm_info_task_sub_1 = tm_info_task_sub.clone();
    info!("subscribe to {}", tm_info_subject_name.as_str());

    let nc = nc.clone();
    let engine_id = engine_id.to_string();
    let task_selector = task_selector.clone();
    let subject_name = tm_info_subject_name.clone();
    let workspace_root_dir = workspace_root_dir.clone();
    let lib_root_dir = lib_root_dir.clone();
    let docker_tcp_socket = docker_tcp_socket.clone();
    let mode = mode.clone();

    // incoming tasks receiver
    let thread_name = format!("incoming-lb-task-{}-{}", task_selector.name(), generate_id());
    let _ = thread::Builder::new()
        .name(thread_name.clone())
        .spawn(move || {
            let _drop_logger = LogErrorOnDrop::new(&thread_name);
            let nc = nc.clone();
            let task_selector = task_selector.clone();
            let workspace_root_dir = workspace_root_dir.clone();
            let lib_root_dir = lib_root_dir.clone();
            let docker_tcp_socket = docker_tcp_socket.clone();
            let mode = mode.clone();

            loop {
                for msg in incoming_task_sub.next() {
                    receive_and_queue_task(
                        msg,
                        workspace_root_dir.clone(),
                        lib_root_dir.clone(),
                        docker_tcp_socket.clone(),
                        &task_selector,
                        &nc,
                        &mode,
                        &tx,
                    );
                }
            }
        });

    // respond to get info request on the task manager remaining tasks to run
    let thread_name = format!("tm-get-info-{}-{}", task_selector.name(), generate_id());
    let _ = thread::Builder::new()
        .name(thread_name.clone())
        .spawn(move || {
            let _drop_logger = LogErrorOnDrop::new(&thread_name);
            let engine_id = engine_id.clone();
            let subject_name = subject_name.clone();

            loop {
                for msg in tm_info_task_sub.next() {
                    let remaining_tasks_to_run = match task_manager.try_lock() {
                        Ok(tm) => tm.remaining_tasks_to_run(),
                        Err(_) => 10_000, // set to 10 000 to make the engine unlikely taking a task
                    };

                    let res = GetTaskManagerInfoResponse::new(
                        engine_id.as_str(),
                        subject_name.as_str(),
                        remaining_tasks_to_run,
                    );

                    info!("response to current task manager info request: {:?}", res);

                    let _ = msg.respond(res.as_json_string());
                }
            }
        });

    Ok((incoming_task_sub_1, tm_info_task_sub_1))
}

fn listen_for_events(
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_tcp_socket: Option<String>,
    task_selector: &TaskSelector,
    nc: &Connection,
    mode: &Mode,
    tx: Sender<Box<dyn Task>>,
) -> Result<Subscription, Error> {
    let subject_name = subject_name(mode, &task_selector);
    let sub = nc.queue_subscribe(subject_name.as_str(), subject_name.as_str())?;
    info!("subscribe to {}", subject_name.as_str());

    let nc = nc.clone();
    let sub_1 = sub.clone();
    let task_selector = task_selector.clone();
    let mode = mode.clone();
    let ts_str = match task_selector {
        Infrastructure(_) => "infrastructure",
        Environment(_) => "environment",
    };

    let thread_name = format!("nats-event-receiver-{}", ts_str);
    let _ = thread::Builder::new()
        .name(thread_name.clone())
        .spawn(move || {
            let _drop_logger = LogErrorOnDrop::new(&thread_name);
            let nc = nc.clone();

            loop {
                for msg in sub.next() {
                    debug!("{}", msg);

                    receive_and_queue_task(
                        msg,
                        workspace_root_dir.clone(),
                        lib_root_dir.clone(),
                        docker_tcp_socket.clone(),
                        &task_selector,
                        &nc,
                        &mode,
                        &tx,
                    );
                }
            }
        });

    Ok(sub_1)
}

fn receive_and_queue_task(
    msg: Message,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_tcp_socket: Option<String>,
    task_selector: &TaskSelector,
    nc: &Connection,
    mode: &Mode,
    tx: &Sender<Box<dyn Task>>,
) {
    let nc = nc.clone();
    let mode = mode.clone();

    // call before to run the current task to check that the same task is not running on another engine app
    // check is_the_same_task_running and is_the_next_task_to_run functions to know more about the details
    let pre_run_callback = Box::new(move |task: &dyn Task| {
        PreRun::add(
            is_the_same_task_running(task, nc.clone(), mode.clone()),
            is_the_next_task_to_run(task, nc.clone(), mode.clone()),
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
                TaskSelector::Infrastructure(_) => {
                    Box::new(InfrastructureTask::new(context, req, pre_run_callback))
                }
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

/// Notify the core server that this engine exists and is running
/// if the server does not respond - then retry 10 times (with fibonacci retry) -
/// if it does not respond after all attempts, then gracefully restart the service.
fn watchdog(name: String, nc: Connection, sig_term_tx: Sender<bool>) {
    let _ = thread::Builder::new().name("watchdog".to_string()).spawn(move || {
        let _drop_logger = LogErrorOnDrop::new("watchdog");
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
                    error!(r#"
--------------------------------------------------
Ping KO!! What's wrong? Let's shutdown the service
--------------------------------------------------
"#);
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
                true => Err(EngineInitError::Regular(ErrorKind::LibsDirEmpty)),
                false => Ok(()),
            }
        }
        Err(_) => Err(EngineInitError::Regular(ErrorKind::LibsPathsMissing)),
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

fn generate_id() -> u32 {
    Uuid::new_v4().as_fields().0
}



pub fn main() -> io::Result<()> {
    println!("{}", ASCII_BANNER);

    // Load env variable from .env file
    dotenv().ok();
    env_logger::init();

    let http_listen_on = env::var("HTTP_LISTEN_ON").unwrap_or("0.0.0.0:8080".to_string());
    let engine_id = env::var("ID").unwrap_or(generate_id().to_string());
    let organization = env::var("ORGANIZATION");
    let cloud_provider = env::var("CLOUD_PROVIDER");
    let version_file = env::var("BIN_VERSION_FILE").expect("BIN_VERSION_FILE is mandatory");
    let region = env::var("REGION");
    let nats_server = env::var("QOVERY_NATS_URL").expect("QOVERY_NATS_URL is mandatory");
    let lib_root_dir = env::var("LIB_ROOT_DIR").unwrap_or("lib".to_string());
    let docker_host = env::var("DOCKER_HOST").ok();
    let workspace_root_dir = env::var("WORKSPACE_ROOT_DIR").unwrap_or(format!(
        "{}/.qovery-workspace",
        home_dir().unwrap().to_str().unwrap()
    ));

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

    webserver::launch(http_listen_on.as_str());

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
        Ok(deploy_from_file) => using_json_path_parameter(
            deploy_from_file,
            workspace_root_dir,
            lib_root_dir,
            docker_host,
        ),
        _ => using_nats_server(
            engine_id,
            nats_server,
            workspace_root_dir,
            lib_root_dir,
            docker_host,
            mode,
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
    let pre_run_callback = Box::new(|_: &dyn Task| PreRun::Yes);

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

            let _ = tx_task.send(Box::new(InfrastructureTask::new(
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
        let _ = task_manager.lock().unwrap().run();
    }
}

/// Request engines (within the same zone) to give their Task Manager infos.
fn list_engines_task_manager_infos(
    mode: &Mode,
    nc: &Connection,
    request: GetTaskManagerInfoRequest,
) -> Vec<GetTaskManagerInfoResponse> {
    let tm_info_subject_name = subject(mode, ENGINE_TASK_MANAGER_INFO_SUBJECT);

    let sub = match nc.request_multi(tm_info_subject_name.as_str(), request.as_json_string()) {
        Ok(sub) => sub,
        Err(_) => {
            error!("can't get task manager infos from other engines");
            // FIXME should we retry?
            return vec![];
        }
    };

    let mut results: Vec<GetTaskManagerInfoResponse> = Vec::with_capacity(5);

    for msg in sub.next() {
        match serde_json::from_slice::<GetTaskManagerInfoResponse>(msg.data.as_slice()) {
            Ok(tm_info) => results.push(tm_info),
            Err(err) => error!("{:?}", err),
        };
    }

    results
}

/// This function help to find the engine where to dispatch the next task.
/// The choice is based on the less loaded by counting the number of tasks that remained to be run.
/// If there is no result (E.g network issue) - then return None
fn find_the_engine_where_to_dispatch_the_next_task(
    mut task_manager_infos: Vec<GetTaskManagerInfoResponse>,
) -> Option<GetTaskManagerInfoResponse> {
    let _ = task_manager_infos.sort_by_key(|x| x.remaining_tasks_to_run);

    match task_manager_infos.first() {
        Some(v) => Some(v.clone()),
        None => None,
    }
}

/// This function dispatch a task to an engine through NATS
fn dispatch_task_to_engine(task: &dyn Task, subject_name: &str, nc: &Connection) {
    // FIXME should I wait for ack?
    nc.publish(
        subject_name,
        serde_json::to_string(task.bytes_payload().as_slice()).unwrap(),
    );
}

// the engine can be autonomous using the nats server to receive actions
fn using_nats_server(
    engine_id: String,
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

    /*let _tls_connector = TlsConnector::builder()
    //.add_root_certificate(nats::tls::Certificate::from_pem(f_content.as_bytes()).unwrap())
    .danger_accept_invalid_certs(true)
    .danger_accept_invalid_hostnames(true)
    .build()
    .unwrap();*/

    info!("connect to the NATS server...");

    let nc = nats::Options::new()
        .with_name(name.as_str())
        //.tls_connector(tls_connector) // FIXME
        .connect(nats_server.as_str())?;

    info!("connection to the NATS server established");

    let nc_1 = nc.clone();
    let nc_2 = nc.clone();

    let mode_1 = mode.clone();
    let engine_id_1 = engine_id.clone();

    let (tx_task, rx_task) = unbounded::<Box<dyn Task>>();
    let (tx_quit, rx_quit) = unbounded::<bool>();

    let task_manager = Arc::new(Mutex::new(TaskManager::new()));
    let t1_task_manager = task_manager.clone();

    let _ = thread::Builder::new()
        .name("tm-task-adder".to_string())
        .spawn(move || {
            let _drop_logger = LogErrorOnDrop::new("tm-task-adder");
            let rx_status = t1_task_manager.lock().unwrap().run();

            let _ = thread::Builder::new()
                .name("tm-status-core-updater".to_string())
                .spawn(move || {
                    let _drop_logger = LogErrorOnDrop::new("tm-status-core-updater");
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

            let _ = thread::Builder::new()
                .name("tm-ta-quit-handler".to_string())
                .spawn(move || {
                    let _drop_logger = LogErrorOnDrop::new("tm-ta-quit-handler");
                    // waiting for sig term to quit gracefully by waiting that there is no remaining tasks to execute.
                    let _ = task_manager_is_terminated_rx.recv();
                    let _ = tx_quit.send(true);
                });

            let nc = nc_2.clone();
            let mode = mode_1.clone();
            let engine_id = engine_id_1.clone();

            loop {
                let task = rx_task.recv().unwrap();
                // load balance workload before dispatching the task to the current task manager.
                match t1_task_manager.try_lock() {
                    Ok(mut tm) => {
                        // request info from other engines
                        // to know how many remaining tasks to run they have?
                        match find_the_engine_where_to_dispatch_the_next_task(
                            list_engines_task_manager_infos(
                                &mode,
                                &nc,
                                GetTaskManagerInfoRequest::new(engine_id.as_str()),
                            ),
                        ) {
                            None => tm.add_task(task), // This is a default/fallback choice. Add the task into the local engine. Maybe not the best choice, but better than nothing.
                            Some(tm_info) => {
                                // FIXME how to prevent ping/pong between engines?
                                // dispatch the task into the best engine
                                if tm_info.engine_id == engine_id.as_str() {
                                    // the local engine is less loaded than the others
                                    tm.add_task(task);
                                } else {
                                    // another engine is less loaded
                                    let tm_info_subject_name =
                                        task_manager_info_subject_name(&mode);

                                    dispatch_task_to_engine(
                                        task.borrow(),
                                        tm_info_subject_name.as_str(),
                                        &nc,
                                    );
                                }
                            }
                        };
                    }
                    Err(err) => {
                        error!("{:?}", err);
                        warn!("wait for 5 seconds prior to try to add task again");
                        thread::sleep(Duration::from_secs(5));
                    }
                };
            }
        });

    let _ = listen_for_task_running_check_events(task_manager.clone(), &nc, &mode)?;
    let _ = listen_for_task_order_execution_check(task_manager.clone(), &nc, &mode)?;

    let infrastructure_task_selector = TaskSelector::Infrastructure("infrastructure");
    let environment_task_selector = TaskSelector::Environment("environment");

    let (lb_infrastructure_incoming_task_sub, lb_infrastructure_tm_info_sub) =
        listen_for_incoming_load_balancing_tasks(
            engine_id.as_str(),
            task_manager.clone(),
            workspace_root_dir.clone(),
            lib_root_dir.clone(),
            docker_host.clone(),
            &infrastructure_task_selector,
            &nc,
            &mode,
            tx_task.clone(),
        )?;

    let (lb_environment_incoming_task_sub, lb_environment_tm_info_sub) =
        listen_for_incoming_load_balancing_tasks(
            engine_id.as_str(),
            task_manager.clone(),
            workspace_root_dir.clone(),
            lib_root_dir.clone(),
            docker_host.clone(),
            &environment_task_selector,
            &nc,
            &mode,
            tx_task.clone(),
        )?;

    let infrastructure_sub = listen_for_events(
        workspace_root_dir.clone(),
        lib_root_dir.clone(),
        docker_host.clone(),
        &infrastructure_task_selector,
        &nc,
        &mode,
        tx_task.clone(),
    )?;

    let environment_sub = listen_for_events(
        workspace_root_dir,
        lib_root_dir,
        docker_host,
        &environment_task_selector,
        &nc,
        &mode,
        tx_task.clone(),
    )?;

    let (sig_term_tx, sig_term_rx) = unbounded::<bool>();

    // ping pong
    watchdog(name.clone(), nc.clone(), sig_term_tx.clone());

    let _ = thread::Builder::new()
        .name("sigterm-dispatcher".to_string())
        .spawn(move || {
            let _drop_logger = LogErrorOnDrop::new("sigterm-dispatcher");
            let _ = sig_term_rx.recv();
            warn!("Termination signal received - graceful termination in progress...");
            // unsubscribe listeners
            // do not unsubscribe "task_running_check_sub - it must be alive during the whole tasks completion"
            let _ = infrastructure_sub.unsubscribe();
            let _ = environment_sub.unsubscribe();
            let _ = lb_infrastructure_incoming_task_sub.unsubscribe();
            let _ = lb_infrastructure_tm_info_sub.unsubscribe();
            let _ = lb_environment_incoming_task_sub.unsubscribe();
            let _ = lb_environment_tm_info_sub.unsubscribe();
            info!("unsubscribe from all subjects");
            task_manager.lock().unwrap().stop();
            info!("request to TaskManager to stop receiving new tasks");
        });

    ctrlc::set_handler(move || {
        let _ = sig_term_tx.send(true);
    })
    .expect("Error setting Ctrl-C (SIGTERM) handler");

    info!("server started and listening for incoming requests");
    let _ = rx_quit.recv();
    // if released then quit

    warn!("end of execution");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::find_the_engine_where_to_dispatch_the_next_task;
    use crate::models::GetTaskManagerInfoResponse;

    #[test]
    fn find_the_engine_where_to_dispatch_the_next_task_tests() {
        let x = GetTaskManagerInfoResponse::new("abc", "a.b.c", 4);
        let y = GetTaskManagerInfoResponse::new("def", "d.e.f", 1);
        let z = GetTaskManagerInfoResponse::new("ghi", "g.h.i", 2);

        assert_eq!(
            find_the_engine_where_to_dispatch_the_next_task(vec![x, y, z])
                .unwrap()
                .remaining_tasks_to_run,
            1
        );

        match find_the_engine_where_to_dispatch_the_next_task(vec![]) {
            Some(_) => assert!(false),
            None => assert!(true),
        };
    }
}
