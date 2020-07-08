#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;

use std::borrow::Borrow;
use std::fs::File;
use std::io::{Error, Read, Write};
use std::thread::sleep;
use std::time::Duration;
use std::{env, thread};

use crossbeam_channel::{unbounded, Sender};
use nats::Connection;

use qovery_engine_shared::{subject, Mode};
use qovery_engine_task_manager::models::{Request, Response};
use qovery_engine_task_manager::task_manager::{Task, TaskManager};
use qovery_engine_task_manager::tasks::{EnvironmentTask, InfrastructureTask};

use crate::TaskSelector::{Environment, Infrastructure};
use nats::tls::{Identity, TlsConnector, TlsConnectorBuilder};
use std::path::Path;

enum TaskSelector {
    Infrastructure(&'static str),
    Environment(&'static str),
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
    task_selector: TaskSelector,
    nc: &Connection,
    mode: &Mode,
    tx: Sender<Box<dyn Task>>,
) -> Result<(), Error> {
    let subject_name = subject_name(mode, &task_selector);
    let sub = nc.queue_subscribe(subject_name.as_str(), subject_name.as_str())?;
    info!("subscribe to {}", subject_name.as_str());

    sub.with_handler(move |msg| {
        debug!("{}", msg);
        match serde_json::from_slice::<Request>(msg.data.as_slice()) {
            Ok(req) => {
                tx.send(match task_selector {
                    TaskSelector::Infrastructure(_) => Box::new(InfrastructureTask::new(req)),
                    TaskSelector::Environment(_) => Box::new(EnvironmentTask::new(req)),
                });
                msg.respond(Response::new(None).as_json_string());
            }
            Err(err) => {
                error!(
                    "receiving request but JSON decoding error occurred: {:?}",
                    err
                );
                error!("{}", msg);
                msg.respond(Response::new(Some(err.to_string())).as_json_string());
            }
        };

        Ok(())
    });

    Ok(())
}

pub fn main() -> Result<(), Error> {
    env_logger::init();

    let customer = env::var("CUSTOMER");
    let cloud_provider = env::var("CLOUD_PROVIDER");
    let region = env::var("REGION");
    let nats_server = env::var("NATS_SERVER").expect("NATS_SERVER is mandatory");

    info!(
        "running from current directory: {}",
        env::current_dir().unwrap().to_str().unwrap()
    );

    let mode = if customer.is_ok() && cloud_provider.is_ok() && region.is_ok() {
        let c = customer.unwrap();
        let cp = cloud_provider.unwrap();
        let r = region.unwrap();

        info!("starting in cloud mode");
        info!("customer: {}", c.as_str());
        info!("cloud provider: {}", cp.as_str());
        info!("region: {}", r.as_str());
        Mode::Cloud(c, cp, r)
    } else {
        info!("starting in local mode");
        Mode::Local
    };

    info!("NATS server: {}", nats_server.as_str());

    let name = match &mode {
        Mode::Local => "qovery-engine-app.local".to_string(),
        Mode::Cloud(customer, cloud_provider, region) => format!(
            "qovery-engine-app.{}.{}.{}",
            customer, cloud_provider, region
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
    thread::spawn(move || {
        let mut task_manager = TaskManager::new();
        let _ = task_manager.run();

        thread::spawn(move || loop {
            // TODO send back the message to a topic: E.g core.task.status
            // json: {"status": "Failed", "message": "blablabla", "id": "abc", "created_at": "<datetime>"}

            //let msg = rx_task_status.recv().unwrap();
            //let subject_name = subject_name(&mode, Infrastructure(""));
            //nc_1.request(subject_name.as_str(), msg.unwrap())
        });

        loop {
            let task = rx_task.recv().unwrap();
            task_manager.add_task(task);
        }
    });

    listen_for_events(
        Infrastructure("infrastructure"),
        &nc,
        &mode,
        tx_task.clone(),
    )?;

    listen_for_events(Environment("environment"), &nc, &mode, tx_task.clone())?;

    let (tx_quit, rx_quit) = unbounded::<bool>();

    info!("server started and listening for incoming requests");
    let _ = rx_quit.recv();
    // if released then quit

    Ok(())
}
