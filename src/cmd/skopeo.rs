use crate::cmd::command::{CommandError, CommandKiller, ExecutableCommand, QoveryCommand};
use crate::cmd::docker::ContainerImage;
use std::collections::HashSet;

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
    credentials: Option<(String, String)>,
    common_envs: Vec<(String, String)>,
}

impl Skopeo {
    pub fn new(credentials: Option<(String, String)>) -> Result<Self, SkopeoError> {
        Ok(Self {
            credentials,
            common_envs: vec![
            //("HTTPS_PROXY".to_string(), "http://localhost:8080".to_string())
            ],
        })
    }

    pub fn delete_image(&self, image: &ContainerImage, tls_verify: bool) -> Result<(), SkopeoError> {
        let uri = format!("docker://{}", image.image_name());
        info!("Deleting image {}", uri);
        let tls = format!("--tls-verify={}", tls_verify);
        let creds = if let Some((user, pass)) = &self.credentials {
            format!("--creds={}:{}", user, pass)
        } else {
            "--no-creds".to_string()
        };

        let args = &["delete", &tls, &creds, "--retry-times=5", &uri];
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
        let creds = if let Some((user, pass)) = &self.credentials {
            format!("--creds={}:{}", user, pass)
        } else {
            "--no-creds".to_string()
        };
        let args = &["list-tags", &tls, &creds, "--retry-times=5", &uri];
        let mut output: Vec<String> = vec![];
        skopeo_exec(
            args,
            &self.get_all_envs(&[]),
            &mut |line| {
                info!("{}", line);
                output.push(line);
            },
            &mut |line| info!("{}", line),
            &CommandKiller::from_timeout(Duration::from_secs(30)),
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

    /// List all digests of an image with format `sha256:d35dfc2fe3ef66bcc085ca00d3152b482e6cafb23cdda1864154caf3b19094ba`
    pub fn list_digests(&self, image: &ContainerImage, tls_verify: bool) -> Result<HashSet<String>, SkopeoError> {
        let uri = format!("docker://{}", image.image_name());
        info!("listing digest of image {}", uri);

        let tls = format!("--tls-verify={}", tls_verify);
        let creds = if let Some((user, pass)) = &self.credentials {
            format!("--creds={}:{}", user, pass)
        } else {
            "--no-creds".to_string()
        };

        // We need --raw because else skopeo is only returning the digest for the current arch and not of the whole image tag
        // https://github.com/containers/skopeo/issues/1345
        let args = &["inspect", &tls, &creds, "--retry-times=5", "--raw", &uri];
        let mut output: Vec<String> = vec![];
        skopeo_exec(
            args,
            &self.get_all_envs(&[]),
            &mut |line| {
                info!("{}", line);
                output.push(line);
            },
            &mut |line| info!("{}", line),
            &CommandKiller::from_timeout(Duration::from_secs(30)),
        )?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct JsonOutput {
            //repository: String,
            manifests: Vec<Digest>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Digest {
            digest: String,
        }

        let json: JsonOutput = serde_json::from_str(&output.join("\n")).map_err(|err| SkopeoError::ExecutionError {
            raw_error: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid JSON output: {:?} {}", err, output.join("\n")),
            ),
        })?;

        let mut digests: HashSet<String> = json.manifests.into_iter().map(|m| m.digest).collect();

        // We need to do it again with --format='{{ .Digest }}' because --raw does not return the digest of the tag
        // in case of a multi-arch image
        let args = &[
            "inspect",
            &tls,
            &creds,
            "--retry-times=5",
            "--format={{ .Digest }}",
            &uri,
        ];
        output.clear();
        skopeo_exec(
            args,
            &self.get_all_envs(&[]),
            &mut |line| {
                info!("{}", line);
                output.push(line);
            },
            &mut |line| info!("{}", line),
            &CommandKiller::from_timeout(Duration::from_secs(30)),
        )?;

        digests.insert(output.remove(0));
        Ok(digests)
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
        let skopeo = Skopeo::new(None).unwrap();
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

        let skopeo = Skopeo::new(None).unwrap();
        let ret = skopeo.list_tags(&image_dest, false);
        assert!(ret.is_ok());
        let ret = ret.unwrap();
        assert_eq!(ret.len(), 1);
        assert_eq!(ret[0], "mirror");
    }

    #[test]
    fn test_list_digests() {
        let image = ContainerImage::new(
            Url::parse("https://public.ecr.aws").unwrap(),
            "r3m4q3r9/qovery-ci".to_string(),
            vec!["pause-3.10".to_string()],
        );

        let skopeo = Skopeo::new(None).unwrap();
        let ret = skopeo.list_digests(&image, true);
        assert!(ret.is_ok());
        let ret = ret.unwrap();
        assert!(ret.len() >= 3);
        assert!(ret.iter().all(|d| d.starts_with("sha256:")));
    }
}
