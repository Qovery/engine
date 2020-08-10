#[macro_use]
extern crate log;

use std::borrow::Borrow;
use std::fs::File;
use std::io::{Error, Read};

use crossbeam_channel::unbounded;

use qovery_engine_shared::{subject, Mode};
use qovery_engine_task_manager::models::{Request, Response};
use qovery_engine_task_manager::task_manager::{Task, TaskManager};
use qovery_engine_task_manager::tasks::InfrastructureTask;

fn send_nats_request(json_file_path: &str, subject: &str) -> Result<(), Error> {
    let nc = nats::Options::new()
        .with_name("test-client-rust")
        .connect("panic.qovery.com:4242")?;

    let mut create_cluster_file = File::open(json_file_path).unwrap();

    let mut buff = String::new();
    create_cluster_file.read_to_string(&mut buff).unwrap();

    nc.request(subject, buff.as_bytes());

    Ok(())
}

#[test]
fn create_infrastructure() -> Result<(), Error> {
    send_nats_request(
        "tests/assets/create-infrastructure.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.infrastructure",
    )?;

    Ok(())
}

#[test]
fn create_environment() -> Result<(), Error> {
    send_nats_request(
        "tests/assets/create-environment.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.environment",
    )?;

    Ok(())
}

#[test]
fn create_non_working_environment() -> Result<(), Error> {
    send_nats_request(
        "tests/assets/create-non-working-environment.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.environment",
    )?;

    Ok(())
}
