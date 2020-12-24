use std::fs::read_to_string;

use chrono::Utc;
use retry::delay::Fibonacci;
use retry::OperationResult;

use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause};
use crate::models::Context;
use crate::object_storage::{FileContent, Kind, ObjectStorage};

pub struct S3 {
    context: Context,
    id: String,
    name: String,
    access_key_id: String,
    secret_access_key: String,
}

impl S3 {
    pub fn new(
        context: Context,
        id: String,
        name: String,
        access_key_id: String,
        secret_access_key: String,
    ) -> Self {
        S3 {
            context,
            id,
            name,
            access_key_id,
            secret_access_key,
        }
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (AWS_ACCESS_KEY_ID, self.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, self.secret_access_key.as_str()),
        ]
    }

    fn get_object<S>(&self, object_key: S) -> Result<FileContent, EngineError>
    where
        S: Into<String>,
    {
        // we choose to use the AWS CLI instead of Rusoto S3 due to reliability problems we faced.
        let s3_url = format!("s3://{}", object_key.into());
        let local_path = format!("/tmp/{}.s3object", Utc::now().timestamp_millis()); // FIXME: change hardcoded /tmp/

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context().execution_id(),
            crate::cmd::utilities::exec_with_envs(
                "aws",
                vec!["s3", "cp", &s3_url, &local_path],
                self.credentials_environment_variables(),
            ),
        )?;

        match read_to_string(&local_path) {
            Ok(file_content) => Ok(file_content),
            Err(err) => {
                let message = format!("{:?}", err);

                error!("{}", message);

                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }
}

impl ObjectStorage for S3 {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::S3
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        // TODO check valid credentials
        Ok(())
    }

    fn create_bucket<S>(&self, bucket_name: S) -> Result<(), EngineError>
    where
        S: Into<String>,
    {
        let bucket_name = bucket_name.into();

        cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context().execution_id(),
            crate::cmd::utilities::exec_with_envs(
                "aws",
                vec!["s3api", "create-bucket", "--bucket", bucket_name.as_str()],
                self.credentials_environment_variables(),
            ),
        )
    }

    fn delete_bucket<S>(&self, bucket_name: S) -> Result<(), EngineError>
    where
        S: Into<String>,
    {
        cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context().execution_id(),
            crate::cmd::utilities::exec_with_envs(
                "aws",
                vec![
                    "s3",
                    "rb",
                    "--force",
                    "--bucket",
                    format!("s3://{}", bucket_name.into()).as_str(),
                ],
                self.credentials_environment_variables(),
            ),
        )
    }

    fn get<S>(&self, object_key: S) -> Result<FileContent, EngineError>
    where
        S: Into<String>,
    {
        let object_key = object_key.into();
        let file_content_result = retry::retry(Fibonacci::from_millis(3000).take(5), || match self
            .get_object(object_key.as_str())
        {
            Ok(file_content) => OperationResult::Ok(file_content),
            Err(err) => {
                debug!("{:?}", err);

                warn!(
                    "Can't download object '{}'. Let's retry...",
                    object_key.as_str()
                );

                OperationResult::Retry(err)
            }
        });

        let file_content = match file_content_result {
            Ok(file_content) => file_content,
            Err(_) => {
                let message = "file content is empty (retry \
                                             failed multiple times) - which is not the \
                                             expected content - what's wrong?"
                    .to_string();

                error!("{}", message);

                return Err(self.engine_error(EngineErrorCause::Internal, message));
            }
        };

        Ok(file_content)
    }
}
