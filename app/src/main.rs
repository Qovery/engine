#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;

use std::borrow::Borrow;
use std::fs::File;
use std::io::{Error, Read};
use std::thread::sleep;
use std::time::Duration;
use std::{env, thread};

use crossbeam_channel::{unbounded, Sender};
use nats::Connection;

use qovery_engine_shared::{subject, Mode};
use qovery_engine_task_manager::models::{Request, Response};
use qovery_engine_task_manager::task_manager::{Task, TaskManager};
use qovery_engine_task_manager::tasks::{ApplicationTask, InfrastructureTask};

use crate::TaskSelector::{Application, Infrastructure};

enum TaskSelector {
    Infrastructure(&'static str),
    Application(&'static str),
}

fn listen_for_events(
    task_selector: TaskSelector,
    nc: &Connection,
    mode: &Mode,
    tx: Sender<Box<dyn Task>>,
) -> Result<(), Error> {
    let subject_str = subject(
        mode,
        match task_selector {
            TaskSelector::Infrastructure(s) => s,
            TaskSelector::Application(s) => s,
        },
    );

    let sub = nc.queue_subscribe(subject_str.as_str(), subject_str.as_str())?;

    sub.with_handler(move |msg| {
        debug!("{}", msg);
        match serde_json::from_slice::<Request>(msg.data.as_slice()) {
            Ok(req) => {
                tx.send(match task_selector {
                    TaskSelector::Infrastructure(_) => Box::new(InfrastructureTask::new(req)),
                    TaskSelector::Application(_) => Box::new(ApplicationTask::new(req)),
                });
                msg.respond(Response::new(None).as_json_string());
            }
            Err(err) => {
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

    info!("nats server: {}", nats_server.as_str());

    let name = match &mode {
        Mode::Local => "qovery-engine-app.local".to_string(),
        Mode::Cloud(customer, cloud_provider, region) => format!(
            "qovery-engine-app.{}.{}.{}",
            customer, cloud_provider, region
        ),
    };

    let nc = nats::Options::new()
        .with_name(name.as_str())
        .connect(nats_server.as_str())?;

    let (tx_task, rx_task) = unbounded::<Box<dyn Task>>();
    thread::spawn(move || {
        let mut task_manager = TaskManager::new();
        task_manager.run();

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

    listen_for_events(Application("application"), &nc, &mode, tx_task.clone())?;

    let (tx_quit, rx_quit) = unbounded::<bool>();

    info!("server started and listening for incoming requests");
    let _ = rx_quit.recv();
    // if released then quit

    Ok(())
}
