mod cluster_install;
mod helm_charts;

use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::cmd::git;
use crate::errors::{CommandError, EngineError};
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::eksanywhere::cluster_install::install_eks_anywhere_charts;
use crate::infrastructure::action::utils::mk_logger;
use crate::infrastructure::action::{InfraLogger, InfrastructureAction};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhere;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesUpgradeStatus, send_progress_on_long_task};
use git2::{Cred, CredentialType};
use std::fs;
use std::path::{Component, Path, PathBuf};
use url::Url;

impl InfrastructureAction for EksAnywhere {
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        _has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);

        if infra_ctx.context().is_first_cluster_deployment() {
            let error = EngineError::new_unknown(
                self.get_event_details(Infrastructure(InfrastructureStep::CreateError)),
                "Cluster creation is not supported on first install for EKS Anywhere".to_string(),
                Some(CommandError::new_from_safe_message(
                    "first cluster deployment is not supported for EKS Anywhere".to_string(),
                )),
                None,
                None,
            );
            logger.error(error.clone(), None::<&str>);
            return Err(Box::new(error));
        }

        let mut eksctl_version = String::new();
        let mut cmd = QoveryCommand::new("eksctl", &["version"], &[]);
        if cmd
            .exec_with_output(&mut |line| eksctl_version.push_str(&line), &mut |line| {
                warn!("Error while getting `eksctl` version: {}", line)
            })
            .is_err()
            || eksctl_version.trim().is_empty()
        {
            logger.warn("Unable to get `eksctl` version using `eksctl version`.");
        } else {
            logger.info(format!("Using eksctl: {}", eksctl_version.trim()));
        }

        let mut eksctl_anywhere_version = String::new();
        let mut cmd = QoveryCommand::new("eksctl", &["anywhere", "version"], &[]);
        if cmd
            .exec_with_output(&mut |line| eksctl_anywhere_version.push_str(&line), &mut |line| {
                warn!("Error while getting `eksctl anywhere` version: {}", line)
            })
            .is_err()
            || eksctl_anywhere_version.trim().is_empty()
        {
            logger.warn("Unable to get `eksctl anywhere` version using `eksctl anywhere version`.");
        } else {
            logger.info(format!("Using eksctl anywhere: {}", eksctl_anywhere_version.trim()));
        }

        let cluster_config_path = prepare_eks_anywhere_cluster_config(self, &logger)?;
        if let Some(cluster_config_path) = cluster_config_path.as_ref() {
            run_eks_anywhere_upgrade_plan(self, cluster_config_path, &logger)?;
        }

        send_progress_on_long_task(self, Action::Create, || install_eks_anywhere_charts(self, infra_ctx, logger))
    }

    fn pause_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::PauseError)),
        )))
    }

    fn delete_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::DeleteError)),
        )))
    }

    fn upgrade_cluster(
        &self,
        _infra_ctx: &InfrastructureContext,
        _kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::UpgradeError)),
        )))
    }

    fn is_upgrade_required(&self, _infra_ctx: &InfrastructureContext) -> Option<KubernetesUpgradeStatus> {
        // EKS Anywhere lifecycle is driven by the cluster config and `eksctl anywhere` flows.
        // The generic Kubernetes version drift check would otherwise turn a create into an
        // unsupported cluster restart/upgrade path for this provider.
        None
    }
}

fn prepare_eks_anywhere_cluster_config(
    cluster: &EksAnywhere,
    logger: &impl InfraLogger,
) -> Result<Option<PathBuf>, Box<EngineError>> {
    let Some(eks_anywhere_parameters) = cluster
        .options
        .infrastructure_charts_parameters
        .eks_anywhere_parameters
        .as_ref()
    else {
        return Ok(None);
    };
    let Some(git_repository) = eks_anywhere_parameters.git_repository.as_ref() else {
        return Ok(None);
    };
    let Some(yaml_file_path) = eks_anywhere_parameters.yaml_file_path.as_deref() else {
        return Ok(None);
    };

    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));
    let repository_url = git_repository.url.as_deref().ok_or_else(|| {
        Box::new(EngineError::new_cannot_read_file(
            event_details.clone(),
            CommandError::new_from_safe_message("Missing EKS Anywhere git repository URL".to_string()),
        ))
    })?;
    let commit_id = git_repository.commit_id.as_deref().ok_or_else(|| {
        Box::new(EngineError::new_cannot_read_file(
            event_details.clone(),
            CommandError::new_from_safe_message("Missing EKS Anywhere git repository commit ID".to_string()),
        ))
    })?;
    let repository_url = Url::parse(repository_url).map_err(|e| {
        Box::new(EngineError::new_cannot_read_file(
            event_details.clone(),
            CommandError::new("Invalid EKS Anywhere git repository URL".to_string(), Some(e.to_string()), None),
        ))
    })?;
    let repository_file_path = build_repo_relative_file_path(&git_repository.root_path, yaml_file_path)
        .map_err(|e| Box::new(EngineError::new_cannot_read_file(event_details.clone(), e)))?;
    let destination_path = cluster.temp_dir().join("eksanywhere").join(
        build_storage_relative_file_path(yaml_file_path)
            .map_err(|e| Box::new(EngineError::new_cannot_read_file(event_details.clone(), e)))?,
    );

    logger.info(format!(
        "📥 Downloading EKS Anywhere cluster YAML {} from {} at commit {}",
        repository_file_path.display(),
        repository_url,
        commit_id
    ));

    let temp_repo_dir = tempfile::tempdir_in(cluster.temp_dir()).map_err(|e| {
        Box::new(EngineError::new_cannot_create_file(
            event_details.clone(),
            CommandError::new(
                "Cannot create temporary directory for EKS Anywhere git download".to_string(),
                Some(e.to_string()),
                None,
            ),
        ))
    })?;
    let repo_checkout_path = temp_repo_dir.path().join("repo");
    let file_content = git::fetch_file_at_commit(
        &repository_url,
        commit_id,
        &repository_file_path,
        &repo_checkout_path,
        &git_credentials_callback(git_repository.git_credentials.as_ref()),
    )
    .map_err(|e| {
        Box::new(EngineError::new_builder_clone_repository_error(
            event_details.clone(),
            repository_url.to_string(),
            CommandError::new(
                "Cannot download EKS Anywhere cluster YAML from git repository".to_string(),
                Some(e.to_string()),
                None,
            ),
        ))
    })?;

    if let Some(parent_dir) = destination_path.parent() {
        fs::create_dir_all(parent_dir).map_err(|e| {
            Box::new(EngineError::new_cannot_create_file(
                event_details.clone(),
                CommandError::new(
                    format!(
                        "Cannot create destination directory for EKS Anywhere cluster YAML at {}",
                        parent_dir.display()
                    ),
                    Some(e.to_string()),
                    None,
                ),
            ))
        })?;
    }

    fs::write(&destination_path, file_content).map_err(|e| {
        Box::new(EngineError::new_cannot_write_file(
            event_details,
            CommandError::new(
                format!("Cannot save EKS Anywhere cluster YAML to {}", destination_path.display()),
                Some(e.to_string()),
                None,
            ),
        ))
    })?;

    logger.info(format!("Saved EKS Anywhere cluster YAML to {}", destination_path.display()));

    Ok(Some(destination_path))
}

fn run_eks_anywhere_upgrade_plan(
    cluster: &EksAnywhere,
    cluster_config_path: &Path,
    logger: &impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));
    let cluster_config_path = cluster_config_path.to_string_lossy().to_string();
    let kubeconfig_path = cluster.kubeconfig_local_file_path().to_string_lossy().to_string();
    let envs = [("KUBECONFIG", kubeconfig_path.as_str())];
    let args = [
        "anywhere",
        "upgrade",
        "plan",
        "cluster",
        "-f",
        cluster_config_path.as_str(),
        "--kubeconfig",
        kubeconfig_path.as_str(),
    ];

    logger.info(format!(
        "Running `eksctl anywhere upgrade plan cluster` against {}",
        cluster_config_path
    ));

    let mut cmd = QoveryCommand::new("eksctl", &args, &envs);
    cmd.set_current_dir(cluster.temp_dir());

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    cmd.exec_with_output(&mut |line| stdout.push(line), &mut |line| stderr.push(line))
        .map_err(|e| {
            Box::new(EngineError::new_unknown(
                event_details,
                "EKS Anywhere upgrade plan failed".to_string(),
                Some(CommandError::new(
                    "Cannot run `eksctl anywhere upgrade plan cluster`".to_string(),
                    Some(if stderr.is_empty() {
                        e.to_string()
                    } else {
                        stderr.join("\n")
                    }),
                    None,
                )),
                None,
                None,
            ))
        })?;

    for line in stdout {
        logger.info(line);
    }

    Ok(())
}

fn git_credentials_callback<'a>(
    git_credentials: Option<&'a crate::io_models::application::GitCredentials>,
) -> impl Fn(&str) -> Vec<(CredentialType, Cred)> + 'a {
    move |_user| {
        let mut creds: Vec<(CredentialType, Cred)> = Vec::with_capacity(1);
        if let Some(git_credentials) = git_credentials
            && let Ok(cred) = Cred::userpass_plaintext(&git_credentials.login, &git_credentials.access_token)
        {
            creds.push((CredentialType::USER_PASS_PLAINTEXT, cred));
        }

        creds
    }
}

fn build_repo_relative_file_path(root_path: &str, yaml_file_path: &str) -> Result<PathBuf, CommandError> {
    let mut path = PathBuf::new();
    append_relative_components(&mut path, root_path, "root_path")?;
    append_relative_components(&mut path, yaml_file_path, "yaml_file_path")?;
    Ok(path)
}

fn build_storage_relative_file_path(yaml_file_path: &str) -> Result<PathBuf, CommandError> {
    let mut path = PathBuf::new();
    append_relative_components(&mut path, yaml_file_path, "yaml_file_path")?;
    Ok(path)
}

fn append_relative_components(target: &mut PathBuf, raw_path: &str, field_name: &str) -> Result<(), CommandError> {
    for component in Path::new(raw_path).components() {
        match component {
            Component::Normal(part) => target.push(part),
            Component::CurDir | Component::RootDir => {}
            Component::ParentDir | Component::Prefix(_) => {
                return Err(CommandError::new_from_safe_message(format!(
                    "Invalid EKS Anywhere {field_name}: parent directory segments are not allowed"
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_repo_relative_file_path, build_storage_relative_file_path};
    use crate::errors::ErrorMessageVerbosity;
    use std::path::PathBuf;

    #[test]
    fn should_build_repo_relative_file_path_from_root_and_yaml_paths() {
        let path = build_repo_relative_file_path("/", "/clusters/cluster-a.yaml").expect("path should be valid");
        assert_eq!(path, PathBuf::from("clusters/cluster-a.yaml"));

        let path = build_repo_relative_file_path("/manifests", "cluster.yaml").expect("path should be valid");
        assert_eq!(path, PathBuf::from("manifests/cluster.yaml"));
    }

    #[test]
    fn should_reject_parent_directory_in_repo_relative_file_path() {
        let err = build_repo_relative_file_path("/", "../cluster.yaml").expect_err("path should be rejected");
        assert!(err.message(ErrorMessageVerbosity::SafeOnly).contains("yaml_file_path"));
    }

    #[test]
    fn should_build_storage_relative_file_path() {
        let path = build_storage_relative_file_path("/clusters/cluster-a.yaml").expect("path should be valid");
        assert_eq!(path, PathBuf::from("clusters/cluster-a.yaml"));
    }
}
