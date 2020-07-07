#[macro_use]
extern crate log;
use std::borrow::Borrow;
use std::fs::File;
use std::io::{Error, Read};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use crossbeam_channel::unbounded;

use qovery_engine_shared::{subject, Mode};
use qovery_engine_task_manager::models::{Request, Response};
use qovery_engine_task_manager::task_manager::{Task, TaskManager};
use qovery_engine_task_manager::tasks::CreateInfrastructureTask;

#[test]
fn create_infrastructure() -> Result<(), Error> {
    env_logger::init();

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

    let (tx_task, rx_task) = unbounded::<Box<dyn Task>>();
    thread::spawn(move || {
        let mut task_manager = TaskManager::new();
        task_manager.run();

        loop {
            let task = rx_task.recv().unwrap();
            task_manager.add_task(task);
        }
    });

    sub.with_handler(move |msg| {
        debug!("{}", msg);
        match serde_json::from_slice::<Request>(msg.data.as_slice()) {
            Ok(req) => {
                tx_task.send(Box::new(CreateInfrastructureTask::new(req)));
                msg.respond(Response::new(None).as_json_string());
            }
            Err(err) => {
                msg.respond(Response::new(Some(err.to_string())).as_json_string());
            }
        };

        Ok(())
    });

    let mut create_cluster_file = File::open("tests/assets/create-infrastructure.json").unwrap();

    let mut buff = String::new();
    create_cluster_file.read_to_string(&mut buff).unwrap();

    loop {
        nc.request(create_infrastructure_subject.as_str(), buff.as_bytes());
        sleep(Duration::from_secs(30));
    }
}
