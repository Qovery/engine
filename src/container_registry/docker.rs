use crate::cmd;
use crate::cmd::command::QoveryCommand;
use crate::container_registry::Kind;
use crate::error::{SimpleError, SimpleErrorKind};
use chrono::Duration;
use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::OperationResult;

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerImageManifest {
    pub schema_version: i64,
    pub media_type: String,
    pub config: Config,
    pub layers: Vec<Layer>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub media_type: String,
    pub size: i64,
    pub digest: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub media_type: String,
    pub size: i64,
    pub digest: String,
}

pub fn docker_manifest_inspect(
    container_registry_kind: Kind,
    docker_envs: Vec<(&str, &str)>,
    image_name: String,
    image_tag: String,
    registry_url: String,
) -> Option<DockerImageManifest> {
    let image_with_tag = format!("{}:{}", image_name, image_tag);
    let registry_provider = match container_registry_kind {
        Kind::DockerHub => "DockerHub",
        Kind::Ecr => "AWS ECR",
        Kind::Docr => "DigitalOcean Registry",
        Kind::ScalewayCr => "Scaleway Registry",
    };

    // Note: `docker manifest inspect` is still experimental for the time being:
    // https://docs.docker.com/engine/reference/commandline/manifest_inspect/
    let mut envs = docker_envs.clone();
    envs.push(("DOCKER_CLI_EXPERIMENTAL", "enabled"));

    let binary = "docker";
    let image_full_url = format!("{}/{}", registry_url.as_str(), &image_with_tag);
    let args = vec!["manifest", "inspect", image_full_url.as_str()];
    let mut raw_output: Vec<String> = vec![];

    let mut cmd = QoveryCommand::new("docker", &args, &envs);
    return match cmd.exec_with_timeout(Duration::minutes(1), |line| raw_output.push(line), |_| {}) {
        Ok(_) => {
            let joined = raw_output.join("");
            match serde_json::from_str(&joined) {
                Ok(extracted_manifest) => Some(extracted_manifest),
                Err(e) => {
                    error!(
                        "error while trying to deserialize manifest image manifest for image {} in {} ({}): {:?}",
                        image_with_tag, registry_provider, registry_url, e,
                    );
                    None
                }
            }
        }
        Err(e) => {
            error!(
                "error while trying to inspect image manifest for image {} in {} ({}), command `{}`: {:?}",
                image_with_tag,
                registry_provider,
                registry_url,
                cmd::command::command_to_string(binary, &args, &envs),
                e,
            );
            None
        }
    };
}

pub fn docker_login(
    container_registry_kind: Kind,
    docker_envs: Vec<(&str, &str)>,
    registry_login: String,
    registry_pass: String,
    registry_url: String,
) -> Result<(), SimpleError> {
    let registry_provider = match container_registry_kind {
        Kind::DockerHub => "DockerHub",
        Kind::Ecr => "AWS ECR",
        Kind::Docr => "DigitalOcean Registry",
        Kind::ScalewayCr => "Scaleway Registry",
    };

    let binary = "docker";
    let args = vec![
        "login",
        registry_url.as_str(),
        "-u",
        registry_login.as_str(),
        "-p",
        registry_pass.as_str(),
    ];

    let mut cmd = QoveryCommand::new(binary, &args, &docker_envs);
    match cmd.exec() {
        Ok(_) => Ok(()),
        Err(e) => {
            let error_message = format!(
                "error while trying to login to registry {} {}, command `{}`: {:?}",
                registry_provider,
                registry_url,
                cmd::command::command_to_string(binary, &args, &docker_envs),
                e,
            );
            error!("{}", error_message);

            Err(SimpleError::new(SimpleErrorKind::Other, Some(error_message)))
        }
    }
}

pub fn docker_tag_and_push_image(
    container_registry_kind: Kind,
    docker_envs: Vec<(&str, &str)>,
    image_name: String,
    image_tag: String,
    dest: String,
) -> Result<(), SimpleError> {
    let image_with_tag = format!("{}:{}", image_name, image_tag);
    let registry_provider = match container_registry_kind {
        Kind::DockerHub => "DockerHub",
        Kind::Ecr => "AWS ECR",
        Kind::Docr => "DigitalOcean Registry",
        Kind::ScalewayCr => "Scaleway Registry",
    };

    let mut cmd = QoveryCommand::new("docker", &vec!["tag", &image_with_tag, dest.as_str()], &docker_envs);
    match retry::retry(Fibonacci::from_millis(3000).take(5), || match cmd.exec() {
        Ok(_) => OperationResult::Ok(()),
        Err(e) => {
            info!("failed to tag image {}, retrying...", image_with_tag);
            OperationResult::Retry(e)
        }
    }) {
        Err(Operation { error, .. }) => {
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(format!("failed to tag image {}: {:?}", image_with_tag, error)),
            ))
        }
        _ => {}
    }

    let mut cmd = QoveryCommand::new("docker", &vec!["push", dest.as_str()], &docker_envs);
    match retry::retry(Fibonacci::from_millis(5000).take(5), || {
        match cmd.exec_with_timeout(
            Duration::minutes(10),
            |line| info!("{}", line),
            |line| error!("{}", line),
        ) {
            Ok(_) => OperationResult::Ok(()),
            Err(e) => {
                warn!(
                    "failed to push image {} on {}, {:?} retrying...",
                    image_with_tag, registry_provider, e
                );
                OperationResult::Retry(e)
            }
        }
    }) {
        Err(Operation { error, .. }) => Err(SimpleError::new(SimpleErrorKind::Other, Some(error.to_string()))),
        Err(e) => Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some(format!(
                "unknown error while trying to push image {} to {}. {:?}",
                image_with_tag, registry_provider, e
            )),
        )),
        _ => {
            info!("image {} has successfully been pushed", image_with_tag);
            Ok(())
        }
    }
}

pub fn docker_pull_image(
    container_registry_kind: Kind,
    docker_envs: Vec<(&str, &str)>,
    dest: String,
) -> Result<(), SimpleError> {
    let registry_provider = match container_registry_kind {
        Kind::DockerHub => "DockerHub",
        Kind::Ecr => "AWS ECR",
        Kind::Docr => "DigitalOcean Registry",
        Kind::ScalewayCr => "Scaleway Registry",
    };

    let mut cmd = QoveryCommand::new("docker", &vec!["pull", dest.as_str()], &docker_envs);
    match retry::retry(Fibonacci::from_millis(5000).take(5), || {
        match cmd.exec_with_timeout(
            Duration::minutes(10),
            |line| info!("{}", line),
            |line| error!("{}", line),
        ) {
            Ok(_) => OperationResult::Ok(()),
            Err(e) => {
                warn!(
                    "failed to pull image from {} registry {}, {:?} retrying...",
                    registry_provider,
                    dest.as_str(),
                    e,
                );
                OperationResult::Retry(e)
            }
        }
    }) {
        Err(Operation { error, .. }) => Err(SimpleError::new(SimpleErrorKind::Other, Some(error.to_string()))),
        Err(e) => Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some(format!(
                "unknown error while trying to pull image {} from {} registry. {:?}",
                dest.as_str(),
                registry_provider,
                e,
            )),
        )),
        _ => {
            info!(
                "image {} has successfully been pulled from {} registry",
                dest.as_str(),
                registry_provider,
            );
            Ok(())
        }
    }
}
