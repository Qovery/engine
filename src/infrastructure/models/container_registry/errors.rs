use std::collections::HashSet;
use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq, Hash)]
pub enum RepositoryNamingRule {
    #[error("Max length reached, should be less or equal to {max_length}.")]
    MaxLengthReached { max_length: usize },
    #[error("Min length not reached, should be greater or equal to {min_length}.")]
    MinLengthNotReached { min_length: usize },
    #[error("Should be alpha numeric characters, dashes and periods.")]
    AlphaNumericCharsDashesPeriodsOnly,
}

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum ContainerRegistryError {
    #[error("Unknown error.")]
    Unknown { raw_error_message: String },
    #[error("Cannot instantiate client: `{raw_error_message}`.")]
    CannotInstantiateClient { raw_error_message: String },
    #[error("Cannot convert client: `{raw_error_message}`.")]
    CannotConvertClient { raw_error_message: String },
    #[error("Invalid registry URL error, cannot be parsed: `{registry_url}`.")]
    InvalidRegistryUrl { registry_url: String },
    #[error("Invalid registry name error, name `{registry_name}` is invalid: {raw_error_message:?}")]
    InvalidRegistryName {
        registry_name: String,
        raw_error_message: String,
    },
    #[error("Invalid credentials error.")]
    InvalidCredentials,
    #[error("Cannot get credentials error.")]
    CannotGetCredentials,
    #[error("Cannot create registry error for `{registry_name:?}`: {raw_error_message:?}.")]
    CannotCreateRegistry {
        registry_name: String,
        raw_error_message: String,
    },

    #[error("Cannot delete registry error for `{registry_name:?}`: {raw_error_message:?}.")]
    CannotDeleteRegistry {
        registry_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot delete image `{image_name:?}` error from repository `{repository_name:?}` in registry `{registry_name:?}`: {raw_error_message:?}."
    )]
    CannotDeleteImage {
        registry_name: String,
        repository_name: String,
        image_name: String,
        raw_error_message: String,
    },
    #[error(
        "Image `{image_name:?}` doesn't exist in repository `{repository_name:?}` in registry `{registry_name:?}` error."
    )]
    ImageDoesntExistInRegistry {
        registry_name: String,
        repository_name: String,
        image_name: String,
    },
    #[error("Repository `{repository_name:?}` doesn't exist in registry `{registry_name:?}` error.")]
    RepositoryDoesntExistInRegistry {
        registry_name: String,
        repository_name: String,
    },
    #[error("Registry `{registry_name:?}` doesn't exist, error: {raw_error_message:?}.")]
    RegistryDoesntExist {
        registry_name: String,
        raw_error_message: String,
    },
    #[error("Cannot link registry `{registry_name:?}` to cluster `{cluster_id:?}`: {raw_error_message:?}.")]
    CannotLinkRegistryToCluster {
        registry_name: String,
        cluster_id: String,
        raw_error_message: String,
    },
    #[error("Cannot create repository `{repository_name:?}` in registry `{registry_name:?}`: {raw_error_message:?}.")]
    CannotCreateRepository {
        registry_name: String,
        repository_name: String,
        raw_error_message: String,
    },
    #[error("Cannot get repository `{repository_name:?}` in registry `{registry_name:?}`: {raw_error_message:?}.")]
    CannotGetRepository {
        registry_name: String,
        repository_name: String,
        raw_error_message: String,
    },
    #[error("Cannot delete repository `{repository_name:?}` from registry `{registry_name:?}`: {raw_error_message:?}.")]
    CannotDeleteRepository {
        registry_name: String,
        repository_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot set lifecycle policy for repository `{repository_name:?}` in registry `{registry_name:?}`: {raw_error_message:?}."
    )]
    CannotSetRepositoryLifecyclePolicy {
        registry_name: String,
        repository_name: String,
        raw_error_message: String,
    },

    #[error(
        "Cannot set tags for repository `{repository_name:?}` in registry `{registry_name:?}`: {raw_error_message:?}."
    )]
    CannotSetRepositoryTags {
        registry_name: String,
        repository_name: String,
        raw_error_message: String,
    },

    #[error(
        "Repository name `{repository_name:?}` in registry `{registry_name:?}  is invalid, following rules are broken: {broken_rules:?}"
    )]
    RepositoryNameNotValid {
        registry_name: String,
        repository_name: String,
        broken_rules: HashSet<RepositoryNamingRule>,
    },
}
