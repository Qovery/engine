use super::config::OperatorIdentity;
use super::model::{ContainerInspect, ContainerStatus, ImageInspect};
use std::fs;
use uuid::Uuid;

pub const CLUSTER_ID: &str = "0c6a3b8e-1f0e-4b0a-9a51-6b1f8f7a2c11";
pub const ORGANIZATION_ID: &str = "5e2f1d0c-7a4b-4c3d-8e9f-0a1b2c3d4e5f";

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/operator_container_update/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read {path}: {err}"))
}

pub fn container(name: &str) -> ContainerInspect {
    let mut containers: Vec<ContainerInspect> = serde_json::from_str(&fixture(name)).unwrap();
    containers.remove(0)
}

pub fn running(name: &str) -> ContainerInspect {
    let mut container = container(name);
    container.state.status = ContainerStatus::Running;
    container
}

pub fn image() -> ImageInspect {
    let mut images: Vec<ImageInspect> = serde_json::from_str(&fixture("image_n")).unwrap();
    images.remove(0)
}

pub fn identity() -> OperatorIdentity {
    OperatorIdentity {
        cluster_id: Uuid::parse_str(CLUSTER_ID).unwrap(),
        organization_id: Uuid::parse_str(ORGANIZATION_ID).unwrap(),
    }
}

/// The environment of the README install, as written in `operator.env`.
pub fn readme_env() -> Vec<String> {
    [
        "QOVERY_OPERATOR_RUNTIME=DOCKER",
        &format!("ORGANIZATION_ID={ORGANIZATION_ID}"),
        &format!("CLUSTER_ID={CLUSTER_ID}"),
        "CLUSTER_JWT_TOKEN=fixture-jwt",
        "GRPC_SERVER=https://engine.qovery.com:443",
        "QOVERY_ENGINE_WORKER_IMAGE_TAG_SUFFIX=-eksanywhere",
        "QOVERY_WORKER_DEPLOYMENT_TYPES=INFRASTRUCTURE",
        "MAX_CONCURRENT_WORKER_JOBS=1",
        "DOCKER_SOCKET_PATH=/var/run/docker.sock",
        "QOVERY_BASTION_WORKSPACE_HOST_PATH=/qovery/workspace",
        "RUST_LOG=info",
    ]
    .map(str::to_string)
    .to_vec()
}

pub fn readme() -> ContainerInspect {
    container("container_readme")
}

/// Sets `entry` the way `docker run -e` does: it replaces a variable of the same name, image
/// defaults included, and otherwise goes before the image defaults.
pub fn with_env(mut container: ContainerInspect, entry: &str) -> ContainerInspect {
    let name = entry.split('=').next().unwrap_or_default();
    let customer_entries = readme_env().len();
    let entries = &mut container.config.env.0;
    match entries
        .iter()
        .position(|existing| existing.split('=').next() == Some(name))
    {
        Some(index) if index < customer_entries => entries[index] = entry.to_string(),
        Some(index) => {
            entries.remove(index);
            entries.insert(customer_entries, entry.to_string());
        }
        None => entries.insert(customer_entries, entry.to_string()),
    }
    container
}

pub fn extra_env() -> ContainerInspect {
    with_env(readme(), "HTTPS_PROXY=http://proxy.internal:3128")
}

pub fn override_equal() -> ContainerInspect {
    with_env(readme(), "QOVERY_FIXTURE_DEFAULT=image-n-value")
}

pub fn override_differs() -> ContainerInspect {
    with_env(readme(), "QOVERY_FIXTURE_DEFAULT=customer-value")
}

pub fn rootless() -> ContainerInspect {
    let mut container = with_env(readme(), "DOCKER_SOCKET_HOST_PATH=/run/user/1000/docker.sock");
    container.host_config.binds = vec!["/run/user/1000/docker.sock:/var/run/docker.sock".to_string()];
    container
}

pub fn previous_update() -> ContainerInspect {
    let mut container = readme();
    container.config.labels.extend(
        [
            ("com.example.team", "platform"),
            ("qovery.com/operator-update", "old-exec"),
            ("qovery.com/operator-previous", "0123456789ab"),
            ("qovery.com/operator-restart-policy", "always"),
        ]
        .map(|(key, value)| (key.to_string(), value.to_string())),
    );
    container
}
