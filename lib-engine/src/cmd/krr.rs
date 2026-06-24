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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KrrOptions {
    pub history_hours: u32,
    pub output_format: KrrOutputFormat,
    pub prometheus_url: Url,
    pub namespaces: Vec<String>,
    pub selector: Option<String>,
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

    pub fn args(options: &KrrOptions) -> Vec<String> {
        let mut args = vec![
            "simple".to_string(),
            "--history_duration".to_string(),
            options.history_hours.to_string(),
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

        for namespace in &options.namespaces {
            args.push("-n".to_string());
            args.push(namespace.to_string());
        }

        if let Some(selector) = &options.selector {
            args.push("--selector".to_string());
            args.push(selector.to_string());
        }

        args
    }

    pub fn run(
        &self,
        kubeconfig: &Path,
        options: &KrrOptions,
        envs: &[(&str, &str)],
        command_killer: &CommandKiller,
    ) -> Result<String, CommandError> {
        let args = Self::args(options);
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
            return Err(CommandError::new(
                "KRR command failed".to_string(),
                Some(format!(
                    "Command `krr {}` failed with `{error}`.\nstdout:\n{}\nstderr:\n{}",
                    args.join(" "),
                    stdout_output.join("\n"),
                    stderr_output.join("\n"),
                )),
                Some(envs_owned),
            ));
        }

        if let Some(file_output) = &options.file_output {
            return fs::read_to_string(file_output).map_err(|error| {
                CommandError::new(
                    "Cannot read KRR report output".to_string(),
                    Some(format!("Cannot read KRR report `{}`: {error}", file_output.display())),
                    None,
                )
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
        self.krr_cmd
            .run(kubeconfig, options, envs, &command_killer)
            .map_err(|error| KrrError::CmdError { error })
    }
}

#[cfg(test)]
mod tests {
    use super::{KrrCmd, KrrOptions, KrrOutputFormat};
    use std::path::PathBuf;
    use url::Url;

    #[test]
    fn test_krr_args_with_all_options() {
        let options = KrrOptions {
            history_hours: 120,
            output_format: KrrOutputFormat::Csv,
            prometheus_url: Url::parse("http://prometheus:9090").unwrap(),
            namespaces: vec!["default".to_string(), "qovery".to_string()],
            selector: Some("qovery.com/environment-id=env".to_string()),
            file_output: Some(PathBuf::from("/tmp/krr.csv")),
        };

        assert_eq!(
            KrrCmd::args(&options),
            vec![
                "simple",
                "--history_duration",
                "120",
                "-p",
                "http://prometheus:9090/",
                "-f",
                "csv",
                "--logtostderr",
                "--fileoutput",
                "/tmp/krr.csv",
                "-n",
                "default",
                "-n",
                "qovery",
                "--selector",
                "qovery.com/environment-id=env",
            ]
        );
    }
}
