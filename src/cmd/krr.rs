use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::errors::CommandError;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KrrOutputFormat {
    Table,
    Json,
    Csv,
}

impl KrrOutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            KrrOutputFormat::Table => "table",
            KrrOutputFormat::Json => "json",
            KrrOutputFormat::Csv => "csv",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KrrOptions {
    pub output_format: KrrOutputFormat,
    pub prometheus_url: Url,
    pub extra_args: Vec<String>,
    pub file_output: Option<PathBuf>,
}

#[derive(thiserror::Error, Clone, Debug, PartialEq)]
pub enum KrrError {
    #[error("Kubernetes config file path is not valid or does not exist: {kubeconfig_path}")]
    InvalidKubeConfig { kubeconfig_path: String },
    #[error("KRR command terminated with an error: {error:?}")]
    CmdError { error: CommandError },
    #[error("KRR report output file cannot be read: {path}")]
    CannotReadReport { path: String },
    #[error("KRR option `{name}` is invalid: {message}")]
    InvalidOption { name: String, message: String },
}

pub struct KrrCmd {}

impl Default for KrrCmd {
    fn default() -> Self {
        Self::new()
    }
}

impl KrrCmd {
    pub fn new() -> Self {
        Self {}
    }

    pub fn args(options: &KrrOptions) -> Result<Vec<String>, KrrError> {
        let mut args = vec![
            "simple-limit".to_string(),
            "-p".to_string(),
            options.prometheus_url.to_string(),
            "-f".to_string(),
            options.output_format.as_str().to_string(),
            "--logtostderr".to_string(),
        ];

        if let Some(file_output) = &options.file_output {
            args.push("--fileoutput".to_string());
            args.push(file_output.to_string_lossy().to_string());
        }

        args.extend(validated_extra_args(&options.extra_args)?);

        Ok(args)
    }

    pub fn run(
        &self,
        kubeconfig: &Path,
        options: &KrrOptions,
        envs: &[(&str, &str)],
        command_killer: &CommandKiller,
    ) -> Result<String, KrrError> {
        let args = Self::args(options)?;
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();

        let mut envs_owned = envs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<Vec<_>>();
        envs_owned.push(("KUBECONFIG".to_string(), kubeconfig.to_string_lossy().to_string()));
        envs_owned.push(("COLUMNS".to_string(), "200".to_string()));
        let envs_ref = envs_owned
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>();

        let mut cmd = QoveryCommand::new("krr", args_ref.as_slice(), envs_ref.as_slice());
        let mut stdout_output: Vec<String> = Vec::new();
        let mut stderr_output: Vec<String> = Vec::new();

        if let Err(error) = cmd.exec_with_abort(
            &mut |line| stdout_output.push(line),
            &mut |line| {
                debug!("krr stderr: {}", line);
                stderr_output.push(line);
            },
            command_killer,
        ) {
            return Err(KrrError::CmdError {
                error: CommandError::new(
                    "KRR command failed".to_string(),
                    Some(format!(
                        "Command `krr {}` failed with `{error}`.\nstdout:\n{}\nstderr:\n{}",
                        args.join(" "),
                        stdout_output.join("\n"),
                        stderr_output.join("\n"),
                    )),
                    Some(envs_owned),
                ),
            });
        }

        if let Some(file_output) = &options.file_output {
            return fs::read_to_string(file_output).map_err(|error| KrrError::CannotReadReport {
                path: format!("{}: {error}", file_output.display()),
            });
        }

        Ok(stdout_output.join("\n"))
    }
}

pub struct Krr {
    krr_cmd: KrrCmd,
}

impl Default for Krr {
    fn default() -> Self {
        Self::new()
    }
}

impl Krr {
    pub fn new() -> Self {
        Self { krr_cmd: KrrCmd::new() }
    }

    pub fn get_recommendations(
        &self,
        kubeconfig: &Path,
        options: &KrrOptions,
        envs: &[(&str, &str)],
        abort: &dyn crate::environment::models::abort::Abort,
    ) -> Result<String, KrrError> {
        if !kubeconfig.exists() {
            return Err(KrrError::InvalidKubeConfig {
                kubeconfig_path: kubeconfig.display().to_string(),
            });
        }

        let command_killer = CommandKiller::from(Duration::from_secs(20 * 60), abort);
        self.krr_cmd.run(kubeconfig, options, envs, &command_killer)
    }
}

fn validated_extra_args(args: &[String]) -> Result<Vec<String>, KrrError> {
    let mut validated = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].trim();
        if arg.is_empty() {
            return Err(invalid_option("cmd_args", "empty arguments are not allowed"));
        }

        let (flag, inline_value) = split_flag_value(arg);
        match canonical_bool_flag(flag) {
            Some(canonical) => {
                if inline_value.is_some() {
                    return Err(invalid_option(flag, "does not accept a value"));
                }
                validated.push(canonical.to_string());
                index += 1;
            }
            None => {
                let Some(value_kind) = canonical_value_flag(flag) else {
                    return Err(invalid_option(flag, "is not allowed"));
                };
                let (value, consumed) = match inline_value {
                    Some(value) => (value.to_string(), 1),
                    None => {
                        let next = args
                            .get(index + 1)
                            .ok_or_else(|| invalid_option(flag, "requires a value"))?;
                        (next.to_string(), 2)
                    }
                };
                validate_value(flag, &value, value_kind)?;
                validated.push(value_kind.canonical_flag().to_string());
                validated.push(value);
                index += consumed;
            }
        }
    }
    Ok(validated)
}

#[derive(Clone, Copy)]
enum KrrValueArg {
    PositiveFloat(&'static str),
    Percentile(&'static str),
    PositiveInteger(&'static str),
    NonNegativeInteger(&'static str),
    NonEmptyString(&'static str),
}

impl KrrValueArg {
    fn canonical_flag(self) -> &'static str {
        match self {
            KrrValueArg::PositiveFloat(flag)
            | KrrValueArg::Percentile(flag)
            | KrrValueArg::PositiveInteger(flag)
            | KrrValueArg::NonNegativeInteger(flag)
            | KrrValueArg::NonEmptyString(flag) => flag,
        }
    }
}

fn split_flag_value(arg: &str) -> (&str, Option<&str>) {
    arg.split_once('=')
        .map_or((arg, None), |(flag, value)| (flag, Some(value)))
}

fn canonical_bool_flag(flag: &str) -> Option<&'static str> {
    match flag {
        "--allow-hpa" | "--allow_hpa" => Some("--allow-hpa"),
        "--use-oomkill-data" | "--use_oomkill_data" => Some("--use-oomkill-data"),
        "--verbose" | "-v" => Some("--verbose"),
        _ => None,
    }
}

fn canonical_value_flag(flag: &str) -> Option<KrrValueArg> {
    match flag {
        "--history-duration" | "--history_duration" => Some(KrrValueArg::PositiveFloat("--history_duration")),
        "--timeframe-duration" | "--timeframe_duration" => Some(KrrValueArg::PositiveFloat("--timeframe_duration")),
        "--cpu-request" | "--cpu_request" => Some(KrrValueArg::Percentile("--cpu-request")),
        "--cpu-limit" | "--cpu_limit" => Some(KrrValueArg::Percentile("--cpu-limit")),
        "--memory-buffer-percentage" | "--memory_buffer_percentage" => {
            Some(KrrValueArg::NonNegativeInteger("--memory-buffer-percentage"))
        }
        "--oom-memory-buffer-percentage" | "--oom_memory_buffer_percentage" => {
            Some(KrrValueArg::NonNegativeInteger("--oom-memory-buffer-percentage"))
        }
        "--points-required" | "--points_required" => Some(KrrValueArg::PositiveInteger("--points_required")),
        "--width" => Some(KrrValueArg::PositiveInteger("--width")),
        "--namespace" | "-n" => Some(KrrValueArg::NonEmptyString("-n")),
        "--resource" | "-r" => Some(KrrValueArg::NonEmptyString("-r")),
        "--selector" | "-s" => Some(KrrValueArg::NonEmptyString("--selector")),
        _ => None,
    }
}

fn validate_value(flag: &str, value: &str, kind: KrrValueArg) -> Result<(), KrrError> {
    if value.trim().is_empty() {
        return Err(invalid_option(flag, "requires a non-empty value"));
    }
    match kind {
        KrrValueArg::PositiveFloat(_) => validate_positive_float(flag, value),
        KrrValueArg::Percentile(_) => validate_percentile(flag, value),
        KrrValueArg::PositiveInteger(_) => validate_integer(flag, value, 1),
        KrrValueArg::NonNegativeInteger(_) => validate_integer(flag, value, 0),
        KrrValueArg::NonEmptyString(_) => Ok(()),
    }
}

fn validate_positive_float(name: &str, raw_value: &str) -> Result<(), KrrError> {
    let value = raw_value
        .parse::<f64>()
        .map_err(|_| invalid_option(name, "must be a number"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid_option(name, "must be a finite number greater than 0"));
    }
    Ok(())
}

fn validate_percentile(name: &str, raw_value: &str) -> Result<(), KrrError> {
    let value = raw_value
        .parse::<f64>()
        .map_err(|_| invalid_option(name, "must be a number"))?;
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return Err(invalid_option(name, "must be a finite number between 0 and 100"));
    }
    Ok(())
}

fn validate_integer(name: &str, raw_value: &str, min: u32) -> Result<(), KrrError> {
    let value = raw_value
        .parse::<u32>()
        .map_err(|_| invalid_option(name, "must be an integer"))?;
    if value < min {
        return Err(invalid_option(name, &format!("must be greater than or equal to {min}")));
    }
    Ok(())
}

fn invalid_option(name: &str, message: &str) -> KrrError {
    KrrError::InvalidOption {
        name: name.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{KrrCmd, KrrOptions, KrrOutputFormat};
    use std::path::PathBuf;
    use url::Url;

    #[test]
    fn test_cmd_args_with_all_options() {
        let options = KrrOptions {
            output_format: KrrOutputFormat::Csv,
            prometheus_url: Url::parse("http://prometheus:9090").unwrap(),
            extra_args: vec![
                "--history_duration".to_string(),
                "120".to_string(),
                "-n".to_string(),
                "default".to_string(),
                "--namespace=qovery".to_string(),
                "-r".to_string(),
                "Deployment".to_string(),
                "--selector".to_string(),
                "qovery.com/environment-id=env".to_string(),
                "--timeframe_duration=2.5".to_string(),
                "--cpu-request".to_string(),
                "99".to_string(),
                "--cpu_limit".to_string(),
                "99".to_string(),
                "--memory-buffer-percentage".to_string(),
                "15".to_string(),
                "--points_required".to_string(),
                "100".to_string(),
                "--allow_hpa".to_string(),
                "--use-oomkill-data".to_string(),
                "--oom_memory_buffer_percentage".to_string(),
                "25".to_string(),
            ],
            file_output: Some(PathBuf::from("/tmp/krr.csv")),
        };

        assert_eq!(
            KrrCmd::args(&options).unwrap(),
            vec![
                "simple-limit",
                "-p",
                "http://prometheus:9090/",
                "-f",
                "csv",
                "--logtostderr",
                "--fileoutput",
                "/tmp/krr.csv",
                "--history_duration",
                "120",
                "-n",
                "default",
                "-n",
                "qovery",
                "-r",
                "Deployment",
                "--selector",
                "qovery.com/environment-id=env",
                "--timeframe_duration",
                "2.5",
                "--cpu-request",
                "99",
                "--cpu-limit",
                "99",
                "--memory-buffer-percentage",
                "15",
                "--points_required",
                "100",
                "--allow-hpa",
                "--use-oomkill-data",
                "--oom-memory-buffer-percentage",
                "25",
            ]
        );
    }

    #[test]
    fn test_cmd_args_rejects_unsafe_options() {
        let options = KrrOptions {
            output_format: KrrOutputFormat::Csv,
            prometheus_url: Url::parse("http://prometheus:9090").unwrap(),
            extra_args: vec!["--fileoutput-dynamic".to_string()],
            file_output: Some(PathBuf::from("/tmp/krr.csv")),
        };

        assert!(KrrCmd::args(&options).is_err());
    }
}
