pub const TF_PLUGIN_CACHE_DIR: &str = "TF_PLUGIN_CACHE_DIR";
pub const AWS_ACCESS_KEY_ID: &str = "AWS_ACCESS_KEY_ID";
pub const AWS_SECRET_ACCESS_KEY: &str = "AWS_SECRET_ACCESS_KEY";
pub const AWS_SESSION_TOKEN: &str = "AWS_SESSION_TOKEN";
pub const AWS_DEFAULT_REGION: &str = "AWS_DEFAULT_REGION";
pub const KUBECONFIG: &str = "KUBECONFIG";
pub const SCW_ACCESS_KEY: &str = "SCW_ACCESS_KEY";
pub const SCW_SECRET_KEY: &str = "SCW_SECRET_KEY";
pub const SCW_DEFAULT_PROJECT_ID: &str = "SCW_DEFAULT_PROJECT_ID";
pub const GCP_PROJECT: &str = "GOOGLE_PROJECT";
pub const GCP_REGION: &str = "GOOGLE_REGION";
pub const GCP_CREDENTIALS: &str = "GOOGLE_CREDENTIALS";
pub const GCP_OAUTH_ACCESS_TOKEN: &str = "GOOGLE_OAUTH_ACCESS_TOKEN";
pub const GCP_CLOUDSDK_CONFIG: &str = "CLOUDSDK_CONFIG";

// AWS Partner Network (APN) identifier tag key, required by AWS to measure Qovery-managed resources for the
// AWS Marketplace listing. The value is read once at startup from the QOVERY_AWS_APN_ID env var (see the engine
// CLI) and carried through `Context::aws_apn_id()`; it defaults to "not-set" when the variable is absent.
pub const AWS_APN_ID_TAG_KEY: &str = "aws-apn-id";
