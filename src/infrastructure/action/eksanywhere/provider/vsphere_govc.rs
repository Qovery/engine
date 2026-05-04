use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::errors::CommandError;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use std::env;
use std::time::Duration;
use url::Url;

pub(super) const COMMAND_STDOUT_PREFIX: &str = "CMD│ ";
pub(super) const COMMAND_STDERR_PREFIX: &str = "CMD┃ ";
const GOVC_USERNAME: &str = "GOVC_USERNAME";
const GOVC_PASSWORD: &str = "GOVC_PASSWORD";
const GOVC_URL: &str = "GOVC_URL";
const GOVC_INSECURE: &str = "GOVC_INSECURE";
const GOVC_PERSIST_SESSION: &str = "GOVC_PERSIST_SESSION";
const GOVC_TLS_CERTIFICATE: &str = "GOVC_TLS_CERTIFICATE";
const GOVC_TLS_KEY: &str = "GOVC_TLS_KEY";
const VSPHERE_USER: &str = "VSPHERE_USER";
const VSPHERE_PASSWORD: &str = "VSPHERE_PASSWORD";
const HTTP_PREFIX: &str = "http://";
const HTTPS_PREFIX: &str = "https://";
const GOVC_STILL_RUNNING_MESSAGE: &str = "Command still running. No output available. Waiting for next line...";

pub(super) fn build_govc_envs(
    cloud_provider: &dyn CloudProvider,
    metadata: &super::VSphereClusterMetadata,
) -> Vec<(String, String)> {
    let mut envs: Vec<(String, String)> = cloud_provider
        .credentials_environment_variables()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    inject_govc_env_if_missing(&mut envs, GOVC_USERNAME, &[VSPHERE_USER]);
    inject_govc_env_if_missing(&mut envs, GOVC_PASSWORD, &[VSPHERE_PASSWORD]);
    inject_govc_env_if_missing(&mut envs, GOVC_URL, &[]);
    inject_govc_env_if_missing(&mut envs, GOVC_INSECURE, &[]);
    inject_govc_env_if_missing(&mut envs, GOVC_PERSIST_SESSION, &[]);

    if let Some(server) = metadata.vcenter_server.as_ref()
        && !envs.iter().any(|(k, _)| k == GOVC_URL)
    {
        let govc_url = if server.starts_with(HTTP_PREFIX) || server.starts_with(HTTPS_PREFIX) {
            server.to_string()
        } else {
            format!("{HTTPS_PREFIX}{server}")
        };
        envs.push((GOVC_URL.to_string(), govc_url));
    }

    if let Some(insecure) = metadata.insecure
        && !envs.iter().any(|(k, _)| k == GOVC_INSECURE)
    {
        envs.push((GOVC_INSECURE.to_string(), if insecure { "1" } else { "0" }.to_string()));
    }

    if !envs.iter().any(|(k, _)| k == GOVC_PERSIST_SESSION) {
        // Avoid stale govc session cache between runs that can hide recent tag changes.
        envs.push((GOVC_PERSIST_SESSION.to_string(), "false".to_string()));
    }

    envs
}

fn inject_govc_env_if_missing(envs: &mut Vec<(String, String)>, env_name: &str, fallback_env_names: &[&str]) {
    if envs.iter().any(|(k, _)| k == env_name) {
        return;
    }

    for candidate in std::iter::once(env_name).chain(fallback_env_names.iter().copied()) {
        let Ok(value) = env::var(candidate) else {
            continue;
        };
        if value.trim().is_empty() {
            continue;
        }
        envs.push((env_name.to_string(), value));
        return;
    }
}

/// Returns a non-empty, trimmed value for a GOVC env key from the prepared env list.
/// Empty or whitespace-only values are treated as missing.
pub(super) fn govc_env_value<'a>(govc_env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    govc_env
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn validate_govc_auth_envs(govc_env: &[(String, String)]) -> Result<(), CommandError> {
    let username = govc_env_value(govc_env, GOVC_USERNAME);
    let password = govc_env_value(govc_env, GOVC_PASSWORD);

    let has_user_password = match (username, password) {
        (Some(_), Some(_)) => true,
        (None, None) => false,
        _ => {
            return Err(CommandError::new_from_safe_message(
                "Incomplete vSphere credentials for govc: set both `GOVC_USERNAME` and `GOVC_PASSWORD`.".to_string(),
            ));
        }
    };
    let has_client_cert_auth =
        govc_env_value(govc_env, GOVC_TLS_CERTIFICATE).is_some() && govc_env_value(govc_env, GOVC_TLS_KEY).is_some();
    let has_url_user_info = govc_env_value(govc_env, GOVC_URL)
        .and_then(|govc_url| Url::parse(govc_url).ok())
        .map(|url| !url.username().is_empty())
        .unwrap_or(false);

    if has_user_password || has_client_cert_auth || has_url_user_info {
        return Ok(());
    }

    Err(CommandError::new_from_safe_message(
        "Missing vSphere credentials for govc preflight. Set `GOVC_USERNAME`/`GOVC_PASSWORD` (or `VSPHERE_USER`/`VSPHERE_PASSWORD`).".to_string(),
    ))
}

pub(super) fn log_govc_version(logger: &impl InfraLogger, govc_env: &[(String, String)]) {
    match run_govc_command(&["version"], govc_env) {
        Ok(lines) if !lines.is_empty() => logger.info(format!("Using govc: {}", lines.join(" ").trim())),
        _ => logger.warn("Unable to get `govc` version using `govc version`."),
    }
}

pub(super) fn run_govc_command(args: &[&str], govc_env: &[(String, String)]) -> Result<Vec<String>, CommandError> {
    execute_govc_command(args, govc_env, Duration::from_secs(45), |_| {}, |_| {})
}

pub(super) fn run_govc_command_with_timeout_logged(
    args: &[&str],
    govc_env: &[(String, String)],
    timeout: Duration,
    logger: &impl InfraLogger,
    label: &str,
) -> Result<Vec<String>, CommandError> {
    logger.info(format!("{COMMAND_STDOUT_PREFIX}▶️ Running `{label}`."));

    let result = execute_govc_command(
        args,
        govc_env,
        timeout,
        |trimmed| {
            if !trimmed.is_empty() {
                logger.info(format!("{COMMAND_STDOUT_PREFIX}{trimmed}"));
            }
        },
        |trimmed| {
            if trimmed == GOVC_STILL_RUNNING_MESSAGE {
                logger.info(format!("{COMMAND_STDOUT_PREFIX}⏳ `{label}` is still running..."));
            } else if !trimmed.is_empty() {
                logger.warn(format!("{COMMAND_STDERR_PREFIX}{trimmed}"));
            }
        },
    );

    match result {
        Ok(lines) => {
            logger.info(format!("{COMMAND_STDOUT_PREFIX}✅ `{label}` completed."));
            Ok(lines)
        }
        Err(error) => {
            logger.warn(format!("{COMMAND_STDERR_PREFIX}❌ `{label}` failed."));
            Err(error)
        }
    }
}

fn execute_govc_command(
    args: &[&str],
    govc_env: &[(String, String)],
    timeout: Duration,
    mut stdout_handler: impl FnMut(&str),
    mut stderr_handler: impl FnMut(&str),
) -> Result<Vec<String>, CommandError> {
    let env_pairs: Vec<(&str, &str)> = govc_env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut cmd = QoveryCommand::new("govc", args, &env_pairs);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    cmd.exec_with_abort(
        &mut |line| {
            stdout_handler(line.trim());
            stdout.push(line);
        },
        &mut |line| {
            stderr_handler(line.trim());
            stderr.push(line);
        },
        &CommandKiller::from_timeout(timeout),
    )
    .map_err(|e| {
        CommandError::new(
            format!("Cannot run `govc {}`", args.join(" ")),
            Some(super::stderr_or_error(&stderr, e.to_string())),
            None,
        )
    })?;

    Ok(stdout
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}
