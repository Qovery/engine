use crate::cmd::command::ExecutableCommand;
use crate::cmd::command::QoveryCommand;
use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::gcp::GcpCredentials;
use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use crate::services::gcp::auth_service::GoogleAuthService;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum CloudJobServiceError {
    #[error("Service is not ready, error: `{raw_error_message}`")]
    ServiceNotReady { raw_error_message: String },
    #[error("Cannot initialize cloud job service: {raw_error_message:?}")]
    CannotInitializeCloudJobService { raw_error_message: String },
    #[error("Cannot create cloud job `{job_name}`: {raw_error_message:?}")]
    CannotCreateCloudJob {
        job_name: String,
        raw_error_message: String,
    },
}

pub struct CloudJob {
    pub _name: String,
}

// TODO(ENG-1809): this service implementation needs to be done using rust SDK for GCP
pub struct CloudJobService {
    credentials: GcpCredentials,
    is_ready: bool,
}

impl CloudJobService {
    pub fn new_with_credentials(credentials: GcpCredentials) -> Result<Self, CloudJobServiceError> {
        // Not optimized, but will be removed once using rust SDK for GCP, prevent from having to inject this service in all services above
        if let GcpCredentials::ServiceAccount(google_credentials) = &credentials
            && let Err(e) = GoogleAuthService::activate_service_account(google_credentials.as_ref())
        {
            return Err(CloudJobServiceError::CannotInitializeCloudJobService {
                raw_error_message: e.to_string(),
            });
        }
        Ok(CloudJobService {
            credentials,
            is_ready: true,
        })
    }

    pub fn is_ready(&self) -> Result<(), CloudJobServiceError> {
        if !self.is_ready {
            return Err(CloudJobServiceError::ServiceNotReady {
                raw_error_message: "Google auth service is not ready, did you initialize it?".to_string(),
            });
        }

        Ok(())
    }

    pub fn create_job(
        &self,
        job_name: &str,
        job_image_with_tag: &str,
        job_command: &str,
        job_args: &[&str],
        service_account_email: &str,
        project_id: &str,
        region: GcpRegion,
        execute_now: bool,
        job_labels: Option<HashMap<String, String>>,
    ) -> Result<CloudJob, CloudJobServiceError> {
        if let Err(e) = self.is_ready() {
            return Err(CloudJobServiceError::ServiceNotReady {
                raw_error_message: e.to_string(),
            });
        }

        let mut job_command_args: String = "".to_string();
        if !job_args.is_empty() {
            job_command_args = format!("--args={}", job_args.join(","));
        };
        let mut job_labels_args: String = "".to_string();
        if let Some(labels) = job_labels {
            let mut labels_args: Vec<String> = vec![];
            for (key, value) in labels.iter() {
                labels_args.push(format!("{key}={value}"));
            }
            job_labels_args = labels_args.join(",")
        }

        let access_token_file = match &self.credentials {
            GcpCredentials::ServiceAccount(_) => None,
            GcpCredentials::AccessToken(credentials) => Some(
                GoogleAuthService::write_access_token_file(credentials.access_token.as_str()).map_err(|e| {
                    CloudJobServiceError::CannotCreateCloudJob {
                        job_name: job_name.to_string(),
                        raw_error_message: e.to_string(),
                    }
                })?,
            ),
        };

        let mut args = Vec::new();
        if let Some(access_token_file) = &access_token_file {
            args.push(format!(
                "--access-token-file={}",
                access_token_file.path().to_str().unwrap_or_default()
            ));
        }
        args.extend([
            "run".to_string(),
            "jobs".to_string(),
            "create".to_string(),
            job_name.to_string(),
            format!("--image={job_image_with_tag}"),
            format!("--command={job_command}"),
            job_command_args,
            match service_account_email.is_empty() {
                true => "".to_string(),
                false => format!("--service-account={service_account_email}"),
            },
            format!("--region={}", region.to_cloud_provider_format()),
            match execute_now {
                true => "--execute-now".to_string(),
                false => "".to_string(),
            },
            format!("--project={project_id}"),
            format!("--labels={job_labels_args}"),
        ]);
        let args = args
            .iter()
            .filter(|arg| !arg.is_empty())
            .map(String::as_str)
            .collect::<Vec<&str>>();

        match QoveryCommand::new("gcloud", args.as_slice(), &[self.credentials.cloudsdk_config()]).exec() {
            Ok(_) => Ok(CloudJob {
                _name: job_name.to_string(),
            }),
            Err(e) => Err(CloudJobServiceError::CannotCreateCloudJob {
                job_name: job_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }
}
