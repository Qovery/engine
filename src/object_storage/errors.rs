use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ObjectStorageError {
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
    #[error("Cannot get workspace error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotGetWorkspace {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot create file error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotCreateFile {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot open file error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotOpenFile {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot read file error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotReadFile {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot get object file `{file_name:?}` error in `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotGetObjectFile {
        bucket_name: String,
        file_name: String,
        raw_error_message: String,
    },
    #[error("Cannot upload file error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotUploadFile {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot delete file error for `{bucket_name:?}`: {raw_error_message:?}.")]
    CannotDeleteFile {
        bucket_name: String,
        raw_error_message: String,
    },
}
