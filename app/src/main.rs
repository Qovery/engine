#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;

mod constants;

use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::fs::File;
use std::io::{Error, Read, Write};
use std::thread::sleep;
use std::time::Duration;
use std::{env, thread};

use crossbeam_channel::{unbounded, Sender};
use nats::{Connection, Subscription};

use qovery_engine_shared::{subject, Mode};
use qovery_engine_task_manager::models::{Request, Response};
use qovery_engine_task_manager::task_manager::{InternalTask, Status, Task, TaskManager};
use qovery_engine_task_manager::tasks::{EnvironmentTask, InfrastructureTask};

use crate::constants::ASCII_BANNER;
use crate::TaskSelector::{Environment, Infrastructure};
use chrono::{DateTime, Utc};
use dirs::home_dir;
use nats::tls::{Identity, TlsConnector, TlsConnectorBuilder};
use qovery_engine::models::Context;
use retry::delay::Fibonacci;
use retry::OperationResult;
use std::path::Path;
use std::sync::{Arc, Mutex};

const CORE_TASK_STATUS_SUBJECT: &str = "core.task.status";
const CORE_PING_SUBJECT: &str = "core.ping";

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

fn listen_for_events(
    workspace_root_dir: String,
    lib_root_dir: String,
    task_selector: TaskSelector,
    nc: &Connection,
    mode: &Mode,
    tx: Sender<Box<dyn Task>>,
) -> Result<Subscription, Error> {
    let subject_name = subject_name(mode, &task_selector);
    let sub = nc.queue_subscribe(subject_name.as_str(), subject_name.as_str())?;
    info!("subscribe to {}", subject_name.as_str());

    let _ = sub.clone().with_handler(move |msg| {
        debug!("{}", msg);
        match serde_json::from_slice::<Request>(msg.data.as_slice()) {
            Ok(req) => {
                let context = Context::new(
                    req.id.as_str(),
                    workspace_root_dir.as_str(),
                    lib_root_dir.as_str(),
                );

                tx.send(match task_selector {
                    TaskSelector::Infrastructure(_) => {
                        Box::new(InfrastructureTask::new(context, req))
                    }
                    TaskSelector::Environment(_) => Box::new(EnvironmentTask::new(context, req)),
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

        Ok(())
    });

    Ok(sub)
}

/// Notify the core server that this engine exists and is running
/// if the server does not respond - then retry 10 times (with fibonacci retry) -
/// if it does not respond after all attempts, then gracefully restart the service.
fn watchdog(name: String, nc: Connection, sig_term_tx: Sender<bool>) {
    thread::spawn(move || {
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

pub fn main() -> Result<(), Error> {
    println!("{}", ASCII_BANNER);
    env_logger::init();

    let organization = env::var("ORGANIZATION");
    let cloud_provider = env::var("CLOUD_PROVIDER");
    let region = env::var("REGION");
    let nats_server = env::var("NATS_SERVER").expect("NATS_SERVER is mandatory");
    let lib_root_dir = env::var("LIB_ROOT_DIR").unwrap_or("lib".to_string());
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

    thread::spawn(move || {
        let rx_status = t1_task_manager.lock().unwrap().run();

        thread::spawn(move || {
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

                        debug!("status response: {}", json.as_str());
                        let _ = nc.request_timeout(
                            CORE_TASK_STATUS_SUBJECT,
                            json.as_bytes(),
                            Duration::from_secs(60),
                        );
                        // FIXME handle timeout?
                    }
                    Err(err) => {}
                };
            }
        });

        let task_manager_is_terminated_rx = t1_task_manager.lock().unwrap().is_terminated();

        thread::spawn(move || {
            // waiting for sig term to quit gracefully by waiting that there is no remaining tasks to execute.
            let _ = task_manager_is_terminated_rx.recv();
            tx_quit.send(true);
        });

        loop {
            let task = rx_task.recv().unwrap();
            t1_task_manager.lock().unwrap().add_task(task);
        }
    });

    let infrastructure_sub = listen_for_events(
        workspace_root_dir.clone(),
        lib_root_dir.clone(),
        Infrastructure("infrastructure"),
        &nc,
        &mode,
        tx_task.clone(),
    )?;

    let environment_sub = listen_for_events(
        workspace_root_dir,
        lib_root_dir,
        Environment("environment"),
        &nc,
        &mode,
        tx_task.clone(),
    )?;

    let (sig_term_tx, sig_term_rx) = unbounded::<bool>();

    // ping pong
    watchdog(name.clone(), nc.clone(), sig_term_tx.clone());

    thread::spawn(move || {
        let _ = sig_term_rx.recv();
        warn!("Termination signal received - graceful termination in progress...");
        // unsubscribe listeners
        infrastructure_sub.unsubscribe();
        environment_sub.unsubscribe();
        task_manager.lock().unwrap().stop();
    });

    ctrlc::set_handler(move || {
        sig_term_tx.send(true);
    })
    .expect("Error setting Ctrl-C (Sig. Term) handler");

    info!("server started and listening for incoming requests");
    let _ = rx_quit.recv();
    // if released then quit

    warn!("end of execution");
    Ok(())
}
