use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::models::Context;

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
