use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::models::Context;

fn main() {
    let context = Context::new("unique-id", "/tmp/qovery-workspace", "lib", None, None);

    // build image with Docker
    let local_docker = LocalDocker::new(context, "local-docker", "local-docker-name");

    // use ECR as Container Registry

    // use cloudflare as DNS provider

    // use Kubernetes 1.16

    // use AWS
}
