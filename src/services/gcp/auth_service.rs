use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::environment::models::gcp::io::JsonCredentials as IOJsonCredentials;
use crate::environment::models::gcp::JsonCredentials;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;
use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum AuthServiceError {
    #[error("Cannot activate service account `{service_account_email}`, error: `{raw_error_message}`")]
    CannotActivateServiceAccount {
        service_account_email: String,
        raw_error_message: String,
    },
}

pub struct GoogleAuthService {}

impl GoogleAuthService {
    pub fn activate_service_account(google_credentials: JsonCredentials) -> Result<(), AuthServiceError> {
        let dir = tempdir().map_err(|e| AuthServiceError::CannotActivateServiceAccount {
            service_account_email: google_credentials.client_email.clone(),
            raw_error_message: format!("Cannot create temp directory for google credentials, error: {e}"),
        })?;
        let file_path = dir.path().join("google_credentials.json");
        let mut file = File::create(&file_path).map_err(|e| AuthServiceError::CannotActivateServiceAccount {
            service_account_email: google_credentials.client_email.clone(),
            raw_error_message: format!("Cannot create temp file for google credentials, error: {e}"),
        })?;

        let io_credentials = IOJsonCredentials::from(google_credentials.clone());
        writeln!(
            file,
            "{}",
            serde_json::to_string(&io_credentials).map_err(|e| {
                AuthServiceError::CannotActivateServiceAccount {
                    service_account_email: google_credentials.client_email.clone(),
                    raw_error_message: format!("Cannot serialize google credentials to json string, error: {e}",),
                }
            })?
        )
        .map_err(|e| AuthServiceError::CannotActivateServiceAccount {
            service_account_email: google_credentials.client_email.clone(),
            raw_error_message: format!("Cannot write google credentials to temp file, error: {e}"),
        })?;

        match QoveryCommand::new(
            "gcloud",
            &[
                "auth",
                "activate-service-account",
                &google_credentials.client_email,
                format!("--key-file={}", file_path.to_str().unwrap_or_default()).as_str(),
            ],
            &[],
        )
        .exec()
        {
            Ok(_) => Ok(()),
            Err(e) => Err(AuthServiceError::CannotActivateServiceAccount {
                service_account_email: google_credentials.client_email.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }
}
