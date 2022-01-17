use std::fs::File;

use crate::cmd::utilities::QoveryCommand;
use retry::delay::Fibonacci;
use retry::{Error, OperationResult};

use crate::constants::{AWS_ACCESS_KEY_ID, AWS_DEFAULT_REGION, AWS_SECRET_ACCESS_KEY};
use crate::error::SimpleErrorKind::Other;
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause, SimpleError};
use crate::models::{Context, StringPath};
use crate::object_storage::{Kind, ObjectStorage};

pub struct S3 {
    context: Context,
    id: String,
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: String,
}

impl S3 {
    pub fn new(
        context: Context,
        id: String,
        name: String,
        access_key_id: String,
        secret_access_key: String,
        region: String,
    ) -> Self {
        S3 {
            context,
            id,
            name,
            access_key_id,
            secret_access_key,
            region,
        }
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (AWS_ACCESS_KEY_ID, self.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, self.secret_access_key.as_str()),
            (AWS_DEFAULT_REGION, self.region.as_str()),
        ]
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

    fn create_bucket(&self, bucket_name: &str) -> Result<(), EngineError> {
        let mut cmd = QoveryCommand::new(
            "aws",
            &vec!["s3api", "create-bucket", "--bucket", bucket_name],
            &self.credentials_environment_variables(),
        );
        cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context().execution_id(),
            cmd.exec()
                .map_err(|err| SimpleError::new(Other, Some(format!("{:?}", err)))),
        )
    }

    fn delete_bucket(&self, bucket_name: &str) -> Result<(), EngineError> {
        let mut cmd = QoveryCommand::new(
            "aws",
            &vec![
                "s3",
                "rb",
                "--force",
                "--bucket",
                format!("s3://{}", bucket_name).as_str(),
            ],
            &self.credentials_environment_variables(),
        );
        cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context().execution_id(),
            cmd.exec()
                .map_err(|err| SimpleError::new(Other, Some(format!("{:?}", err)))),
        )
    }

    fn get(&self, bucket_name: &str, object_key: &str, use_cache: bool) -> Result<(StringPath, File), EngineError> {
        let workspace_directory = crate::fs::workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("object-storage/s3/{}", self.name()),
        )
        .map_err(|err| self.engine_error(EngineErrorCause::Internal, err.to_string()))?;

        let s3_url = format!("s3://{}/{}", bucket_name, object_key);
        let file_path = format!("{}/{}/{}", workspace_directory, bucket_name, object_key);
        let file_path_wt_filename = format!("{}/{}/", workspace_directory, bucket_name);

        if use_cache {
            // does config file already exists?
            match File::open(file_path.as_str()) {
                Ok(file) => {
                    debug!("{} cache hit", file_path.as_str());
                    return Ok((file_path, file));
                }
                Err(_) => debug!("{} cache miss", file_path.as_str()),
            }
        }

        // retrieve config file from object storage
        let mut cmd = QoveryCommand::new(
            "aws",
            &vec!["s3", "cp", s3_url.as_str(), file_path.as_str()],
            &self.credentials_environment_variables(),
        );
        let result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
            // we choose to use the AWS CLI instead of Rusoto S3 due to reliability problems we faced.
            let result = cast_simple_error_to_engine_error(
                self.engine_error_scope(),
                self.context().execution_id(),
                cmd.exec()
                    .map_err(|err| SimpleError::new(Other, Some(format!("{:?}", err)))),
            );

            match result {
                Ok(_) => OperationResult::Ok(()),
                Err(err) => {
                    debug!("{:?}", err);

                    warn!("Can't download object '{}'. Let's retry...", object_key);

                    OperationResult::Retry(err)
                }
            }
        });

        match result {
            Ok(_) => {
                match File::open(file_path.as_str()) {
                    Ok(file) => return Ok((file_path, file)),
                    Err(err) => {
                        error!("{}", &err);
                        //Err(self.engine_error(EngineErrorCause::Internal, format!("{:?}", err)))
                    }
                }
            }
            Err(err) => error!("{:?}", err),
        };

        // need to do this dirty trick because of different S3 management between regions
        let mut cmd = QoveryCommand::new(
            "aws",
            &vec!["s3", "cp", s3_url.as_str(), file_path_wt_filename.as_str()],
            &self.credentials_environment_variables(),
        );
        let result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
            // we choose to use the AWS CLI instead of Rusoto S3 due to reliability problems we faced.
            let result = cast_simple_error_to_engine_error(
                self.engine_error_scope(),
                self.context().execution_id(),
                cmd.exec()
                    .map_err(|err| SimpleError::new(Other, Some(format!("{:?}", err)))),
            );

            match result {
                Ok(_) => OperationResult::Ok(()),
                Err(err) => {
                    debug!("{:?}", err);

                    warn!(
                        "Can't download object without filename '{}'. Let's retry...",
                        object_key
                    );

                    OperationResult::Retry(err)
                }
            }
        });

        let file = match result {
            Ok(_) => File::open(file_path_wt_filename.as_str()),
            Err(err) => {
                return match err {
                    Error::Operation { error, .. } => Err(error),
                    Error::Internal(err) => Err(self.engine_error(EngineErrorCause::Internal, err)),
                };
            }
        };

        match file {
            Ok(file) => return Ok((file_path_wt_filename, file)),
            Err(err) => {
                error!("{}", &err);
                Err(self.engine_error(EngineErrorCause::Internal, format!("{:?}", err)))
            }
        }
    }

    fn put(&self, bucket_name: &str, object_key: &str, file_path: &str) -> Result<(), EngineError> {
        let mut cmd = QoveryCommand::new(
            "aws",
            &vec![
                "s3",
                "cp",
                file_path,
                format!("s3://{}/{}", bucket_name, object_key).as_str(),
            ],
            &self.credentials_environment_variables(),
        );
        cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context().execution_id(),
            cmd.exec()
                .map_err(|err| SimpleError::new(Other, Some(format!("{}", err)))),
        )
    }
}
