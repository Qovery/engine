#[cfg(feature = "test-functional")]
fn send_nats_request(json_file_path: &str, subject: &str) -> Result<(), Error> {
    let nc = nats::Options::new()
        .with_name("test-client-rust")
        .connect("panic.qovery.com:4242")?;

    let mut create_cluster_file = File::open(json_file_path).unwrap();

    let mut buff = String::new();
    create_cluster_file.read_to_string(&mut buff).unwrap();

    nc.request(subject, buff.as_bytes()).unwrap();

    Ok(())
}

#[cfg(feature = "test-functional")]
#[test]
fn create_infrastructure() -> Result<(), Error> {
    send_nats_request(
        "tests/assets/create-infrastructure.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.infrastructure",
    )?;

    Ok(())
}

#[cfg(feature = "test-functional")]
#[test]
fn create_qovery_infrastructure() -> Result<(), Error> {
    send_nats_request(
        "tests/assets/create-qovery-infrastructure.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.infrastructure",
    )?;

    Ok(())
}

#[cfg(feature = "test-functional")]
#[test]
fn create_environment() -> Result<(), Error> {
    send_nats_request(
        "tests/assets/create-environment.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.environment",
    )?;

    Ok(())
}

#[cfg(feature = "test-functional")]
#[test]
fn create_non_working_environment() -> Result<(), Error> {
    send_nats_request(
        "tests/assets/create-non-working-environment.json",
        "engine.cloud.adwopakdpo221.aws.us-east-2.environment",
    )?;

    Ok(())
}
