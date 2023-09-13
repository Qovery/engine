use crate::build_platform::SshKey;
use crate::cloud_provider::helm::HelmChartError;
use crate::cloud_provider::service::{Action, Service};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::command::CommandKiller;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::helm_chart::reporter::HelmChartDeploymentReporter;
use crate::deployment_report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::deployment_report::{execute_long_deployment, DeploymentTaskImpl};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::git;
use crate::io_models::application::GitCredentials;
use crate::models::helm_chart::{HelmChart, HelmChartSource, HelmValueSource};
use crate::models::types::CloudProvider;
use anyhow::anyhow;
use git2::{Cred, CredentialType};
use itertools::Itertools;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::Duration;

const HELM_CHART_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

impl<T: CloudProvider> DeploymentAction for HelmChart<T> {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let pre_run = |logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            prepare_helm_chart_directory(self, target, event_details.clone(), logger)?;

            // Now the chart is ready at self.chart_workspace_directory()

            // Check users does not bypass restrictions
            if !self.is_cluster_wide_ressources_allowed() {
                // * Cannot install CRDS
                // * Cannot install in another namespaces
                // * Cannot install cluster wide resources (i.e: ClusterIssuer)
            }

            Ok(())
        };

        let run = |_logger: &EnvProgressLogger, _state: ()| Ok(());
        let post_run = |_logger: &EnvSuccessLogger, _state: ()| {};

        let task = DeploymentTaskImpl {
            pre_run: &pre_run,
            run: &run,
            post_run_success: &post_run,
        };

        execute_long_deployment(HelmChartDeploymentReporter::new(self, target, Action::Create), task)
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let _event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));

        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let _event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        Ok(())
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let _event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Restart));

        Ok(())
    }
}

fn write_helm_value_with_replacement<'a>(
    lines: impl Iterator<Item = Cow<'a, str>>,
    output_file_path: &Path,
    env_vars: &HashMap<String, String>,
) -> Result<(), anyhow::Error> {
    let mut output_writer = BufWriter::new(File::create(output_file_path)?);
    let ret: Result<(), anyhow::Error> = lines
        .map(|l| replace_qovery_env_variable(l, env_vars))
        .map_ok(|l| -> Result<(), anyhow::Error> {
            output_writer.write_all(l.as_bytes())?;
            output_writer.write_all(&[b'\n'])?;
            Ok(())
        })
        .flatten_ok()
        .collect();

    ret?;
    output_writer.flush()?;

    Ok(())
}

fn replace_qovery_env_variable<'a>(
    mut line: Cow<'a, str>,
    envs: &HashMap<String, String>,
) -> Result<Cow<'a, str>, anyhow::Error> {
    const PREFIX: &str = "qovery.env.";

    // Loop until we find all occurrences on the same line
    loop {
        // If no pattern matches, exit the loop to return our result
        let Some(beg_pos) = line.find(PREFIX) else {
            break;
        };

        // Built-in variable are not allowed because they contains ID in them
        // Which we will not be able to replace during a clone. So use must set an alias or use its own vars
        if line[beg_pos + PREFIX.len()..].starts_with("QOVERY_") {
            return Err(anyhow!("You cannot use Qovery built_in variable in your helm values file. Please create and use an alias. line: {}", line));
        }

        let variable_name =
            if let Some(end_pos) = line[beg_pos..].find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.') {
                &line[beg_pos..end_pos]
            } else {
                &line[beg_pos..]
            };

        let Some(variable) = envs.get(&variable_name[PREFIX.len()..]) else {
            return Err(anyhow!(
                "Invalid variable, specified {:?} variable does not exist at line: {}",
                variable_name,
                line
            ));
        };

        line = Cow::Owned(line.replace(variable_name, variable))
    }

    Ok(line)
}

fn git_credentials_callback<'a>(
    git_credentials: &'a Option<GitCredentials>,
    ssh_keys: &'a [SshKey],
) -> impl Fn(&str) -> Vec<(CredentialType, Cred)> + 'a {
    move |user: &str| {
        let mut creds: Vec<(CredentialType, Cred)> = Vec::with_capacity(ssh_keys.len() + 1);
        for ssh_key in ssh_keys.iter() {
            let public_key = ssh_key.public_key.as_deref();
            let passphrase = ssh_key.passphrase.as_deref();
            if let Ok(cred) = Cred::ssh_key_from_memory(user, public_key, &ssh_key.private_key, passphrase) {
                creds.push((CredentialType::SSH_MEMORY, cred));
            }
        }

        if let Some(git_creds) = git_credentials {
            creds.push((
                CredentialType::USER_PASS_PLAINTEXT,
                Cred::userpass_plaintext(&git_creds.login, &git_creds.access_token).unwrap(),
            ));
        }

        creds
    }
}

// Goal is to download the chart in the workspace directory with everything ready to execute
// 1. Download the chart on disk
// 2. Copy the values files in the chart location with qovery replacements
fn prepare_helm_chart_directory<T: CloudProvider>(
    this: &HelmChart<T>,
    target: &DeploymentTarget,
    event_details: EventDetails,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>> {
    // Error Mapper.
    // There are a lot of small io error possible. We only want to return a meaningful error msg to user
    let to_error = |msg: String| -> Box<EngineError> {
        Box::new(EngineError::new_helm_chart_error(
            event_details.clone(),
            HelmChartError::CreateTemplateError {
                chart_name: this.name().to_string(),
                msg,
            },
        ))
    };

    // Prepare the chart with template folder
    match this.chart_source() {
        HelmChartSource::Repository {
            chart_name,
            chart_version,
            url: repository,
            skip_tls_verify,
            ..
        } => {
            fs::create_dir(this.chart_workspace_directory())
                .map_err(|e| to_error(format!("Cannot create destination directory for chart due to {}", e)))?;

            let url_without_password = {
                let mut url = repository.clone();
                let _ = url.set_password(None);
                url
            };
            logger.info(format!(
                "Downloading Helm chart {} at version {} from {}",
                chart_name, chart_version, url_without_password
            ));

            target
                .helm
                .download_chart(
                    repository,
                    chart_name,
                    chart_version,
                    this.chart_workspace_directory(),
                    *skip_tls_verify,
                    &[],
                    &CommandKiller::from(HELM_CHART_DOWNLOAD_TIMEOUT, target.should_abort),
                )
                .map_err(|e| (event_details.clone(), e))?;
        }
        HelmChartSource::Git {
            git_url,
            commit_id,
            root_path,
            git_credentials,
            ssh_keys,
        } => {
            logger.info(format!(
                "Cloning Helm chart from git repository {} at commit {}",
                git_url, commit_id
            ));

            let tmpdir = tempfile::tempdir_in(this.workspace_directory())
                .map_err(|e| to_error(format!("Cannot create tempdir {}", e)))?;

            git::clone_at_commit(
                git_url,
                commit_id,
                &tmpdir,
                &git_credentials_callback(git_credentials, ssh_keys),
            )
            .map_err(|e| to_error(format!("Cannot clone helm chart git repository due to {}", e)))?;

            fs::rename(&tmpdir.path().join(root_path), this.chart_workspace_directory())
                .map_err(|e| to_error(format!("Cannot move helm chart directory due to {}", e)))?;
        }
    }

    // Now we retrieve and prepare the chart values
    match this.chart_values() {
        HelmValueSource::Raw { values } => {
            for value in values {
                logger.info(format!("Preparing Helm values file {}", &value.name));

                let lines = value.content.lines().map(Cow::Borrowed);
                write_helm_value_with_replacement(
                    lines,
                    &this.chart_workspace_directory().join(&value.name),
                    this.environment_variables(),
                )
                .map_err(|e| to_error(format!("Cannot prepare helm value file {} due to {}", value.name, e)))?;
            }
        }
        HelmValueSource::Git {
            git_url,
            git_credentials,
            commit_id,
            values_path,
            ssh_keys,
        } => {
            logger.info(format!(
                "Fetching Helm values from git repository {} at commit {}",
                git_url, commit_id
            ));

            let tmpdir = tempfile::tempdir_in(this.workspace_directory())
                .map_err(|e| to_error(format!("Cannot create tempdir {}", e)))?;
            git::clone_at_commit(
                git_url,
                commit_id,
                &tmpdir,
                &git_credentials_callback(git_credentials, ssh_keys),
            )
            .map_err(|e| to_error(format!("Cannot clone helm values git repository due to {}", e)))?;

            for value in values_path {
                let Some(filename) = value.file_name() else {
                    logger.warning(format!("Invalid filename for {:?}", value));
                    continue;
                };

                logger.info(format!("Preparing Helm values file {:?}", filename));
                let input_file = File::open(tmpdir.path().join(value))
                    .map_err(|e| to_error(format!("Cannot create destination file for helm value due to {}", e)))?;

                let lines = BufReader::new(input_file)
                    .lines()
                    .map(|l| Cow::Owned(l.unwrap_or_default()));

                write_helm_value_with_replacement(
                    lines,
                    &this.chart_workspace_directory().join(filename),
                    this.environment_variables(),
                )
                .map_err(|e| to_error(format!("Cannot prepare helm value file {:?} due to {}", filename, e)))?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashmap;

    #[test]
    fn test_replace_qovery_env_variables() {
        let envs = hashmap! {
          "TOTO".to_string() => "toto_var".to_string(),
            "LABEL_NAME".to_string() => "toto_label".to_string()
        };

        let ret = replace_qovery_env_variable(Cow::Borrowed("toto: qovery.env.TOTO"), &envs);
        assert!(matches!(ret, Ok(line) if line == "toto: toto_var"));

        let ret = replace_qovery_env_variable(Cow::Borrowed("qovery.env.LABEL_NAME: qovery.env.TOTO"), &envs);
        assert!(matches!(ret, Ok(line) if line == "toto_label: toto_var"));

        let ret = replace_qovery_env_variable(Cow::Borrowed("wesh wesh"), &envs);
        assert!(matches!(ret, Ok(line) if line == "wesh wesh"));

        let ret = replace_qovery_env_variable(Cow::Borrowed("qovery.env.DO_NOT_EXIST"), &envs);
        assert!(ret.is_err());

        let ret = replace_qovery_env_variable(Cow::Borrowed("qovery.env.QOVERY_"), &envs);
        assert!(ret.is_err());
    }
}
