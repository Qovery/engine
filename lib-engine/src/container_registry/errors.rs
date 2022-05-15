use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContainerRegistryError {
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
    #[error("Cannot delete image `{image_name:?}` error from repository `{repository_name:?}` in registry `{registry_name:?}`: {raw_error_message:?}.")]
    CannotDeleteImage {
        registry_name: String,
        repository_name: String,
        image_name: String,
        raw_error_message: String,
    },
    #[error("Image `{image_name:?}` doesn't exist in repository `{repository_name:?}` in registry `{registry_name:?}` error.")]
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
    #[error(
        "Cannot delete repository `{repository_name:?}` from registry `{registry_name:?}`: {raw_error_message:?}."
    )]
    CannotDeleteRepository {
        registry_name: String,
        repository_name: String,
        raw_error_message: String,
    },
    #[error("Cannot set lifecycle policy for repository `{repository_name:?}` in registry `{registry_name:?}`: {raw_error_message:?}.")]
    CannotSetRepositoryLifecyclePolicy {
        registry_name: String,
        repository_name: String,
        raw_error_message: String,
    },
}
