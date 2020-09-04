use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::models::Context;

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(
        context.clone(),
        "my-local-docker-id-123",
        "my-default-local-docker",
    )
}
