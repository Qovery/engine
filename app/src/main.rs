#[macro_use]
extern crate log;
#[macro_use]
extern crate prometheus;
#[macro_use]
extern crate serde;

use std::fs::File;
use std::io::{BufRead, BufReader, Error};
use std::path::Path;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use std::{env, thread};
use std::{fs, io, process};

use chrono::Utc;
use crossbeam_channel::{unbounded, Sender};
use dirs::home_dir;
use dotenv::dotenv;
use retry::delay::Fibonacci;
use retry::OperationResult;
use uuid::Uuid;

use qovery_engine::cmd;
use qovery_engine::models::Context;
use qovery_engine_task_manager::models::Request;
use qovery_engine_task_manager::task_manager::{PreRun, Task, TaskManager};
use qovery_engine_task_manager::tasks::{EnvironmentTask, InfrastructureTask};
use qovery_engine_task_manager::utils::LogErrorOnDrop;
use utils::Mode;

use crate::constants::ASCII_BANNER;
use crate::custom_error::ErrorKind::BinVersion;
use crate::custom_error::{EngineInitError, ErrorKind};
use crate::models::TaskSelector::{Environment, Infrastructure};
use crate::models::{Ping, Response, StatusResponse, TaskSelector};
use crate::nats::{subjects, Connection, Message, Subscription};
use std::env::VarError;

mod constants;
mod custom_error;
mod models;
mod nats;
mod utils;
mod webserver;

fn listen_for_events(
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_tcp_socket: Option<String>,
    task_selector: &TaskSelector,
    nc: &Connection,
    mode: &Mode,
    tx: Sender<Box<dyn Task>>,
) -> Result<Subscription, Error> {
    let subject_name = subjects::get_subject_name(mode, task_selector);
    let sub = nc.queue_subscribe(&subject_name)?;
    info!("subscribe to {:?}", subject_name);

    let ts_str = match task_selector {
        Infrastructure(_) => "infrastructure",
        Environment(_) => "environment",
    };

    let _ = {
        let thread_name = format!("nats-event-receiver-{}", ts_str);
        let sub = sub.clone();
        let task_selector = *task_selector;

        thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || {
                let _drop_logger = LogErrorOnDrop::new(&thread_name);

                loop {
                    for msg in sub.next() {
                        debug!("{}", msg);

                        receive_and_queue_task(
                            msg,
                            workspace_root_dir.as_str(),
                            lib_root_dir.as_str(),
                            &docker_tcp_socket,
                            &task_selector,
                            &tx,
                        );
                    }
                }
            })
            .unwrap()
    };

    Ok(sub)
}

fn receive_and_queue_task(
    msg: Message,
    workspace_root_dir: &str,
    lib_root_dir: &str,
    docker_tcp_socket: &Option<String>,
    task_selector: &TaskSelector,
    tx: &Sender<Box<dyn Task>>,
) {
    // call before to run the current task to check that the same task is not running on another engine app
    // check is_the_same_task_running and is_the_next_task_to_run functions to know more about the details
    let pre_run_callback = Box::new(move |_task: &dyn Task| PreRun::Yes);

    match serde_json::from_slice::<Request>(&msg.data) {
        Ok(req) => {
            let context = Context::new(
                req.id.to_string(),
                workspace_root_dir.to_string(),
                lib_root_dir.to_string(),
                docker_tcp_socket.clone(),
                req.metadata.clone(),
            );

            let task: Box<dyn Task> = match task_selector {
                TaskSelector::Infrastructure(_) => Box::new(InfrastructureTask::new(context, req, pre_run_callback)),
                TaskSelector::Environment(_) => Box::new(EnvironmentTask::new(context, req, pre_run_callback)),
            };

            let _ = tx
                .send(task)
                .map_err(|err| error!("Cannot send task receive_and_queue_task: {}", err));

            let _ = msg
                .respond(Response::new(None).as_json_string())
                .map_err(|err| error!("Cannot respond to nats receive_and_queue_task: {}", err));
        }
        Err(err) => {
            error!("{}", msg);
            error!("receiving request but JSON decoding error occurred: {:?}", err);
            let _ = msg
                .respond(Response::new(Some(err.to_string())).as_json_string())
                .map_err(|err| error!("Cannot send reponse to nats receive_and_queue_task: {}", err));
        }
    };
}

/// Notify the core server that this engine exists and is running
/// if the server does not respond - then retry 10 times (with fibonacci retry) -
/// if it does not respond after all attempts, then gracefully restart the service.
fn watchdog(name: String, nc: Connection, sig_term_tx: Sender<bool>) {
    let _ = thread::Builder::new()
        .name("watchdog".to_string())
        .spawn(move || {
            let _drop_logger = LogErrorOnDrop::new("watchdog");
            let engine_started_at = Utc::now();
            let ping = Ping::new(engine_started_at, name.as_str());
            let json = serde_json::to_string(&ping).unwrap();
            let err_msg = r#"
--------------------------------------------------
Ping KO!! What's wrong? Let's shutdown the service
--------------------------------------------------
"#;
            loop {
                let ping_res = retry::retry(Fibonacci::from_millis(3000).take(10), || {
                    let res = nc.request_timeout(&subjects::CORE_PING, json.as_bytes(), Duration::from_secs(5));
                    match res {
                        Ok(_) => OperationResult::Ok(0),
                        _ => {
                            warn!(
                                "ping failed ({:?}), let's retry to ping the core server in a few seconds",
                                *subjects::CORE_PING
                            );
                            OperationResult::Retry(0)
                        }
                    }
                });

                match ping_res {
                    Ok(_) => {
                        debug!("ping OK!");
                        sleep(Duration::from_secs(600));
                    }
                    Err(_) => {
                        error!("{}", err_msg);
                        let _ = sig_term_tx
                            .send(true)
                            .map_err(|err| error!("Cannot send sig term signal in watchdog {}", err));
                        return;
                    }
                }
            }
        })
        .unwrap();
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
            let result_cmd = cmd::utilities::run_version_command_for(&binary_name);
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
    env_logger::init();

    let http_listen_on = env::var("HTTP_LISTEN_ON").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let engine_id = env::var("ID").unwrap_or_else(|_| generate_id().to_string());
    let organization = env::var("ORGANIZATION");
    let cloud_provider = env::var("CLOUD_PROVIDER");
    let version_file = env::var("BIN_VERSION_FILE").expect("BIN_VERSION_FILE is mandatory");
    let region = env::var("REGION");
    let nats_server = env::var("QOVERY_NATS_URL").expect("QOVERY_NATS_URL is mandatory");
    let lib_root_dir = env::var("LIB_ROOT_DIR").unwrap_or_else(|_| "lib".to_string());
    let docker_host = env::var("DOCKER_HOST").ok();
    let workspace_root_dir = env::var("WORKSPACE_ROOT_DIR")
        .unwrap_or(format!("{}/.qovery-workspace", home_dir().unwrap().to_str().unwrap()));

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

    match env::var("DEPLOY_FROM_FILE") {
        Ok(deploy_from_file) => match env::var("DEPLOY_FROM_FILE_KIND") {
            Ok(value) => match value.as_str() {
                "infra" => using_json_path_parameter(
                    deploy_from_file,
                    workspace_root_dir,
                    lib_root_dir,
                    TaskSelector::Infrastructure(""),
                    docker_host,
                ),
                "env" => using_json_path_parameter(
                    deploy_from_file,
                    workspace_root_dir,
                    lib_root_dir,
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
        _ => using_nats_server(nats_server, workspace_root_dir, lib_root_dir, docker_host, mode),
    }
}

// the engine can be launch using a json file given in parameter
pub fn using_json_path_parameter(
    deploy_from_file: String,
    workspace_root_dir: String,
    lib_root_dir: String,
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
    let req: Request = serde_json::from_reader(file)
        .map_err(|err| {
            error!("Impossible to parse json file: {}", err);
            process::exit(1);
        })
        .unwrap();

    let mut task_manager = TaskManager::new();
    let context = Context::new(
        req.id.to_string(),
        workspace_root_dir,
        lib_root_dir,
        docker_host,
        req.metadata.clone(),
    );

    let task: Box<dyn Task> = match deployment_type {
        TaskSelector::Environment(_) => Box::new(EnvironmentTask::new(
            context.clone(),
            req.clone(),
            Box::new(|_: &dyn Task| PreRun::Yes),
        )),
        TaskSelector::Infrastructure(_) => Box::new(InfrastructureTask::new(
            context,
            req,
            Box::new(|_: &dyn Task| PreRun::Yes),
        )),
    };

    task_manager.add_task(task);
    let _ = task_manager.run();

    loop {
        std::thread::park();
    }
}

// the engine can be autonomous using the nats server to receive actions
fn using_nats_server(
    nats_server: String,
    workspace_root_dir: String,
    lib_root_dir: String,
    docker_host: Option<String>,
    mode: Mode,
) -> Result<(), Error> {
    info!("NATS server: {}", nats_server.as_str());

    let name = match &mode {
        Mode::Local => "qovery-engine-app.local".to_string(),
        Mode::Cloud(organization, cloud_provider, region) => {
            format!("qovery-engine-app.{}.{}.{}", organization, cloud_provider, region)
        }
    };

    info!("NATS client name: {}", name.as_str());

    //let mut f = File::open("certs/ca.pem").unwrap();
    //let mut f_content = String::new();
    //f.read_to_string(&mut f_content);

    // let _tls_connector = TlsConnector::builder()
    // .add_root_certificate(nats::tls::Certificate::from_pem(f_content.as_bytes()).unwrap())
    // .danger_accept_invalid_certs(true)
    // .danger_accept_invalid_hostnames(true)
    // .build()
    // .unwrap();

    info!("connect to the NATS server...");
    let nc = Connection::new(name.as_str(), nats_server.as_str())?;
    info!("connection to the NATS server established");

    let (task_tx, task_rx) = unbounded::<Box<dyn Task>>();
    let mut tm = TaskManager::new();
    let status_rx = tm.run().unwrap();
    let task_manager = Arc::new(tm);

    let _ = {
        let thread_name = "tm-status-core-updater";
        let nc = nc.clone();
        let func = move || {
            let _drop_logger = LogErrorOnDrop::new(thread_name);
            loop {
                // send back the message to a topic: E.g core.task.status
                // json: {"status": {"kind": "Failed", "message": "blablabla"}, "id": "abc", "created_at": "<datetime>"}
                match status_rx.recv() {
                    Ok(Ok(internal_task)) => {
                        let sr = StatusResponse::new(internal_task.task.id().to_string(), internal_task.status);
                        let json = serde_json::to_string(&sr).unwrap();
                        debug!("send through NATS StatusResponse: {}", json.as_str());
                        let _ = nc
                            .publish(&subjects::CORE_TASK_STATUS, json.as_bytes())
                            .map_err(|err| error!("Cannot publish on {}: {}", subjects::CORE_TASK_STATUS.name, err));
                    }
                    Ok(Err(err)) => error!("{:?}", err),
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

    let _ = {
        let thread_name = "tm-task-adder";
        let task_manager = task_manager.clone();
        let func = move || {
            let _drop_logger = LogErrorOnDrop::new(thread_name);

            // FIXME instead of manually implementing the loadbalincing ourselves between engines
            // we should just use NATS queue groups and stop picking task when the taskManager
            // is currently busy.
            // The issue is that there is no coordination between threads inside the app
            // and that each one is unqueue tasks from NATS without really knowing if we can
            // process them downstream.
            // A quickfix would be to use bounded queue instead of unbounded one to force
            // upstream thread to block if the engine can't accept new task quickly enough
            loop {
                let task = task_rx.recv().unwrap();
                // load balance workload before dispatching the task to the current task manager.
                // request info from other engines
                // to know how many remaining tasks to run they have?
                task_manager.add_task(task);
            }
        };

        thread::Builder::new()
            .name(thread_name.to_string())
            .spawn(func)
            .unwrap()
    };

    let infrastructure_task_selector = TaskSelector::Infrastructure("infrastructure");
    let environment_task_selector = TaskSelector::Environment("environment");

    listen_for_events(
        workspace_root_dir.clone(),
        lib_root_dir.clone(),
        docker_host.clone(),
        &infrastructure_task_selector,
        &nc,
        &mode,
        task_tx.clone(),
    )?;

    listen_for_events(
        workspace_root_dir,
        lib_root_dir,
        docker_host,
        &environment_task_selector,
        &nc,
        &mode,
        task_tx.clone(),
    )?;

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

    watchdog(name, nc, sig_term_tx.clone());
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
