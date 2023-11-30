use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum ObjectStorageError {
    #[error("Quotas exceeded while performing action on `{bucket_name:?}`: {raw_error_message:?}.")]
    QuotasExceeded {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Invalid bucket name error for `{bucket_name:?}`: {raw_error_message:?}.")]
    InvalidBucketName {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot create bucket error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotCreateBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot get bucket error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotGetBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot delete bucket error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotDeleteBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot empty bucket error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotEmptyBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot tag bucket error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotTagBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot activate bucket versioning on bucket `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotActivateBucketVersioning {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot get object object `{object_name:?}` error in `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotGetObjectFile {
        bucket_name: String,
        object_name: String,
        raw_error_message: String,
    },
    #[error("Cannot upload object `{object_name:?}` error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotUploadFile {
        bucket_name: String,
        object_name: String,
        raw_error_message: String,
    },
    #[error("Cannot delete object `{object_name:?}` error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotDeleteFile {
        bucket_name: String,
        object_name: String,
        raw_error_message: String,
    },
}
