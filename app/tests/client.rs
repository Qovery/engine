use nats::Connection;
use std::fs::File;
use std::io::Read;

#[allow(dead_code)]
fn send_nats_request(json_file_path: &str, subject: &str) -> Result<(), ()> {
    let nc = match nats::Options::new()
        .with_name("test-client-rust")
        .connect("panic.qovery.com:4242")
    {
        Ok(connection) => connection,
        Err(_) => return Err(()),
    };

    let mut create_cluster_file = match File::open(json_file_path) {
        Ok(file) => file,
        Err(_) => return Err(()),
    };

    let mut buff = String::new();
    create_cluster_file.read_to_string(&mut buff);

    match nc.request(subject, buff.as_bytes()) {
        Ok(_) => Ok(()),
        Err(_) => Err(()),
    }
}

#[cfg(feature = "test-functional")]
#[test]
fn create_infrastructure() {
    assert!(send_nats_request(
        "tests/assets/create-infrastructure.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.infrastructure",
    )
    .is_ok());
}

#[cfg(feature = "test-functional")]
#[test]
fn create_qovery_infrastructure() {
    assert!(send_nats_request(
        "tests/assets/create-qovery-infrastructure.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.infrastructure",
    )
    .is_ok());
}

#[cfg(feature = "test-functional")]
#[test]
fn create_environment() {
    assert!(send_nats_request(
        "tests/assets/create-environment.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.environment",
    )
    .is_ok());
}

#[cfg(feature = "test-functional")]
#[test]
fn create_non_working_environment() {
    assert!(send_nats_request(
        "tests/assets/create-non-working-environment.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.environment",
    )
    .is_ok());
}
