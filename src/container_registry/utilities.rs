use crate::cmd;
use crate::container_registry::Kind;
use crate::error::{SimpleError, SimpleErrorKind};
use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::OperationResult;

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
    };

    match retry::retry(Fibonacci::from_millis(3000).take(5), || {
        match cmd::utilities::exec("docker", vec!["tag", &image_with_tag, dest.as_str()], &docker_envs) {
            Ok(_) => OperationResult::Ok(()),
            Err(e) => {
                info!("failed to tag image {}, retrying...", image_with_tag);
                OperationResult::Retry(e)
            }
        }
    }) {
        Err(Operation { error, .. }) => {
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(format!("failed to tag image {}: {:?}", image_with_tag, error.message)),
            ))
        }
        _ => {}
    }

    match retry::retry(Fibonacci::from_millis(5000).take(5), || {
        match cmd::utilities::exec("docker", vec!["push", dest.as_str()], &docker_envs) {
            Ok(_) => OperationResult::Ok(()),
            Err(e) => {
                warn!(
                    "failed to push image {} on {}, {:?} retrying...",
                    image_with_tag, registry_provider, e.message
                );
                OperationResult::Retry(e)
            }
        }
    }) {
        Err(Operation { error, .. }) => Err(error),
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
