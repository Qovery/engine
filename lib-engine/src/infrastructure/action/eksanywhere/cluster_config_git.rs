use crate::cmd::git;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, InfrastructureStep};
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::eksanywhere::{EksAnywhere, EksAnywhereGitRepository};
use crate::io_models::application::GitCredentials;
use git2::{Cred, CredentialType};
use std::fs;
use std::path::{Component, Path, PathBuf};
use url::Url;

struct ClusterConfigFetchContext<'a> {
    repository_url: Url,
    commit_id: &'a str,
    repository_file_path: PathBuf,
    destination_path: PathBuf,
    git_credentials: Option<&'a GitCredentials>,
}

pub(super) fn prepare_eks_anywhere_cluster_config(
    cluster: &EksAnywhere,
    logger: &impl InfraLogger,
) -> Result<Option<PathBuf>, Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));
    let Some(fetch_ctx) = prepare_cluster_config_fetch_context(cluster, &event_details)? else {
        return Ok(None);
    };

    log_section_title(logger, "📥", "Cluster config");
    logger.info(format!(
        "Downloading `{}` from Git at commit {}",
        fetch_ctx.repository_file_path.display(),
        fetch_ctx.commit_id
    ));
    info!(
        "Downloading EKS Anywhere cluster YAML {} from {} at commit {}",
        fetch_ctx.repository_file_path.display(),
        fetch_ctx.repository_url,
        fetch_ctx.commit_id
    );

    let file_content = fetch_cluster_config_file_from_git(cluster, &fetch_ctx, &event_details)?;
    save_cluster_config_file(&fetch_ctx.destination_path, file_content, &event_details)?;

    logger.info(format!(
        "✅ Cluster config ready: `{}`.",
        filename_for_user(fetch_ctx.destination_path.as_path())
    ));
    info!("Saved EKS Anywhere cluster YAML to {}", fetch_ctx.destination_path.display());

    Ok(Some(fetch_ctx.destination_path))
}

fn prepare_cluster_config_fetch_context<'a>(
    cluster: &'a EksAnywhere,
    event_details: &EventDetails,
) -> Result<Option<ClusterConfigFetchContext<'a>>, Box<EngineError>> {
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

    let repository_url = required_git_repository_url(git_repository, event_details)?;
    let commit_id = required_git_repository_commit_id(git_repository, event_details)?;
    let repository_file_path = build_repo_relative_file_path(&git_repository.root_path, yaml_file_path)
        .map_err(|e| Box::new(EngineError::new_cannot_read_file(event_details.clone(), e)))?;
    let destination_path = cluster.temp_dir().join("eksanywhere").join(
        build_storage_relative_file_path(yaml_file_path)
            .map_err(|e| Box::new(EngineError::new_cannot_read_file(event_details.clone(), e)))?,
    );

    Ok(Some(ClusterConfigFetchContext {
        repository_url,
        commit_id,
        repository_file_path,
        destination_path,
        git_credentials: git_repository.git_credentials.as_ref(),
    }))
}

fn required_git_repository_url(
    git_repository: &EksAnywhereGitRepository,
    event_details: &EventDetails,
) -> Result<Url, Box<EngineError>> {
    let repository_url = git_repository.url.as_deref().ok_or_else(|| {
        Box::new(EngineError::new_cannot_read_file(
            event_details.clone(),
            CommandError::new_from_safe_message("Missing EKS Anywhere git repository URL".to_string()),
        ))
    })?;

    Url::parse(repository_url).map_err(|e| {
        Box::new(EngineError::new_cannot_read_file(
            event_details.clone(),
            CommandError::new("Invalid EKS Anywhere git repository URL".to_string(), Some(e.to_string()), None),
        ))
    })
}

fn required_git_repository_commit_id<'a>(
    git_repository: &'a EksAnywhereGitRepository,
    event_details: &EventDetails,
) -> Result<&'a str, Box<EngineError>> {
    git_repository.commit_id.as_deref().ok_or_else(|| {
        Box::new(EngineError::new_cannot_read_file(
            event_details.clone(),
            CommandError::new_from_safe_message("Missing EKS Anywhere git repository commit ID".to_string()),
        ))
    })
}

fn fetch_cluster_config_file_from_git(
    cluster: &EksAnywhere,
    fetch_ctx: &ClusterConfigFetchContext,
    event_details: &EventDetails,
) -> Result<Vec<u8>, Box<EngineError>> {
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

    git::fetch_file_at_commit(
        &fetch_ctx.repository_url,
        fetch_ctx.commit_id,
        &fetch_ctx.repository_file_path,
        &repo_checkout_path,
        &git_credentials_callback(fetch_ctx.git_credentials),
    )
    .map_err(|e| {
        Box::new(EngineError::new_builder_clone_repository_error(
            event_details.clone(),
            fetch_ctx.repository_url.to_string(),
            CommandError::new(
                "Cannot download EKS Anywhere cluster YAML from git repository".to_string(),
                Some(e.to_string()),
                None,
            ),
        ))
    })
}

fn save_cluster_config_file(
    destination_path: &Path,
    file_content: Vec<u8>,
    event_details: &EventDetails,
) -> Result<(), Box<EngineError>> {
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

    fs::write(destination_path, file_content).map_err(|e| {
        Box::new(EngineError::new_cannot_write_file(
            event_details.clone(),
            CommandError::new(
                format!("Cannot save EKS Anywhere cluster YAML to {}", destination_path.display()),
                Some(e.to_string()),
                None,
            ),
        ))
    })
}

fn git_credentials_callback<'a>(
    git_credentials: Option<&'a GitCredentials>,
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

fn filename_for_user(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn log_section_title(logger: &impl InfraLogger, icon: &str, title: &str) {
    logger.info("");
    logger.info(format!("***** {icon} {title} *****"));
    logger.info("");
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
