mod models;
mod task_manager;
mod tasks;

#[macro_use]
extern crate serde;

use crate::models::{Request, Response};
use crate::task_manager::{Task, TaskManager};
use crate::tasks::CreateInfrastructureTask;
use crossbeam_channel::unbounded;
use std::borrow::Borrow;
use std::fs::File;
use std::io::{Error, Read};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

fn subject<'a>(mode: &'a Mode, subject: &'a str) -> String {
    match mode {
        Mode::Local => format!("engine.local.{}", subject),
        Mode::Cloud(cloud_provider, region, customer) => format!(
            "engine.cloud.{}.{}.{}.{}",
            customer, cloud_provider, region, subject
        ),
    }
}

type CloudProvider<'a> = &'a str;
type Region<'a> = &'a str;
type Customer<'a> = &'a str;

enum Mode<'a> {
    Local,
    Cloud(Customer<'a>, CloudProvider<'a>, Region<'a>),
}

fn main() -> Result<(), Error> {
    let name = "qovery-engine-app";
    let mode = Mode::Cloud("a1cd1w2xkw", "aws", "us-east-2");

    let nc = nats::Options::new()
        .with_name(name)
        .connect("localhost:4222")?;

    let create_infrastructure_subject = subject(&mode, "create-infrastructure");
    let sub = nc.queue_subscribe(
        create_infrastructure_subject.as_str(),
        create_infrastructure_subject.as_str(),
    )?;

    let (tx, rx) = unbounded::<Box<dyn Task>>();
    thread::spawn(move || {
        let mut task_manager = TaskManager::new();
        task_manager.run();

        loop {
            let task = rx.recv().unwrap();
            task_manager.add_task(task);
        }
    });

    sub.with_handler(move |msg| {
        println!("{}", msg);
        match serde_json::from_slice::<Request>(msg.data.as_slice()) {
            Ok(req) => {
                tx.send(Box::new(CreateInfrastructureTask::new(req)));
                msg.respond(Response::new(None).as_json_string());
            }
            Err(err) => {
                msg.respond(Response::new(Some(err.to_string())).as_json_string());
            }
        };

        Ok(())
    });

    // TODO remove - for testing purpose
    let mut create_cluster_file =
        File::open("qovery-engine-app/tests/assets/create-infrastructure.json").unwrap();

    let mut buff = String::new();
    create_cluster_file.read_to_string(&mut buff).unwrap();

    loop {
        nc.request(create_infrastructure_subject.as_str(), buff.as_bytes());
        sleep(Duration::from_secs(30));
    }

    Ok(())
}
