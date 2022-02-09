use crate::errors;
use crate::events::io::EventDetails;
use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct CommandError {
    message: String,
    message_unsafe: String,
}

impl From<errors::CommandError> for CommandError {
    fn from(error: errors::CommandError) -> Self {
        CommandError {
            message: error.message_safe.unwrap_or("".to_string()),
            message_unsafe: error.message_raw,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Tag {
    /// Unknown: unknown error.
    Unknown,
    MissingRequiredEnvVariable,
    ClusterHasNoWorkerNodes,
    CannotGetWorkspaceDirectory,
    UnsupportedInstanceType,
    CannotRetrieveClusterConfigFile,
    CannotGetClusterNodes,
    NotEnoughResourcesToDeployEnvironment,
    CannotUninstallHelmChart,
    CannotExecuteK8sVersion,
    CannotDetermineK8sMasterVersion,
    CannotDetermineK8sRequestedUpgradeVersion,
    CannotDetermineK8sKubeletWorkerVersion,
    CannotDetermineK8sKubeProxyVersion,
    CannotExecuteK8sApiCustomMetrics,
    K8sPodDisruptionBudgetInInvalidState,
    K8sPodsDisruptionBudgetCannotBeRetrieved,
    K8sCannotDeletePod,
    K8sCannotGetCrashLoopingPods,
    K8sCannotGetPods,
    K8sUpgradeDeployedVsRequestedVersionsInconsistency,
    K8sScaleReplicas,
    K8sLoadBalancerConfigurationIssue,
    K8sServiceError,
    K8sGetLogs,
    K8sGetEvents,
    K8sDescribe,
    K8sHistory,
    K8sCannotCreateNamespace,
    K8sPodIsNotReady,
    K8sNodeIsNotReadyWithTheRequestedVersion,
    K8sNodeIsNotReady,
    UnsupportedRegion,
    UnsupportedZone,
    CannotFindRequiredBinary,
    SubnetsCountShouldBeEven,
    CannotGetOrCreateIamRole,
    CannotCopyFilesFromDirectoryToDirectory,
    CannotPauseClusterTasksAreRunning,
    TerraformCannotRemoveEntryOut,
    TerraformNoStateFileExists,
    TerraformErrorWhileExecutingPipeline,
    TerraformErrorWhileExecutingDestroyPipeline,
    HelmChartsSetupError,
    HelmChartsDeployError,
    HelmChartsUpgradeError,
    HelmChartUninstallError,
    HelmHistoryError,
    CannotGetAnyAvailableVPC,
    UnsupportedVersion,
    CannotGetSupportedVersions,
    CannotGetCluster,
    ObjectStorageCannotCreateBucket,
    ObjectStorageCannotPutFileIntoBucket,
}

impl From<errors::Tag> for Tag {
    fn from(tag: errors::Tag) -> Self {
        match tag {
            errors::Tag::Unknown => Tag::Unknown,
            errors::Tag::UnsupportedInstanceType => Tag::UnsupportedInstanceType,
            errors::Tag::CannotRetrieveClusterConfigFile => Tag::CannotRetrieveClusterConfigFile,
            errors::Tag::CannotGetClusterNodes => Tag::CannotGetClusterNodes,
            errors::Tag::NotEnoughResourcesToDeployEnvironment => Tag::NotEnoughResourcesToDeployEnvironment,
            errors::Tag::MissingRequiredEnvVariable => Tag::MissingRequiredEnvVariable,
            errors::Tag::ClusterHasNoWorkerNodes => Tag::ClusterHasNoWorkerNodes,
            errors::Tag::CannotGetWorkspaceDirectory => Tag::CannotGetWorkspaceDirectory,
            errors::Tag::CannotUninstallHelmChart => Tag::CannotUninstallHelmChart,
            errors::Tag::CannotExecuteK8sVersion => Tag::CannotExecuteK8sVersion,
            errors::Tag::CannotDetermineK8sMasterVersion => Tag::CannotDetermineK8sMasterVersion,
            errors::Tag::CannotDetermineK8sRequestedUpgradeVersion => Tag::CannotDetermineK8sRequestedUpgradeVersion,
            errors::Tag::CannotDetermineK8sKubeletWorkerVersion => Tag::CannotDetermineK8sKubeletWorkerVersion,
            errors::Tag::CannotDetermineK8sKubeProxyVersion => Tag::CannotDetermineK8sKubeProxyVersion,
            errors::Tag::CannotExecuteK8sApiCustomMetrics => Tag::CannotExecuteK8sApiCustomMetrics,
            errors::Tag::K8sPodDisruptionBudgetInInvalidState => Tag::K8sPodDisruptionBudgetInInvalidState,
            errors::Tag::K8sPodsDisruptionBudgetCannotBeRetrieved => Tag::K8sPodsDisruptionBudgetCannotBeRetrieved,
            errors::Tag::K8sCannotDeletePod => Tag::K8sCannotDeletePod,
            errors::Tag::K8sCannotGetCrashLoopingPods => Tag::K8sCannotGetCrashLoopingPods,
            errors::Tag::K8sCannotGetPods => Tag::K8sCannotGetPods,
            errors::Tag::K8sUpgradeDeployedVsRequestedVersionsInconsistency => {
                Tag::K8sUpgradeDeployedVsRequestedVersionsInconsistency
            }
            errors::Tag::K8sScaleReplicas => Tag::K8sScaleReplicas,
            errors::Tag::K8sLoadBalancerConfigurationIssue => Tag::K8sLoadBalancerConfigurationIssue,
            errors::Tag::K8sServiceError => Tag::K8sServiceError,
            errors::Tag::K8sGetLogs => Tag::K8sGetLogs,
            errors::Tag::K8sGetEvents => Tag::K8sGetEvents,
            errors::Tag::K8sDescribe => Tag::K8sDescribe,
            errors::Tag::K8sHistory => Tag::K8sHistory,
            errors::Tag::K8sCannotCreateNamespace => Tag::K8sCannotCreateNamespace,
            errors::Tag::K8sPodIsNotReady => Tag::K8sPodIsNotReady,
            errors::Tag::CannotFindRequiredBinary => Tag::CannotFindRequiredBinary,
            errors::Tag::SubnetsCountShouldBeEven => Tag::SubnetsCountShouldBeEven,
            errors::Tag::CannotGetOrCreateIamRole => Tag::CannotGetOrCreateIamRole,
            errors::Tag::CannotCopyFilesFromDirectoryToDirectory => Tag::CannotCopyFilesFromDirectoryToDirectory,
            errors::Tag::CannotPauseClusterTasksAreRunning => Tag::CannotPauseClusterTasksAreRunning,
            errors::Tag::TerraformCannotRemoveEntryOut => Tag::TerraformCannotRemoveEntryOut,
            errors::Tag::TerraformNoStateFileExists => Tag::TerraformNoStateFileExists,
            errors::Tag::TerraformErrorWhileExecutingPipeline => Tag::TerraformErrorWhileExecutingPipeline,
            errors::Tag::TerraformErrorWhileExecutingDestroyPipeline => {
                Tag::TerraformErrorWhileExecutingDestroyPipeline
            }
            errors::Tag::HelmChartsSetupError => Tag::HelmChartsSetupError,
            errors::Tag::HelmChartsDeployError => Tag::HelmChartsDeployError,
            errors::Tag::HelmChartsUpgradeError => Tag::HelmChartsUpgradeError,
            errors::Tag::HelmChartUninstallError => Tag::HelmChartUninstallError,
            errors::Tag::HelmHistoryError => Tag::HelmHistoryError,
            errors::Tag::CannotGetAnyAvailableVPC => Tag::CannotGetAnyAvailableVPC,
            errors::Tag::UnsupportedVersion => Tag::UnsupportedVersion,
            errors::Tag::CannotGetSupportedVersions => Tag::CannotGetSupportedVersions,
            errors::Tag::CannotGetCluster => Tag::CannotGetCluster,
            errors::Tag::ObjectStorageCannotCreateBucket => Tag::ObjectStorageCannotCreateBucket,
            errors::Tag::ObjectStorageCannotPutFileIntoBucket => Tag::ObjectStorageCannotPutFileIntoBucket,
            errors::Tag::UnsupportedRegion => Tag::UnsupportedRegion,
            errors::Tag::UnsupportedZone => Tag::UnsupportedZone,
            errors::Tag::K8sNodeIsNotReadyWithTheRequestedVersion => Tag::K8sNodeIsNotReadyWithTheRequestedVersion,
            errors::Tag::K8sNodeIsNotReady => Tag::K8sNodeIsNotReady,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct EngineError {
    tag: Tag,
    event_details: EventDetails,
    qovery_log_message: String,
    user_log_message: String,
    message: Option<CommandError>,
    link: Option<String>,
    hint_message: Option<String>,
}

impl From<errors::EngineError> for EngineError {
    fn from(error: errors::EngineError) -> Self {
        EngineError {
            tag: Tag::from(error.tag),
            event_details: EventDetails::from(error.event_details),
            qovery_log_message: error.qovery_log_message,
            user_log_message: error.user_log_message,
            message: match error.message {
                Some(msg) => Some(CommandError::from(msg)),
                None => None,
            },
            link: error.link.map(|url| url.to_string()),
            hint_message: error.hint_message,
        }
    }
}
