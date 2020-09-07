use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::models::Context;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(
        context.clone(),
        "my-local-docker-id-123",
        "my-default-local-docker",
    )
}

pub fn init() {
    println!(
        "running from current directory: {}",
        std::env::current_dir().unwrap().to_str().unwrap()
    );

    env_logger::init();
}

pub fn generate_id() -> String {
    // Should follow DNS naming convention https://tools.ietf.org/html/rfc1035
    let uuid;

    loop {
        let rand_string: String = thread_rng().sample_iter(Alphanumeric).take(15).collect();
        if rand_string.chars().next().unwrap().is_alphabetic() {
            uuid = rand_string.to_lowercase();
            break;
        }
    }

    uuid
}
