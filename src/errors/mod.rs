pub mod io;

extern crate url;

use crate::cloud_provider::utilities::VersionsNumber;
use crate::error::{EngineError as LegacyEngineError, EngineErrorCause, EngineErrorScope};
use crate::events::EventDetails;
use url::Url;

/// CommandError: command error, mostly returned by third party tools.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandError {
    /// message: full error message, can contains unsafe text such as passwords and tokens.
    message_raw: String,
    /// message_safe: error message omitting displaying any protected data such as passwords and tokens.
    message_safe: Option<String>,
}

impl CommandError {
    /// Returns CommandError message_raw. May contains unsafe text such as passwords and tokens.
    pub fn message_raw(&self) -> String {
        self.message_raw.to_string()
    }

    /// Returns CommandError message_safe omitting all unsafe text such as passwords and tokens.
    pub fn message_safe(&self) -> Option<String> {
        self.message_safe.clone()
    }

    /// Returns error all message (safe + unsafe).
    pub fn message(&self) -> String {
        // TODO(benjaminch): To be revamped, not sure how we should deal with safe and unsafe messages.
        if let Some(msg) = &self.message_safe {
            return format!("{} {}", msg, self.message_raw);
        }

        self.message_raw.to_string()
    }

    /// Creates a new CommandError from safe message. To be used when message is safe.
    pub fn new_from_safe_message(message: String) -> Self {
        CommandError::new(message.clone(), Some(message))
    }

    /// Creates a new CommandError having both a safe and an unsafe message.
    pub fn new(message_raw: String, message_safe: Option<String>) -> Self {
        CommandError {
            message_raw,
            message_safe,
        }
    }

    /// Create a new CommandError from a CMD command.
    pub fn new_from_command_line(
        message: String,
        bin: String,
        cmd_args: Vec<String>,
        envs: Vec<(String, String)>,
        stdout: Option<String>,
        stderr: Option<String>,
    ) -> Self {
        let mut unsafe_message = format!(
            "{}\ncommand: {} {}\nenv: {}",
            message.to_string(),
            bin.to_string(),
            cmd_args.join(" "),
            envs.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<String>>()
                .join(" ")
        );

        if let Some(txt) = stdout {
            unsafe_message = format!("{}\nSTDOUT {}", unsafe_message, txt);
        }
        if let Some(txt) = stderr {
            unsafe_message = format!("{}\nSTDERR {}", unsafe_message, txt);
        }

        CommandError::new(unsafe_message, Some(message))
    }
}

#[derive(Clone, Debug)]
/// Tag: unique identifier for an error.
pub enum Tag {
    /// Unknown: unknown error.
    Unknown,
    /// MissingRequiredEnvVariable: represents an error where a required env variable is not set.
    MissingRequiredEnvVariable,
    /// ClusterHasNoWorkerNodes: represents an error where the current cluster doesn't have any worker nodes.
    ClusterHasNoWorkerNodes,
    /// CannotGetWorkspaceDirectory: represents an error while trying to get workspace directory.
    CannotGetWorkspaceDirectory,
    /// UnsupportedInstanceType: represents an unsupported instance type for the given cloud provider.
    UnsupportedInstanceType,
    /// UnsupportedRegion: represents an unsupported region for the given cloud provider.
    UnsupportedRegion,
    /// UnsupportedZone: represents an unsupported zone in region for the given cloud provider.
    UnsupportedZone,
    /// CannotRetrieveKubernetesConfigFile: represents an error while trying to retrieve Kubernetes config file.
    CannotRetrieveClusterConfigFile,
    /// CannotGetClusterNodes: represents an error while trying to get cluster's nodes.
    CannotGetClusterNodes,
    /// NotEnoughResourcesToDeployEnvironment: represents an error when trying to deploy an environment but there are not enough resources available on the cluster.
    NotEnoughResourcesToDeployEnvironment,
    /// CannotUninstallHelmChart: represents an error when trying to uninstall an helm chart on the cluster, uninstallation couldn't be proceeded.
    CannotUninstallHelmChart,
    /// CannotExecuteK8sVersion: represents an error when trying to execute kubernetes version command.
    CannotExecuteK8sVersion,
    /// CannotDetermineK8sMasterVersion: represents an error when trying to determine kubernetes master version which cannot be retrieved.
    CannotDetermineK8sMasterVersion,
    /// CannotDetermineK8sRequestedUpgradeVersion: represents an error when trying to determines upgrade requested kubernetes version.
    CannotDetermineK8sRequestedUpgradeVersion,
    /// CannotDetermineK8sKubeletWorkerVersion: represents an error when trying to determine kubelet worker version which cannot be retrieved.
    CannotDetermineK8sKubeletWorkerVersion,
    /// CannotDetermineK8sKubeProxyVersion: represents an error when trying to determine kube proxy version which cannot be retrieved.
    CannotDetermineK8sKubeProxyVersion,
    /// CannotExecuteK8sApiCustomMetrics: represents an error when trying to get K8s API custom metrics.
    CannotExecuteK8sApiCustomMetrics,
    /// K8sPodDisruptionBudgetInInvalidState: represents an error where pod disruption budget is in an invalid state.
    K8sPodDisruptionBudgetInInvalidState,
    /// K8sPodDisruptionBudgetCqnnotBeRetrieved: represents an error where pod disruption budget cannot be retrieved.
    K8sPodsDisruptionBudgetCannotBeRetrieved,
    /// K8sCannotDeletePod: represents an error where we are not able to delete a pod.
    K8sCannotDeletePod,
    /// K8sCannotGetCrashLoopingPods: represents an error where we are not able to get crash looping pods.
    K8sCannotGetCrashLoopingPods,
    /// K8sCannotGetPods: represents an error where we are not able to get pods.
    K8sCannotGetPods,
    /// K8sUpgradeDeployedVsRequestedVersionsInconsistency: represents an error where there is a K8s versions inconsistency between deployed and requested.
    K8sUpgradeDeployedVsRequestedVersionsInconsistency,
    /// K8sScaleReplicas: represents an error while trying to scale replicas.
    K8sScaleReplicas,
    /// K8sLoadBalancerConfigurationIssue: represents an error where loadbalancer has a configuration issue.
    K8sLoadBalancerConfigurationIssue,
    /// K8sServiceError: represents an error on a k8s service.
    K8sServiceError,
    /// K8sGetLogs: represents an error during a k8s logs command.
    K8sGetLogs,
    /// K8sGetEvents: represents an error during a k8s get events command.
    K8sGetEvents,
    /// K8sDescribe: represents an error during a k8s describe command.
    K8sDescribe,
    /// K8sHistory: represents an error during a k8s history command.
    K8sHistory,
    /// K8sCannotCreateNamespace: represents an error while trying to create a k8s namespace.
    K8sCannotCreateNamespace,
    /// K8sPodIsNotReady: represents an error where the given pod is not ready.
    K8sPodIsNotReady,
    /// K8sNodeIsNotReadyInTheGivenVersion: represents an error where the given node is not ready in the given version.
    K8sNodeIsNotReadyWithTheRequestedVersion,
    /// K8sNodeIsNotReady: represents an error where the given node is not ready.
    K8sNodeIsNotReady,
    /// CannotFindRequiredBinary: represents an error where a required binary is not found on the system.
    CannotFindRequiredBinary,
    /// SubnetsCountShouldBeEven: represents an error where subnets count should be even to have as many public than private subnets.
    SubnetsCountShouldBeEven,
    /// CannotGetOrCreateIamRole: represents an error where we cannot get or create the given IAM role.
    CannotGetOrCreateIamRole,
    /// CannotCopyFilesFromDirectoryToDirectory: represents an error where we cannot copy files from one directory to another.
    CannotCopyFilesFromDirectoryToDirectory,
    /// CannotPauseClusterTasksAreRunning: represents an error where we cannot pause the cluster because some tasks are still running in the engine.
    CannotPauseClusterTasksAreRunning,
    /// TerraformCannotRemoveEntryOut: represents an error where we cannot remove an entry out of Terraform.
    TerraformCannotRemoveEntryOut,
    /// TerraformNoStateFileExists: represents an error where there is no Terraform state file.
    TerraformNoStateFileExists,
    /// TerraformErrorWhileExecutingPipeline: represents an error while executing Terraform pipeline.
    TerraformErrorWhileExecutingPipeline,
    /// TerraformErrorWhileExecutingDestroyPipeline: represents an error while executing Terraform destroying pipeline.
    TerraformErrorWhileExecutingDestroyPipeline,
    /// HelmChartsSetupError: represents an error while trying to setup helm charts.
    HelmChartsSetupError,
    /// HelmChartsDeployError: represents an error while trying to deploy helm charts.
    HelmChartsDeployError,
    /// HelmChartsUpgradeError: represents an error while trying to upgrade helm charts.
    HelmChartsUpgradeError,
    /// HelmChartUninstallError: represents an error while trying to uninstall an helm chart.
    HelmChartUninstallError,
    /// HelmHistoryError: represents an error while trying to execute helm history on a helm chart.
    HelmHistoryError,
    /// CannotGetAnyAvailableVPC: represents an error while trying to get any available VPC.
    CannotGetAnyAvailableVPC,
    /// UnsupportedVersion: represents an error where product doesn't support the given version.
    UnsupportedVersion,
    /// CannotGetSupportedVersions: represents an error while trying to get supported versions.
    CannotGetSupportedVersions,
    /// CannotGetCluster: represents an error where we cannot get cluster.
    CannotGetCluster,
    /// ObjectStorageCannotCreateBucket: represents an error while trying to create a new object storage bucket.
    ObjectStorageCannotCreateBucket,
    /// ObjectStorageCannotPutFileIntoBucket: represents an error while trying to put a file into an object storage bucket.
    ObjectStorageCannotPutFileIntoBucket,
}

#[derive(Clone, Debug)]
/// EngineError: represents an engine error. Engine will always returns such errors carrying context infos easing monitoring and debugging.
pub struct EngineError {
    /// tag: error unique identifier
    tag: Tag,
    /// event_details: holds context details in which error was triggered such as organization ID, cluster ID, etc.
    event_details: EventDetails,
    /// qovery_log_message: message targeted toward Qovery team, carrying eventual debug / more fine grained messages easing investigations.
    qovery_log_message: String,
    /// user_log_message: message targeted toward Qovery users, might avoid any useless info for users such as Qovery specific identifiers and so on.
    user_log_message: String,
    /// raw_message: raw error message such as command input / output.
    message: Option<CommandError>,
    /// link: link to error documentation (qovery blog, forum, etc.)
    link: Option<Url>,
    /// hint_message: an hint message aiming to give an hint to the user. For example: "Happens when application port has been changed but application hasn't been restarted.".
    hint_message: Option<String>,
}

impl EngineError {
    /// Returns error's unique identifier.
    pub fn tag(&self) -> &Tag {
        &self.tag
    }

    /// Returns error's event details.
    pub fn event_details(&self) -> &EventDetails {
        &self.event_details
    }

    /// Returns qovery log message.
    pub fn qovery_log_message(&self) -> &str {
        &self.qovery_log_message
    }

    /// Returns user log message.
    pub fn user_log_message(&self) -> &str {
        &self.user_log_message
    }

    /// Returns proper error message.
    pub fn message(&self) -> String {
        match &self.message {
            Some(msg) => msg.message(),
            None => self.qovery_log_message.to_string(),
        }
    }

    /// Returns error's link.
    pub fn link(&self) -> &Option<Url> {
        &self.link
    }

    /// Returns error's hint message.
    pub fn hint_message(&self) -> &Option<String> {
        &self.hint_message
    }

    /// Creates new EngineError.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `tag`: Error unique identifier.
    /// * `qovery_log_message`: Error log message targeting Qovery team for investigation / monitoring purposes.
    /// * `user_log_message`: Error log message targeting Qovery user, avoiding any extending pointless details.
    /// * `error_message`: Raw error message.
    /// * `raw_message_safe`: Error raw message such as command input / output where any unsafe data as been omitted (such as plain passwords / tokens).
    /// * `link`: Link documenting the given error.
    /// * `hint_message`: hint message aiming to give an hint to the user. For example: "Happens when application port has been changed but application hasn't been restarted.".
    fn new(
        event_details: EventDetails,
        tag: Tag,
        qovery_log_message: String,
        user_log_message: String,
        message: Option<CommandError>,
        link: Option<Url>,
        hint_message: Option<String>,
    ) -> Self {
        EngineError {
            event_details,
            tag,
            qovery_log_message,
            user_log_message,
            message,
            link,
            hint_message,
        }
    }

    /// Converts to legacy engine error easing migration.
    pub fn to_legacy_engine_error(self) -> LegacyEngineError {
        LegacyEngineError::new(
            EngineErrorCause::Internal,
            EngineErrorScope::from(self.event_details.transmitter()),
            self.event_details.execution_id().to_string(),
            Some(self.message()),
        )
    }

    /// Creates new unknown error.
    ///
    /// Note: do not use unless really needed, every error should have a clear type.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `qovery_log_message`: Error log message targeting Qovery team for investigation / monitoring purposes.
    /// * `user_log_message`: Error log message targeting Qovery user, avoiding any extending pointless details.
    /// * `message`: Error message such as command input / output.
    /// * `link`: Link documenting the given error.
    /// * `hint_message`: hint message aiming to give an hint to the user. For example: "Happens when application port has been changed but application hasn't been restarted.".
    pub fn new_unknown(
        event_details: EventDetails,
        qovery_log_message: String,
        user_log_message: String,
        message: Option<CommandError>,
        link: Option<Url>,
        hint_message: Option<String>,
    ) -> EngineError {
        EngineError::new(
            event_details,
            Tag::Unknown,
            qovery_log_message,
            user_log_message,
            message,
            link,
            hint_message,
        )
    }

    /// Creates new error for missing required env variable.
    ///
    ///
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `variable_name`: Variable name which is not set.
    pub fn new_missing_required_env_variable(event_details: EventDetails, variable_name: String) -> EngineError {
        let message = format!("`{}` environment variable wasn't found.", variable_name);
        EngineError::new(
            event_details,
            Tag::MissingRequiredEnvVariable,
            message.to_string(),
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Creates new error for cluster has no worker nodes.
    ///
    ///
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    pub fn new_cluster_has_no_worker_nodes(event_details: EventDetails) -> EngineError {
        let message = "No worker nodes present, can't proceed with operation.";
        EngineError::new(
            event_details,
            Tag::ClusterHasNoWorkerNodes,
            message.to_string(),
            message.to_string(),
            None,
            None,
            Some(
                "This can happen if there where a manual operations on the workers or the infrastructure is paused."
                    .to_string(),
            ),
        )
    }

    /// Creates new error for unsupported instance type.
    ///
    /// Cloud provider doesn't support the requested instance type.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `requested_instance_type`: Raw requested instance type string.
    /// * `error_message`: Raw error message.
    pub fn new_unsupported_instance_type(
        event_details: EventDetails,
        requested_instance_type: &str,
        error_message: CommandError,
    ) -> EngineError {
        let message = format!("`{}` instance type is not supported", requested_instance_type);
        EngineError::new(
            event_details,
            Tag::UnsupportedInstanceType,
            message.to_string(),
            message,
            Some(error_message),
            None, // TODO(documentation): Create a page entry to details this error
            Some("Selected instance type is not supported, please check provider's documentation.".to_string()),
        )
    }

    /// Creates new error for unsupported region.
    ///
    /// Cloud provider doesn't support the requested region.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `requested_region`: Raw requested region string.
    /// * `error_message`: Raw error message.
    pub fn new_unsupported_region(
        event_details: EventDetails,
        requested_region: String,
        error_message: CommandError,
    ) -> EngineError {
        let message = format!("`{}` region is not supported", requested_region);
        EngineError::new(
            event_details,
            Tag::UnsupportedRegion,
            message.to_string(),
            message,
            Some(error_message),
            None, // TODO(documentation): Create a page entry to details this error
            Some("Selected region is not supported, please check provider's documentation.".to_string()),
        )
    }

    /// Creates new error for unsupported zone for region.
    ///
    /// Cloud provider doesn't support the requested zone in region.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `region`: Raw requested region string.
    /// * `requested_zone`: Raw requested zone string.
    /// * `error_message`: Raw error message.
    pub fn new_unsupported_zone(
        event_details: EventDetails,
        region: String,
        requested_zone: String,
        error_message: CommandError,
    ) -> EngineError {
        let message = format!("Zone `{}` is not supported in region `{}`.", requested_zone, region);
        EngineError::new(
            event_details,
            Tag::UnsupportedZone,
            message.to_string(),
            message,
            Some(error_message),
            None, // TODO(documentation): Create a page entry to details this error
            Some("Selected zone is not supported in the region, please check provider's documentation.".to_string()),
        )
    }

    /// Creates new error: cannot get workspace directory.
    ///
    /// Error occured while trying to get workspace directory.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `error_message`: Raw error message.
    pub fn new_cannot_get_workspace_directory(event_details: EventDetails, error_message: CommandError) -> EngineError {
        let message = "Error while trying to get workspace directory";
        EngineError::new(
            event_details,
            Tag::CannotGetWorkspaceDirectory,
            message.to_string(),
            message.to_string(),
            Some(error_message),
            None,
            None,
        )
    }

    /// Creates new error for cluster configuration file couldn't be retrieved.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `error_message`: Raw error message.
    pub fn new_cannot_retrieve_cluster_config_file(
        event_details: EventDetails,
        error_message: CommandError,
    ) -> EngineError {
        let message = "Cannot retrieve Kubernetes instance type is not supported";
        EngineError::new(
            event_details,
            Tag::CannotRetrieveClusterConfigFile,
            message.to_string(),
            message.to_string(),
            Some(error_message),
            None,
            None,
        )
    }

    /// Creates new error for Kubernetes cannot get nodes.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `error_message`: Raw error message.
    pub fn new_cannot_get_cluster_nodes(event_details: EventDetails, error_message: CommandError) -> EngineError {
        let message = "Cannot get Kubernetes nodes";
        EngineError::new(
            event_details,
            Tag::CannotRetrieveClusterConfigFile,
            message.to_string(),
            message.to_string(),
            Some(error_message),
            None,
            None,
        )
    }

    /// Creates new error for cannot deploy because there are not enough available resources on the cluster.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `requested_ram_in_mib`: How much RAM in mib is requested.
    /// * `free_ram_in_mib`: How much RAM in mib is free.
    /// * `requested_cpu`: How much CPU is requested.
    /// * `free_cpu`: How much CPU is free.
    pub fn new_cannot_deploy_not_enough_resources_available(
        event_details: EventDetails,
        requested_ram_in_mib: u32,
        free_ram_in_mib: u32,
        requested_cpu: f32,
        free_cpu: f32,
    ) -> EngineError {
        let mut message = vec!["There is not enough resources on the cluster:".to_string()];

        if free_cpu > requested_cpu {
            message.push(format!(
                "{} CPU requested and only {} CPU available",
                free_cpu, requested_cpu
            ));
        }

        if requested_ram_in_mib > free_ram_in_mib {
            message.push(format!(
                "{}mib RAM requested and only {}mib RAM  available",
                requested_ram_in_mib, free_ram_in_mib
            ));
        }

        let message = message.join("\n");

        EngineError::new(
            event_details,
            Tag::NotEnoughResourcesToDeployEnvironment,
            message.to_string(),
            message.to_string(),
            None,
            None,
            Some("Consider to add one more node or upgrade your nodes configuration. If not possible, pause or delete unused environments.".to_string()),
        )
    }

    /// Creates new error for cannot deploy because there are not enough free pods available on the cluster.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `requested_pods`: How many pods are requested.
    /// * `free_pods`: How many pods qre free.
    pub fn new_cannot_deploy_not_enough_free_pods_available(
        event_details: EventDetails,
        requested_pods: u32,
        free_pods: u32,
    ) -> EngineError {
        let message = format!(
            "There is not enough free Pods (free {} VS {} required) on the cluster.",
            free_pods, requested_pods,
        );

        EngineError::new(
            event_details,
            Tag::NotEnoughResourcesToDeployEnvironment,
            message.to_string(),
            message.to_string(),
            None,
            None,
            Some("Consider to add one more node or upgrade your nodes configuration. If not possible, pause or delete unused environments.".to_string()),
        )
    }

    /// Creates new error for cannot uninstall an helm chart.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `helm_chart_name`: Helm chart name.
    /// * `errored_object_kind`: Errored kubernetes object name.
    /// * `raw_error`: Raw error message.
    pub fn new_cannot_uninstall_helm_chart(
        event_details: EventDetails,
        helm_chart_name: String,
        errored_object_kind: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Wasn't able to delete all objects type {}, it's a blocker to then delete cert-manager namespace. {}",
            helm_chart_name, errored_object_kind,
        );

        EngineError::new(
            event_details,
            Tag::CannotUninstallHelmChart,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for cannot exec K8s version.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `error_message`: Raw error message.
    pub fn new_cannot_execute_k8s_exec_version(
        event_details: EventDetails,
        error_message: CommandError,
    ) -> EngineError {
        let message = "Unable to execute Kubernetes exec version: `{}`";

        EngineError::new(
            event_details,
            Tag::CannotExecuteK8sVersion,
            message.to_string(),
            message.to_string(),
            Some(error_message),
            None,
            None,
        )
    }

    /// Creates new error for cannot determine kubernetes master version.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `kubernetes_raw_version`: Kubernetes master raw version string we tried to get version from.
    pub fn new_cannot_determine_k8s_master_version(
        event_details: EventDetails,
        kubernetes_raw_version: String,
    ) -> EngineError {
        let message = format!(
            "Unable to determine Kubernetes master version: `{}`",
            kubernetes_raw_version,
        );

        EngineError::new(
            event_details,
            Tag::CannotDetermineK8sMasterVersion,
            message.to_string(),
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Creates new error for cannot determine kubernetes master version.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `kubernetes_upgrade_requested_raw_version`: Kubernetes requested upgrade raw version string.
    /// * `error_message`: Raw error message.
    pub fn new_cannot_determine_k8s_requested_upgrade_version(
        event_details: EventDetails,
        kubernetes_upgrade_requested_raw_version: String,
        error_message: Option<CommandError>,
    ) -> EngineError {
        let message = format!(
            "Unable to determine Kubernetes upgrade requested version: `{}`. Upgrade is not possible.",
            kubernetes_upgrade_requested_raw_version,
        );

        EngineError::new(
            event_details,
            Tag::CannotDetermineK8sRequestedUpgradeVersion,
            message.to_string(),
            message.to_string(),
            error_message,
            None,
            None,
        )
    }

    /// Creates new error for cannot determine kubernetes kubelet worker version.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `kubelet_worker_raw_version`: Kubelet raw version string we tried to get version from.
    pub fn new_cannot_determine_k8s_kubelet_worker_version(
        event_details: EventDetails,
        kubelet_worker_raw_version: String,
    ) -> EngineError {
        let message = format!(
            "Unable to determine Kubelet worker version: `{}`",
            kubelet_worker_raw_version,
        );

        EngineError::new(
            event_details,
            Tag::CannotDetermineK8sKubeletWorkerVersion,
            message.to_string(),
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Creates new error for cannot determine kubernetes kube proxy version.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `kube_proxy_raw_version`: Kube proxy raw version string we tried to get version from.
    pub fn new_cannot_determine_k8s_kube_proxy_version(
        event_details: EventDetails,
        kube_proxy_raw_version: String,
    ) -> EngineError {
        let message = format!("Unable to determine Kube proxy version: `{}`", kube_proxy_raw_version,);

        EngineError::new(
            event_details,
            Tag::CannotDetermineK8sKubeProxyVersion,
            message.to_string(),
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Creates new error for cannot get api custom metrics.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_k8s_error`: Raw error message.
    pub fn new_cannot_get_k8s_api_custom_metrics(
        event_details: EventDetails,
        raw_k8s_error: CommandError,
    ) -> EngineError {
        let message = "Error while looking at the API metric value";

        EngineError::new(
            event_details,
            Tag::CannotExecuteK8sApiCustomMetrics,
            message.to_string(),
            message.to_string(),
            Some(raw_k8s_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes pod disruption budget being in an invalid state.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `pod_name`: Pod name having PDB in an invalid state.
    pub fn new_k8s_pod_disruption_budget_invalid_state(event_details: EventDetails, pod_name: String) -> EngineError {
        let message = format!(
            "Unable to upgrade Kubernetes, pdb for app `{}` in invalid state.",
            pod_name,
        );

        EngineError::new(
            event_details,
            Tag::K8sPodDisruptionBudgetInInvalidState,
            message.to_string(),
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Creates new error for kubernetes not being able to retrieve pods disruption budget.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_k8s_error`: Raw error message.
    pub fn new_k8s_cannot_retrieve_pods_disruption_budget(
        event_details: EventDetails,
        raw_k8s_error: CommandError,
    ) -> EngineError {
        let message = "Unable to upgrade Kubernetes, can't get pods disruptions budgets.";

        EngineError::new(
            event_details,
            Tag::K8sPodsDisruptionBudgetCannotBeRetrieved,
            message.to_string(),
            message.to_string(),
            Some(raw_k8s_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes not being able to delete a pod.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `pod_name`: Pod's name.
    /// * `raw_k8s_error`: Raw error message.
    pub fn new_k8s_cannot_delete_pod(
        event_details: EventDetails,
        pod_name: String,
        raw_k8s_error: CommandError,
    ) -> EngineError {
        let message = format!("Unable to delete Kubernetes pod `{}`.", pod_name);

        EngineError::new(
            event_details,
            Tag::K8sCannotDeletePod,
            message.to_string(),
            message.to_string(),
            Some(raw_k8s_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes not being able to get crash looping pods.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_k8s_error`: Raw error message.
    pub fn new_k8s_cannot_get_crash_looping_pods(
        event_details: EventDetails,
        raw_k8s_error: CommandError,
    ) -> EngineError {
        let message = "Unable to get crash looping Kubernetes pods.";

        EngineError::new(
            event_details,
            Tag::K8sCannotGetCrashLoopingPods,
            message.to_string(),
            message.to_string(),
            Some(raw_k8s_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes not being able to get pods.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_k8s_error`: Raw error message.
    pub fn new_k8s_cannot_get_pods(event_details: EventDetails, raw_k8s_error: CommandError) -> EngineError {
        let message = "Unable to get Kubernetes pods.";

        EngineError::new(
            event_details,
            Tag::K8sCannotGetPods,
            message.to_string(),
            message.to_string(),
            Some(raw_k8s_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes upgrade version inconsistency.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `deployed_version`: Deployed k8s version.
    /// * `requested_version`: Requested k8s version.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_version_upgrade_deployed_vs_requested_versions_inconsistency(
        event_details: EventDetails,
        deployed_version: VersionsNumber,
        requested_version: VersionsNumber,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Unable to upgrade Kubernetes due to version inconsistency. Deployed version: {}, requested version: {}.",
            deployed_version, requested_version
        );

        EngineError::new(
            event_details,
            Tag::K8sUpgradeDeployedVsRequestedVersionsInconsistency,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes scale replicas.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `selector`: K8s selector.
    /// * `namespace`: K8s namespace.
    /// * `requested_replicas`: Number of requested replicas.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_scale_replicas(
        event_details: EventDetails,
        selector: String,
        namespace: String,
        requested_replicas: u32,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Unable to scale Kubernetes `{}` replicas to `{}` in namespace `{}`.",
            selector, requested_replicas, namespace,
        );

        EngineError::new(
            event_details,
            Tag::K8sScaleReplicas,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes load balancer configuration issue.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_loadbalancer_configuration_issue(
        event_details: EventDetails,
        raw_error: CommandError,
    ) -> EngineError {
        let message = "Error, there is an issue with loadbalancer configuration.";

        EngineError::new(
            event_details,
            Tag::K8sLoadBalancerConfigurationIssue,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes service issue.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_service_issue(event_details: EventDetails, raw_error: CommandError) -> EngineError {
        let message = "Error, there is an issue with service.";

        EngineError::new(
            event_details,
            Tag::K8sServiceError,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes get logs.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `selector`: Selector to get logs for.
    /// * `namespace`: Resource's namespace to get logs for.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_get_logs_error(
        event_details: EventDetails,
        selector: String,
        namespace: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error, unable to retrieve logs for pod with selector `{}` in namespace `{}`.",
            selector, namespace
        );

        EngineError::new(
            event_details,
            Tag::K8sGetLogs,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes get events.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `namespace`: Resource's namespace to get json events for.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_get_json_events(
        event_details: EventDetails,
        namespace: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!("Error, unable to retrieve events in namespace `{}`.", namespace);

        EngineError::new(
            event_details,
            Tag::K8sGetLogs,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes describe.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `selector`: Selector to get description for.
    /// * `namespace`: Resource's namespace to get description for.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_describe(
        event_details: EventDetails,
        selector: String,
        namespace: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error, unable to describe pod with selector `{}` in namespace `{}`.",
            selector, namespace
        );

        EngineError::new(
            event_details,
            Tag::K8sDescribe,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes history.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `namespace`: Resource's namespace to get history for.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_history(event_details: EventDetails, namespace: String, raw_error: CommandError) -> EngineError {
        let message = format!("Error, unable to get history in namespace `{}`.", namespace);

        EngineError::new(
            event_details,
            Tag::K8sHistory,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes namespace creation issue.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `namespace`: Resource's namespace to get history for.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_create_namespace(
        event_details: EventDetails,
        namespace: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!("Error, unable to create namespace `{}`.", namespace);

        EngineError::new(
            event_details,
            Tag::K8sCannotCreateNamespace,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes pod not being ready.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `selector`: Selector to get description for.
    /// * `namespace`: Resource's namespace to get history for.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_pod_not_ready(
        event_details: EventDetails,
        selector: String,
        namespace: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error, pod with selector `{}` in namespace `{}` is not ready.",
            selector, namespace
        );

        EngineError::new(
            event_details,
            Tag::K8sPodIsNotReady,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes node not being ready with the requested version.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `requested_version`: Requested version.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_node_not_ready_with_requested_version(
        event_details: EventDetails,
        requested_version: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error, node is not ready with the requested version `{}`.",
            requested_version
        );

        EngineError::new(
            event_details,
            Tag::K8sNodeIsNotReadyWithTheRequestedVersion,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes node not being ready.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_k8s_node_not_ready(event_details: EventDetails, raw_error: CommandError) -> EngineError {
        let message = "Error, node is not ready.";

        EngineError::new(
            event_details,
            Tag::K8sNodeIsNotReady,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for kubernetes not being able to get crash looping pods.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `missing_binary_name`: Name of the missing required binary.
    pub fn new_missing_required_binary(event_details: EventDetails, missing_binary_name: String) -> EngineError {
        let message = format!("`{}` binary is required but was not found.", missing_binary_name);

        EngineError::new(
            event_details,
            Tag::CannotFindRequiredBinary,
            message.to_string(),
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Creates new error for subnets count not being even. Subnets count should be even to get the same number as private and public.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `zone_name`: Number of subnets.
    /// * `subnets_count`: Number of subnets.
    pub fn new_subnets_count_is_not_even(
        event_details: EventDetails,
        zone_name: String,
        subnets_count: usize,
    ) -> EngineError {
        let message = format!(
            "Number of subnets for zone `{:?}` should be even but got `{}` subnets.",
            zone_name, subnets_count,
        );

        EngineError::new(
            event_details,
            Tag::SubnetsCountShouldBeEven,
            message.to_string(),
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Creates new error for IAM role which cannot be retrieved or created.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `role_name`: IAM role name which failed to be created.
    /// * `raw_error`: Raw error message.
    pub fn new_cannot_get_or_create_iam_role(
        event_details: EventDetails,
        role_name: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!("Error while getting or creating the role {}.", role_name,);

        EngineError::new(
            event_details,
            Tag::CannotGetOrCreateIamRole,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for cannot generate and copy all files from a directory to another directory.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `from_dir`: Source directory for the copy.
    /// * `to_dir`: Target directory for the copy.
    /// * `raw_error`: Raw error message.
    pub fn new_cannot_copy_files_from_one_directory_to_another(
        event_details: EventDetails,
        from_dir: String,
        to_dir: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error while trying to copy all files from `{}` to `{}`.",
            from_dir, to_dir
        );

        EngineError::new(
            event_details,
            Tag::CannotCopyFilesFromDirectoryToDirectory,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for cannot pause cluster because some tasks are still running on the engine.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    pub fn new_cannot_pause_cluster_tasks_are_running(
        event_details: EventDetails,
        raw_error: Option<CommandError>,
    ) -> EngineError {
        let message = "Can't pause the infrastructure now, some engine jobs are currently running.";

        EngineError::new(
            event_details,
            Tag::CannotPauseClusterTasksAreRunning,
            message.to_string(),
            message.to_string(),
            raw_error,
            None,
            None,
        )
    }

    /// Creates new error for removing an element out of terraform.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `entry`: Entry which failed to be removed out of Terraform.
    /// * `raw_error`: Raw error message.
    pub fn new_terraform_cannot_remove_entry_out(
        event_details: EventDetails,
        entry: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!("Error while trying to remove {} out of terraform state file.", entry);

        EngineError::new(
            event_details,
            Tag::TerraformCannotRemoveEntryOut,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for Terraform state file doesn't exist.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_terraform_state_does_not_exist(event_details: EventDetails, raw_error: CommandError) -> EngineError {
        let message = "No state list exists yet.";

        EngineError::new(
            event_details,
            Tag::TerraformNoStateFileExists,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            Some("This is normal if it's a newly created cluster".to_string()),
        )
    }

    /// Creates new error for Terraform having an issue during Terraform pipeline: init, plan & apply.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_terraform_error_while_executing_pipeline(
        event_details: EventDetails,
        raw_error: CommandError,
    ) -> EngineError {
        let message = "Error while applying Terraform pipeline: init, plan & apply";

        EngineError::new(
            event_details,
            Tag::TerraformErrorWhileExecutingPipeline,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error for Terraform having an issue during Terraform pipeline: init, validate & destroy.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_terraform_error_while_executing_destroy_pipeline(
        event_details: EventDetails,
        raw_error: CommandError,
    ) -> EngineError {
        let message = "Error while applying Terraform pipeline: init, validate & destroy";

        EngineError::new(
            event_details,
            Tag::TerraformErrorWhileExecutingDestroyPipeline,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error while setup Helm charts to deploy.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_helm_charts_setup_error(event_details: EventDetails, raw_error: CommandError) -> EngineError {
        let message = "Error while helm charts setup";

        EngineError::new(
            event_details,
            Tag::HelmChartsSetupError,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error while deploying Helm charts.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_helm_charts_deploy_error(event_details: EventDetails, raw_error: CommandError) -> EngineError {
        let message = "Error while helm charts deployment";

        EngineError::new(
            event_details,
            Tag::HelmChartsDeployError,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error while upgrading Helm charts.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_helm_charts_upgrade_error(event_details: EventDetails, raw_error: CommandError) -> EngineError {
        let message = "Error while helm charts deployment";

        EngineError::new(
            event_details,
            Tag::HelmChartsUpgradeError,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error while uninstalling Helm chart.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `helm_chart`: Helm chart name.
    /// * `raw_error`: Raw error message.
    pub fn new_helm_chart_uninstall_error(
        event_details: EventDetails,
        helm_chart: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!("Error while uninstalling helm chart: `{}`.", helm_chart.to_string());

        EngineError::new(
            event_details,
            Tag::HelmChartUninstallError,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error while trying to get Helm chart history.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `helm_chart`: Helm chart name.
    /// * `namespace`: Namespace.
    /// * `raw_error`: Raw error message.
    pub fn new_helm_chart_history_error(
        event_details: EventDetails,
        helm_chart: String,
        namespace: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error while trying to get helm chart `{}` history in namespace `{}`.",
            helm_chart.to_string(),
            namespace
        );

        EngineError::new(
            event_details,
            Tag::HelmHistoryError,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error while trying to get any available VPC.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_cannot_get_any_available_vpc(event_details: EventDetails, raw_error: CommandError) -> EngineError {
        let message = "Error while trying to get any available VPC.";

        EngineError::new(
            event_details,
            Tag::CannotGetAnyAvailableVPC,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error while trying to get supported versions.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `product_name`: Product name for which we want to get supported versions.
    /// * `raw_error`: Raw error message.
    pub fn new_cannot_get_supported_versions_error(
        event_details: EventDetails,
        product_name: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error while trying to get supported versions for `{}`.",
            product_name.to_string()
        );

        EngineError::new(
            event_details,
            Tag::CannotGetSupportedVersions,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new unsupported version error.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `product_name`: Product name for which version is not supported.
    /// * `raw_error`: Raw error message.
    pub fn new_unsupported_version_error(
        event_details: EventDetails,
        product_name: String,
        version: VersionsNumber,
    ) -> EngineError {
        let message = format!(
            "Error, version `{}` is not supported for `{}`.",
            version.to_string(),
            product_name.to_string()
        );

        EngineError::new(
            event_details,
            Tag::UnsupportedVersion,
            message.to_string(),
            message.to_string(),
            None,
            None,
            None,
        )
    }

    /// Creates new object storage cannot create bucket.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `bucket_name`: Object storage bucket name.
    /// * `raw_error`: Raw error message.
    pub fn new_object_storage_cannot_create_bucket_error(
        event_details: EventDetails,
        bucket_name: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error, cannot create object storage bucket `{}`.",
            bucket_name.to_string(),
        );

        EngineError::new(
            event_details,
            Tag::ObjectStorageCannotCreateBucket,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new object storage cannot put file into bucket.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `bucket_name`: Object storage bucket name.
    /// * `file_name`: File name to be added into the bucket.
    /// * `raw_error`: Raw error message.
    pub fn new_object_storage_cannot_put_file_into_bucket_error(
        event_details: EventDetails,
        bucket_name: String,
        file_name: String,
        raw_error: CommandError,
    ) -> EngineError {
        let message = format!(
            "Error, cannot put file `{}` into object storage bucket `{}`.",
            file_name.to_string(),
            bucket_name.to_string(),
        );

        EngineError::new(
            event_details,
            Tag::ObjectStorageCannotPutFileIntoBucket,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            None,
        )
    }

    /// Creates new error while trying to get cluster.
    ///
    /// Arguments:
    ///
    /// * `event_details`: Error linked event details.
    /// * `raw_error`: Raw error message.
    pub fn new_cannot_get_cluster_error(event_details: EventDetails, raw_error: CommandError) -> EngineError {
        let message = "Error, cannot get cluster.";

        EngineError::new(
            event_details,
            Tag::CannotGetCluster,
            message.to_string(),
            message.to_string(),
            Some(raw_error),
            None,
            Some("Maybe there is a lag and cluster is not yet reported, please retry later.".to_string()),
        )
    }
}
