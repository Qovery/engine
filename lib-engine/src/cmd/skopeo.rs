use crate::cmd::command::{CommandError, CommandKiller, ExecutableCommand, QoveryCommand};
use crate::cmd::docker::ContainerImage;

use std::process::ExitStatus;

use serde_derive::Deserialize;
use std::time::Duration;

#[derive(thiserror::Error, Debug)]
pub enum SkopeoError {
    #[error("Skopeo terminated with a non success exit status code: {exit_status:?}")]
    ExitStatusError { exit_status: ExitStatus },

    #[error("Skopeo terminated with an unknown error: {raw_error:?}")]
    ExecutionError { raw_error: std::io::Error },

    #[error("Skopeo aborted due to user cancel request: {raw_error_message:?}")]
    Aborted { raw_error_message: String },

    #[error("Skopeo command terminated due to timeout: {raw_error_message:?}")]
    Timeout { raw_error_message: String },
}

impl SkopeoError {
    pub fn is_aborted(&self) -> bool {
        matches!(self, Self::Aborted { .. })
    }
}

#[derive(Debug)]
pub struct Skopeo {
    common_envs: Vec<(String, String)>,
}

impl Skopeo {
    pub fn new() -> Result<Self, SkopeoError> {
        Ok(Self { common_envs: vec![] })
    }

    pub fn delete_image(&self, image: &ContainerImage, tls_verify: bool) -> Result<(), SkopeoError> {
        let uri = format!("docker://{}", image.image_name());
        info!("Deleting image {}", uri);
        let tls = format!("--tls-verify={}", tls_verify);

        let args = &["delete", &tls, &uri];
        skopeo_exec(
            args,
            &self.get_all_envs(&[]),
            &mut |line| info!("{}", line),
            &mut |line| info!("{}", line),
            &CommandKiller::never(),
        )
    }

    pub fn list_tags(&self, image: &ContainerImage, tls_verify: bool) -> Result<Vec<String>, SkopeoError> {
        let uri = format!("docker://{}", image.repository_with_host());
        info!("listing image tags {}", uri);

        let tls = format!("--tls-verify={}", tls_verify);
        let args = &["list-tags", &tls, &uri];
        let mut output: Vec<String> = vec![];
        skopeo_exec(
            args,
            &self.get_all_envs(&[]),
            &mut |line| {
                info!("{}", line);
                output.push(line);
            },
            &mut |line| info!("{}", line),
            &CommandKiller::never(),
        )?;

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct JsonOutput {
            //repository: String,
            tags: Vec<String>,
        }

        let output: JsonOutput =
            serde_json::from_str(&output.join("\n")).map_err(|err| SkopeoError::ExecutionError {
                raw_error: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid JSON output: {:?} {}", err, output.join("\n")),
                ),
            })?;

        Ok(output.tags)
    }

    fn get_all_envs<'a>(&'a self, envs: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all_envs: Vec<(&str, &str)> = self.common_envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all_envs.append(&mut envs.to_vec());

        all_envs
    }
}

fn skopeo_exec<F, X>(
    args: &[&str],
    envs: &[(&str, &str)],
    stdout_output: &mut F,
    stderr_output: &mut X,
    cmd_killer: &CommandKiller,
) -> Result<(), SkopeoError>
where
    F: FnMut(String),
    X: FnMut(String),
{
    let mut cmd = QoveryCommand::new("skopeo", args, envs);
    cmd.set_kill_grace_period(Duration::from_secs(0));
    let ret = cmd.exec_with_abort(stdout_output, stderr_output, cmd_killer);

    match ret {
        Ok(_) => Ok(()),
        Err(CommandError::TimeoutError(msg)) => Err(SkopeoError::Timeout { raw_error_message: msg }),
        Err(CommandError::Killed(msg)) => Err(SkopeoError::Aborted { raw_error_message: msg }),
        Err(CommandError::ExitStatusError(err)) => Err(SkopeoError::ExitStatusError { exit_status: err }),
        Err(CommandError::ExecutionError(err)) => Err(SkopeoError::ExecutionError { raw_error: err }),
    }
}

// start a local registry to run this test
// docker run --rm -ti -p 5000:5000 --name registry registry:2
#[cfg(feature = "test-local-docker")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::docker::{ContainerImage, Docker};
    use url::Url;

    fn private_registry_url() -> Url {
        Url::parse("http://localhost:5000").unwrap()
    }

    #[test]
    fn test_delete_image() {
        let docker = Docker::new_with_local_builder(None).unwrap();
        let image_source = ContainerImage::new(
            Url::parse("https://public.ecr.aws").unwrap(),
            "r3m4q3r9/pub-mirror-debian".to_string(),
            vec!["11.6-ci".to_string()],
        );
        let image_dest =
            ContainerImage::new(private_registry_url(), "skopeo/alpine1".to_string(), vec!["mirror".to_string()]);

        // It should work
        let ret = docker.mirror(
            &image_source,
            &image_dest,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());

        // image should exist
        let ret = docker.pull(
            &image_dest,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());

        // now delete it
        let skopeo = Skopeo::new().unwrap();
        let ret = skopeo.delete_image(&image_dest, false);
        assert!(ret.is_ok());

        // Should not be present anymore
        let ret = docker.pull(
            &image_dest,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_err());
    }

    #[test]
    fn test_list_tags() {
        let docker = Docker::new_with_local_builder(None).unwrap();
        let image_source = ContainerImage::new(
            Url::parse("https://public.ecr.aws").unwrap(),
            "r3m4q3r9/pub-mirror-debian".to_string(),
            vec!["11.6-ci".to_string()],
        );
        let image_dest =
            ContainerImage::new(private_registry_url(), "skopeo/alpine2".to_string(), vec!["mirror".to_string()]);

        // It should work
        let ret = docker.mirror(
            &image_source,
            &image_dest,
            &mut |msg| println!("{msg}"),
            &mut |msg| eprintln!("{msg}"),
            &CommandKiller::never(),
        );
        assert!(ret.is_ok());

        let skopeo = Skopeo::new().unwrap();
        let ret = skopeo.list_tags(&image_dest, false);
        assert!(ret.is_ok());
        let ret = ret.unwrap();
        assert_eq!(ret.len(), 1);
        assert_eq!(ret[0], "mirror");
    }
}
