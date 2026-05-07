#[derive(thiserror::Error, Debug, PartialEq)]
pub enum BlueprintError {
    #[error(
        "Blueprint tag does not respect the following format '<provider>/<service_name>/<service_version>/<manifest_version>'"
    )]
    InvalidTagFormat,

    #[error("Failed to clone blueprint repository: {0}")]
    CloneError(String),

    #[error("qbm.yml not found at {0}")]
    ManifestNotFound(String),

    #[error("Failed to parse qbm.yml: {0}")]
    ManifestParseError(String),

    #[error("Only ServiceBlueprint is supported. StackBlueprint orchestration is handled by q-core.")]
    UnsupportedBlueprintKind,

    #[error("Failed to generate terraform files: {0}")]
    TerraformGenerationError(String),

    #[error("Terraform execution failed: {0}")]
    TerraformExecutionError(String),

    #[error("Helm blueprint execution is not yet implemented")]
    HelmNotImplemented,

    #[error("Invalid git URL '{0}': {1}")]
    InvalidGitUrl(String, String),

    #[error("Blueprint path '{0}' does not exist in the repository")]
    BlueprintPathNotFound(String),

    #[error("Failed to create working directory: {0}")]
    WorkspaceError(String),
}
