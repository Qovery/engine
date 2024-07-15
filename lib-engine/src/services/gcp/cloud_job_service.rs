use crate::cloud_provider::gcp::locations::GcpRegion;
use crate::cmd::command::ExecutableCommand;
use crate::cmd::command::QoveryCommand;
use crate::models::gcp::JsonCredentials;
use crate::models::ToCloudProviderFormat;
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
    is_ready: bool,
}

impl CloudJobService {
    pub fn new(google_credentials: JsonCredentials) -> Result<Self, CloudJobServiceError> {
        // Not optimized, but will be removed once using rust SDK for GCP, prevent from having to inject this service in all services above
        if let Err(e) = GoogleAuthService::activate_service_account(google_credentials) {
            return Err(CloudJobServiceError::CannotInitializeCloudJobService {
                raw_error_message: e.to_string(),
            });
        }
        Ok(CloudJobService { is_ready: true })
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

        match QoveryCommand::new(
            "gcloud",
            vec![
                "run",
                "jobs",
                "create",
                job_name,
                format!("--image={job_image_with_tag}").as_str(),
                format!("--command={job_command}").as_str(),
                job_command_args.as_str(),
                format!("--service-account={service_account_email}").as_str(),
                format!("--region={}", region.to_cloud_provider_format()).as_str(),
                match execute_now {
                    true => "--execute-now",
                    false => "",
                },
                format!("--project={project_id}").as_str(),
                format!("--labels={job_labels_args}").as_str(),
            ]
            .into_iter()
            .filter(|&x| !x.is_empty())
            .collect::<Vec<&str>>()
            .as_slice(),
            &[],
        )
        .exec()
        {
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
