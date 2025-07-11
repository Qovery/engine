use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use std::time::Duration;
use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum AzureAuthServiceError {
    #[error("Cannot login to Azure: {raw_message}")]
    CannotLogin { raw_message: String },
}

pub struct AzureAuthService;

impl AzureAuthService {
    pub fn login(client_id: &str, client_secret: &str, tenant_id: &str) -> Result<(), AzureAuthServiceError> {
        let mut output = vec![];
        let mut error = vec![];
        // az login -t <tenant_id> -u <client_id> -p <client_secret> --service-principal
        QoveryCommand::new(
            "az",
            &[
                "login",
                "-t",
                tenant_id,
                "-u",
                client_id,
                format!("-p={client_secret}").as_str(), // handling secrets starting with `-`
                "--service-principal",
            ],
            &[],
        )
        .exec_with_abort(
            &mut |line| {
                output.push(line);
            },
            &mut |line| {
                error.push(line);
            },
            &CommandKiller::from_timeout(Duration::from_secs(30)),
        )
        .map_err(|_e| AzureAuthServiceError::CannotLogin {
            raw_message: error.join("\n"),
        })
    }

    /// Attempts to login to Azure with retries.
    /// By default, it will retry 10 times with a 5 seconds interval between attempts,
    /// and will timeout after 10 minutes.
    ///
    /// This is useful for cases where the Azure login might fail due to transient issues,
    /// propagating temporary credentials / passwords seems to take some time on Azure.
    pub fn login_with_retry(
        client_id: &str,
        client_secret: &str,
        tenant_id: &str,
    ) -> Result<(), AzureAuthServiceError> {
        let timeout = Duration::from_secs(10 * 60);
        let mut successful_attempt = 0;
        let mut attempts = 0;
        let expected_successful_attempts = 10;

        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            attempts += 1;
            if Self::login(client_id, client_secret, tenant_id).is_ok() {
                successful_attempt += 1;
                if successful_attempt >= expected_successful_attempts {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        }

        Err(AzureAuthServiceError::CannotLogin {
            raw_message: format!("Failed to login to Azure after {attempts} attempts"),
        })
    }
}
