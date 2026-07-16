use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use crate::cmd::command::{CommandError, CommandKiller, ExecutableCommand, QoveryCommand};
use crate::infrastructure::models::build_platform::{BuildError, GitCmd};
use git2::ErrorCode::Auth;
use git2::ResetType::Hard;
use git2::build::CheckoutBuilder;
use git2::{
    AutotagOption, CertificateCheckStatus, Cred, CredentialType, Error, FetchOptions, Object, RemoteCallbacks,
    Repository, SubmoduleUpdateOptions, opts,
};
use tracing::field::debug;
use url::Url;

pub fn git_initialize_opts(
    git_opts_set_server_connection_timeout_in_milliseconds: Duration,
    git_opts_set_server_timeout_in_milliseconds: Duration,
) {
    unsafe {
        if let Err(err) = opts::set_server_connect_timeout_in_milliseconds(
            git_opts_set_server_connection_timeout_in_milliseconds.as_millis() as i32,
        ) {
            debug(format!("Cannot set git_server_connect_timeout: {err}"));
        }
        if let Err(err) =
            opts::set_server_timeout_in_milliseconds(git_opts_set_server_timeout_in_milliseconds.as_millis() as i32)
        {
            debug(format!("Cannot set git_server_timeout: {err}"));
        }
    }
}

// Clones a repository at a given tag. clone_at_commit passes the raw tag/commit string as the
// fetch refspec — for hierarchical tag names like "aws/postgres/17/1.0.1" no local ref ends up
// written.
// This function uses the explicit "refs/tags/<tag>:refs/tags/<tag>"
// mapping so the tag lands in local refs/tags/<tag> and revparse_single resolves it.
pub fn clone_at_tag<P>(
    repository_url: &Url,
    tag: &str,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
) -> Result<(), BuildError>
where
    P: AsRef<Path>,
{
    let tag_ref = format!("refs/tags/{tag}");
    let tag_refspec = format!("+{tag_ref}:{tag_ref}");
    let repo =
        fetch(repository_url, into_dir, get_credentials, &tag_refspec).map_err(|error| BuildError::GitError {
            application: tag.into(),
            git_cmd: GitCmd::Fetch,
            context: format!("url: {repository_url}/ tag: {tag}"),
            raw_error: error,
        })?;
    let _ = checkout(&repo, &tag_ref).map_err(|error| BuildError::GitError {
        application: "".to_string(),
        git_cmd: GitCmd::Checkout,
        context: tag.to_string(),
        raw_error: error,
    })?;
    Ok(())
}

// credential.helper shim: reads creds from GIT_USER/GIT_PASSWORD env so the token never lands
// in argv or in .git/config (same pattern as GitLfs, see cmd/git_lfs.rs).
const CREDENTIAL_HELPER: &str = "!f() { echo \"username=${GIT_USER}\"; echo \"password=${GIT_PASSWORD}\"; }; f";

#[derive(thiserror::Error, Debug)]
pub enum GitCliError {
    #[error("git terminated with an execution error: {raw_error:?}")]
    ExecutionError { raw_error: std::io::Error },

    #[error("git terminated with a non success exit status: {exit_status:?}")]
    ExitStatusError { exit_status: ExitStatus },

    #[error("git aborted due to user cancel request: {raw_error_message}")]
    Aborted { raw_error_message: String },

    #[error("git command timed out: {raw_error_message}")]
    Timeout { raw_error_message: String },
}

impl From<CommandError> for GitCliError {
    fn from(value: CommandError) -> Self {
        match value {
            CommandError::TimeoutError(msg) => GitCliError::Timeout { raw_error_message: msg },
            CommandError::Killed(msg) => GitCliError::Aborted { raw_error_message: msg },
            CommandError::ExitStatusError(status) => GitCliError::ExitStatusError { exit_status: status },
            CommandError::ExecutionError(err) => GitCliError::ExecutionError { raw_error: err },
        }
    }
}

impl From<std::io::Error> for GitCliError {
    fn from(value: std::io::Error) -> Self {
        GitCliError::ExecutionError { raw_error: value }
    }
}

fn git_cli_exec(args: &[&str], envs: &[(&str, &str)], cmd_killer: &CommandKiller) -> Result<(), GitCliError> {
    // Fail fast instead of blocking on an interactive credential/passphrase prompt.
    let mut all_envs: Vec<(&str, &str)> = vec![("GIT_TERMINAL_PROMPT", "0")];
    all_envs.extend_from_slice(envs);

    QoveryCommand::new("git", args, &all_envs).exec_with_abort(
        &mut |line| info!("{line}"),
        // git writes progress ("remote: Counting objects…", "Receiving objects…") to stderr;
        // real errors surface via a non-zero exit status → GitCliError, so keep this at debug.
        &mut |line| debug!("{line}"),
        cmd_killer,
    )?;
    Ok(())
}

/// Clones only the `sparse_path` subfolder of a repo at a tag, via the git CLI.
///
/// Uses partial clone (`--filter=blob:none`) + cone sparse-checkout so only the tagged leaf
/// folder's blobs are downloaded — libgit2 can't do this. `--filter`/`--depth` are skipped for
/// `file://` (local transport rejects them; test-only). Credentials go through the
/// GIT_USER/GIT_PASSWORD env + credential-helper shim so the token stays out of argv and config.
///
/// The caller is responsible for falling back to `clone_at_tag` if the server rejects `--filter`.
pub fn sparse_clone_at_tag(
    repository_url: &Url,
    tag: &str,
    sparse_path: &str,
    into_dir: &Path,
    credentials: Option<(&str, &str)>,
    cmd_killer: &CommandKiller,
) -> Result<(), GitCliError> {
    #[cfg(not(test))]
    if repository_url.scheme() != "https" {
        return Err(GitCliError::ExecutionError {
            raw_error: std::io::Error::other("Repository URL have to start with https://"),
        });
    }

    if into_dir.exists() {
        let _ = std::fs::remove_dir_all(into_dir);
    }

    let dir = into_dir.to_string_lossy();
    let dir = dir.as_ref();
    let url = repository_url.as_str();
    let tag_ref = format!("refs/tags/{tag}");
    let tag_refspec = format!("+{tag_ref}:{tag_ref}");
    let envs: Vec<(&str, &str)> = match credentials {
        Some((login, token)) => vec![("GIT_USER", login), ("GIT_PASSWORD", token)],
        None => vec![],
    };

    git_cli_exec(&["init", "-q", dir], &[], cmd_killer)?;
    git_cli_exec(&["-C", dir, "remote", "add", "origin", url], &[], cmd_killer)?;
    if credentials.is_some() {
        git_cli_exec(&["-C", dir, "config", "credential.helper", CREDENTIAL_HELPER], &[], cmd_killer)?;
    }
    git_cli_exec(&["-C", dir, "config", "remote.origin.promisor", "true"], &[], cmd_killer)?;
    git_cli_exec(
        &["-C", dir, "config", "remote.origin.partialclonefilter", "blob:none"],
        &[],
        cmd_killer,
    )?;
    git_cli_exec(&["-C", dir, "sparse-checkout", "init", "--cone"], &[], cmd_killer)?;
    git_cli_exec(&["-C", dir, "sparse-checkout", "set", sparse_path], &[], cmd_killer)?;

    let mut fetch_args = vec!["-C", dir, "fetch"];
    // Local transport supports neither shallow nor partial clone; skip for file:// (test-only).
    if repository_url.scheme() != "file" {
        fetch_args.extend_from_slice(&["--depth", "1", "--filter=blob:none"]);
    }
    fetch_args.extend_from_slice(&["origin", &tag_refspec]);
    git_cli_exec(&fetch_args, &envs, cmd_killer)?;

    git_cli_exec(&["-C", dir, "checkout", &tag_ref], &envs, cmd_killer)?;

    Ok(())
}

pub fn clone_at_commit<P>(
    repository_url: &Url,
    commit_id: &str,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
    skip_submodules: bool,
) -> Result<(), BuildError>
where
    P: AsRef<Path>,
{
    let repo = fetch(repository_url, into_dir, get_credentials, commit_id).map_err(|error| BuildError::GitError {
        application: "".to_string(),
        git_cmd: GitCmd::Fetch,
        context: format!("url: {repository_url}/ commit id: {commit_id}"),
        raw_error: error,
    })?;
    // position the repo at the correct commit
    let _ = checkout(&repo, commit_id).map_err(|error| BuildError::GitError {
        application: "".to_string(),
        git_cmd: GitCmd::Checkout,
        context: commit_id.to_string(),
        raw_error: error,
    })?;

    // check submodules if needed
    if !skip_submodules {
        let submodules = repo.submodules().map_err(|error| BuildError::GitError {
            application: "".to_string(),
            git_cmd: GitCmd::SubmoduleUpdate,
            context: "".to_string(),
            raw_error: error,
        })?;
        if !submodules.is_empty() {
            // for auth
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(authentication_callback(&get_credentials));
            callbacks.certificate_check(|_, _| Ok(CertificateCheckStatus::CertificateOk));

            let mut fo = FetchOptions::new();
            fo.remote_callbacks(callbacks);
            let mut opts = SubmoduleUpdateOptions::new();
            opts.fetch(fo);

            for mut submodule in submodules {
                info!("getting submodule {:?} from {:?}", submodule.name(), submodule.url());
                submodule
                    .update(true, Some(&mut opts))
                    .map_err(|error| BuildError::GitError {
                        application: "".to_string(),
                        git_cmd: GitCmd::SubmoduleUpdate,
                        context: submodule.name().unwrap_or("").to_string(),
                        raw_error: error,
                    })?
            }
        }
    }

    Ok(())
}

pub fn fetch_file_at_commit<P>(
    repository_url: &Url,
    commit_id: &str,
    file_path: &Path,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
) -> Result<Vec<u8>, BuildError>
where
    P: AsRef<Path>,
{
    let repo = fetch(repository_url, into_dir, get_credentials, commit_id).map_err(|error| BuildError::GitError {
        application: "".to_string(),
        git_cmd: GitCmd::Fetch,
        context: format!("url: {repository_url}/ commit id: {commit_id}"),
        raw_error: error,
    })?;

    file_content_at_commit(&repo, commit_id, file_path).map_err(|error| BuildError::GitError {
        application: "".to_string(),
        git_cmd: GitCmd::Checkout,
        context: format!("commit id: {commit_id}, file path: {}", file_path.display()),
        raw_error: error,
    })
}

// Credentials callback is called endlessly until the server return Auth Ok (or a definitive error)
// If auth is denied, it up to us to return a new credential to try different auth method
// or an error to specify that we have exhausted everything we are able to provide
fn authentication_callback(
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
) -> impl FnMut(&str, Option<&str>, CredentialType) -> Result<Cred, Error> + '_ {
    let mut current_credentials: (String, Vec<(CredentialType, Cred)>) = ("".into(), vec![]);

    move |remote_url, username_from_url, allowed_types| {
        // If we have changed remote, reset our available auth methods
        if remote_url != current_credentials.0 {
            current_credentials = (remote_url.to_string(), get_credentials(username_from_url.unwrap_or("git")));
        }
        let auth_methods = &mut current_credentials.1;

        // Try all the auth method until one match allowed_types
        loop {
            let (cred_type, credential) = match auth_methods.pop() {
                Some(cred) => cred,
                None => {
                    let msg = format!(
                        "Invalid authentication: Exhausted all available auth method to fetch repository {remote_url}"
                    );
                    let mut error = Error::from_str(msg.as_str());
                    error.set_code(Auth);
                    return Err(error);
                }
            };

            if allowed_types.contains(cred_type) {
                return Ok(credential);
            }
        }
    }
}

fn checkout<'a>(repo: &'a Repository, commit_id: &'a str) -> Result<Object<'a>, Error> {
    let obj = repo.revparse_single(commit_id).map_err(|err| {
        let repo_url = repo
            .find_remote("origin")
            .map(|remote| remote.url().unwrap_or_default().to_string())
            .unwrap_or_default();
        let msg = format!(
            "Unable to use git object commit ID {} on repository {}: {}",
            &commit_id, &repo_url, &err
        );
        Error::from_str(&msg)
    })?;

    // Specify some options to be sure repository is in a clean state
    let mut checkout_opts = CheckoutBuilder::new();
    checkout_opts.force().remove_ignored(true).remove_untracked(true);

    repo.reset(&obj, Hard, Some(&mut checkout_opts))?;
    Ok(obj)
}

fn file_content_at_commit(repo: &Repository, commit_id: &str, file_path: &Path) -> Result<Vec<u8>, Error> {
    let obj = repo.revparse_single(commit_id).map_err(|err| {
        let repo_url = repo
            .find_remote("origin")
            .map(|remote| remote.url().unwrap_or_default().to_string())
            .unwrap_or_default();
        let msg = format!(
            "Unable to use git object commit ID {} on repository {}: {}",
            commit_id, repo_url, err
        );
        Error::from_str(&msg)
    })?;
    let commit = obj.peel_to_commit()?;
    let tree = commit.tree()?;
    let entry = tree.get_path(file_path).map_err(|err| {
        Error::from_str(&format!(
            "Unable to find file {} in commit {}: {}",
            file_path.display(),
            commit_id,
            err
        ))
    })?;
    let blob = repo.find_blob(entry.id()).map_err(|err| {
        Error::from_str(&format!(
            "Unable to read blob for file {} in commit {}: {}",
            file_path.display(),
            commit_id,
            err
        ))
    })?;

    Ok(blob.content().to_vec())
}

fn fetch<P>(
    repository_url: &Url,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
    commit_id: &str,
) -> Result<Repository, Error>
where
    P: AsRef<Path>,
{
    #[cfg(not(feature = "test-git-container"))]
    {
        // Allow file:// in unit tests so tests can use a local repo without a network round-trip.
        #[cfg(not(test))]
        if repository_url.scheme() != "https" {
            return Err(Error::from_str("Repository URL have to start with https://"));
        }
        #[cfg(test)]
        if repository_url.scheme() != "https" && repository_url.scheme() != "file" {
            return Err(Error::from_str("Repository URL have to start with https://"));
        }
    }
    #[cfg(feature = "test-git-container")]
    {
        // http is allowed only for tests (git server on testcontainer)
        if !(repository_url.scheme() == "https" || repository_url.scheme() == "http") {
            // if repository_url.scheme() != "https" {
            return Err(Error::from_str("Repository URL have to start with https://"));
        }
    }

    // Prepare authentication callbacks.
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(authentication_callback(&get_credentials));

    // Prepare fetch options.
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(callbacks);
    // Local transport doesn't support shallow fetches; skip depth for file:// (test-only path).
    if repository_url.scheme() != "file" {
        fo.depth(1);
    }
    fo.update_fetchhead(false);
    fo.download_tags(AutotagOption::None);

    // Get our repository
    if into_dir.as_ref().exists() {
        let _ = std::fs::remove_dir_all(into_dir.as_ref());
    }

    #[cfg(not(feature = "test-git-container"))]
    {
        let repo = Repository::init(into_dir.as_ref())?;
        remote_fetch(repository_url, &commit_id, &mut fo, &repo)?;
        Ok(repo)
    }
    #[cfg(feature = "test-git-container")]
    {
        use git2::build::RepoBuilder;

        // git clone is allowed only for tests (git server on testcontainer)
        let mut repo = Repository::init(into_dir.as_ref())?;
        let fetch_status = remote_fetch(repository_url, &commit_id, &mut fo, &repo);
        if fetch_status.is_err() {
            std::fs::remove_dir_all(repo.path()).unwrap_or_default();
            repo = RepoBuilder::new()
                .fetch_options(fo)
                .clone(repository_url.as_str(), into_dir.as_ref())?;
        }
        Ok(repo)
    }
}

fn remote_fetch(
    repository_url: &Url,
    commit_id: &&str,
    mut fo: &mut FetchOptions,
    repo: &Repository,
) -> Result<(), Error> {
    let mut remote = repo.remote("origin", repository_url.as_str())?;
    remote.fetch(&[commit_id], Some(&mut fo), None)?;
    remote.disconnect()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cmd::command::CommandKiller;
    use crate::cmd::git::{
        checkout, clone_at_commit, clone_at_tag, fetch, file_content_at_commit, sparse_clone_at_tag,
    };
    use base64::Engine;
    use base64::engine::general_purpose;
    use git2::{Cred, CredentialType, Repository, Signature};
    use std::fs;
    use std::path::{Path, PathBuf};
    use url::Url;
    use uuid::Uuid;

    struct DirectoryForTests {
        path: String,
    }

    impl DirectoryForTests {
        /// Generates a dir path with a random suffix.
        /// Since tests are runs in parallel and eventually on the same node, it will avoid having directories collisions between tests running on the same node.
        pub fn new_with_random_suffix(base_path: String) -> Self {
            DirectoryForTests {
                path: format!("{}_{}", base_path, Uuid::new_v4()),
            }
        }

        pub fn path(&self) -> String {
            self.path.to_string()
        }
    }

    impl Drop for DirectoryForTests {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_git_fetch_repository() {
        let repo_dir = DirectoryForTests::new_with_random_suffix("/tmp/tmp_git".to_string());
        let repo_path = repo_dir.path();
        let commit = "9a9c1f4373c8128151a9def9ea3d838fa2ed33e8";

        // We only allow https:// at the moment
        let repo = fetch(
            &Url::parse("ssh://git@github.com/Qovery/engine.git").unwrap(),
            &repo_path,
            &|_| vec![],
            commit,
        );
        assert!(matches!(repo, Err(e) if e.message().contains("https://")));

        // Repository must be empty
        let repo = fetch(
            &Url::parse("https://github.com/Qovery/engine-testing.git").unwrap(),
            &repo_path,
            &|_| vec![],
            commit,
        );
        assert!(repo.is_ok()); // clone makes sure to empty the directory

        // Working case
        {
            let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_clone".to_string());
            let repo = fetch(
                &Url::parse("https://github.com/Qovery/engine-testing.git").unwrap(),
                clone_dir.path(),
                &|_| vec![],
                commit,
            );
            assert!(matches!(repo, Ok(_repo)));
        }

        // Invalid credentials
        {
            let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_clone".to_string());
            let get_credentials = |_: &str| {
                vec![(
                    CredentialType::USER_PASS_PLAINTEXT,
                    Cred::userpass_plaintext("FAKE", "FAKE").unwrap(),
                )]
            };
            let repo = fetch(
                &Url::parse("https://gitlab.com/qovery/q-core.git").unwrap(),
                clone_dir.path(),
                &get_credentials,
                commit,
            );
            assert!(matches!(repo, Err(repo) if repo.message().contains("authentication")));
        }

        /*
        // Test with bitbucket credentials
        // This is a toy account feel free to trash it
        {
            let clone_dir = DirectoryToDelete {
                path: "/tmp/engine_test_clone",
            };

            let get_credentials = || {
                vec![
                    (
                        CredentialType::USER_PASS_PLAINTEXT,
                        Cred::userpass_plaintext("{a45d7986-7994-43a9-a961-044799e761d7}", "3uDbu-i3kdanLRV6iSSWzWDJf4oUQu2hbUQ250DMezFEkkmz3oxPRiAcj7RuLrNgmKu7qx6XA820uvvyfUCdx06bt4VCaOZQkEwkWVksNpAkPE1Lw8gPcnEK").unwrap(),
                    ),
                ]
            };
            let repo = clone(
                "https://bitbucket.org/erebe/attachment-parser.git",
                clone_dir.path,
                &get_credentials,
            );
            assert!(matches!(repo, Ok(_)));
        }
        */
    }

    #[test]
    fn test_clone_at_tag_resolves_hierarchical_tag() {
        // Uses a local file:// repo so the test never breaks when a remote tag is deleted.
        let remote_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_tag_remote".to_string());
        let tag = "aws/postgres/17/1.0.1";

        // Build a local repo with the hierarchical tag.
        {
            let remote_repo = Repository::init(remote_dir.path()).expect("init remote");
            fs::create_dir_all(format!("{}/aws/postgres/17", remote_dir.path())).expect("mkdir");
            fs::write(format!("{}/aws/postgres/17/qbm.yml", remote_dir.path()), "version: 1\n").expect("write qbm.yml");
            let mut index = remote_repo.index().expect("index");
            index
                .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
                .expect("add all");
            index.write().expect("write index");
            let tree_id = index.write_tree().expect("write tree");
            let tree = remote_repo.find_tree(tree_id).expect("find tree");
            let sig = Signature::now("Test", "test@test.com").expect("sig");
            let commit_id = remote_repo
                .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .expect("commit");
            let commit_obj = remote_repo.find_object(commit_id, None).expect("find object");
            remote_repo.tag_lightweight(tag, &commit_obj, false).expect("tag");
        }

        let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_clone_at_tag".to_string());
        let remote_url = Url::parse(&format!("file://{}", remote_dir.path())).unwrap();

        let result = clone_at_tag(&remote_url, tag, clone_dir.path(), &|_| vec![]);
        assert!(result.is_ok(), "clone_at_tag failed: {:?}", result.err());

        // Working tree should contain the catalog path for this tag.
        let qbm_path = Path::new(&clone_dir.path()).join("aws/postgres/17/qbm.yml");
        assert!(qbm_path.exists(), "expected blueprint file at {qbm_path:?} after tag checkout");

        // The local tag ref should now exist so a subsequent revparse can resolve it.
        let repo = Repository::open(clone_dir.path()).expect("repo should open");
        let tag_ref = repo
            .find_reference(&format!("refs/tags/{tag}"))
            .expect("local tag ref should exist after clone_at_tag");
        assert!(tag_ref.target().is_some(), "tag ref should point to an object");
    }

    // Builds a local repo with two blueprint leaf folders and a hierarchical tag, returns its path.
    // file:// transport rejects --filter/--depth, so sparse_clone_at_tag exercises sparse-checkout
    // narrowing only (blob filtering is covered by the real-remote E2E, not unit tests).
    fn build_two_leaf_remote(remote: &DirectoryForTests, tag: &str) {
        let repo = Repository::init(remote.path()).expect("init remote");
        for leaf in ["aws/postgres/17", "gcp/cloud-sql/15"] {
            fs::create_dir_all(format!("{}/{leaf}", remote.path())).expect("mkdir");
            fs::write(format!("{}/{leaf}/qbm.yml", remote.path()), "version: 1\n").expect("write qbm.yml");
        }
        let mut index = repo.index().expect("index");
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .expect("add all");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let sig = Signature::now("Test", "test@test.com").expect("sig");
        let commit_id = repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("commit");
        let commit_obj = repo.find_object(commit_id, None).expect("find object");
        repo.tag_lightweight(tag, &commit_obj, false).expect("tag");
    }

    #[test]
    fn test_sparse_clone_at_tag_narrows_to_leaf() {
        let remote_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_sparse_remote".to_string());
        let tag = "aws/postgres/17/1.0.1";
        build_two_leaf_remote(&remote_dir, tag);

        let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_sparse_clone".to_string());
        let remote_url = Url::parse(&format!("file://{}", remote_dir.path())).unwrap();

        let result = sparse_clone_at_tag(
            &remote_url,
            tag,
            "aws/postgres/17",
            Path::new(&clone_dir.path()),
            None,
            &CommandKiller::never(),
        );
        assert!(result.is_ok(), "sparse_clone_at_tag failed: {:?}", result.err());

        // Target leaf materialized...
        let target = Path::new(&clone_dir.path()).join("aws/postgres/17/qbm.yml");
        assert!(target.exists(), "expected leaf blueprint at {target:?}");
        // ...sibling leaf narrowed out by sparse-checkout.
        let sibling = Path::new(&clone_dir.path()).join("gcp/cloud-sql/15/qbm.yml");
        assert!(
            !sibling.exists(),
            "sibling leaf {sibling:?} should be excluded by sparse-checkout"
        );
    }

    #[test]
    fn test_sparse_clone_at_tag_errors_on_missing_tag() {
        // A missing tag makes the fetch fail — the caller relies on this Err to fall back to a
        // full libgit2 clone.
        let remote_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_sparse_missing".to_string());
        build_two_leaf_remote(&remote_dir, "aws/postgres/17/1.0.1");

        let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_sparse_missing_clone".to_string());
        let remote_url = Url::parse(&format!("file://{}", remote_dir.path())).unwrap();

        let result = sparse_clone_at_tag(
            &remote_url,
            "aws/postgres/17/9.9.9",
            "aws/postgres/17",
            Path::new(&clone_dir.path()),
            None,
            &CommandKiller::never(),
        );
        assert!(result.is_err(), "expected error for a non-existent tag");
    }

    #[test]
    fn test_git_checkout() {
        let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_checkout".to_string());
        let valid_commit = "9a9c1f4373c8128151a9def9ea3d838fa2ed33e8";
        let repo = fetch(
            &Url::parse("https://github.com/Qovery/engine-testing.git").unwrap(),
            clone_dir.path(),
            &|_| vec![],
            valid_commit,
        )
        .unwrap();

        // Invalid commit for this repository
        let check = checkout(&repo, "c2c2101f8e4c4ffadb326dc440ba8afb4aeb1310");
        assert!(matches!(check, Err(_err)));

        // Valid commit
        let check = checkout(&repo, valid_commit);
        assert!(check.is_ok());
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), valid_commit);
    }

    #[test]
    fn test_git_file_content_at_commit() {
        let repo_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_git_file".to_string());
        let repo = Repository::init(repo_dir.path()).expect("repository should initialize");
        let workdir = repo.workdir().expect("workdir should exist").to_path_buf();

        fs::create_dir_all(workdir.join("clusters")).expect("directory should be created");
        fs::write(
            workdir.join("clusters/cluster-a.yaml"),
            "kind: Cluster\nmetadata:\n  name: test\n",
        )
        .expect("file should be written");

        let mut index = repo.index().expect("index should open");
        index
            .add_path(Path::new("clusters/cluster-a.yaml"))
            .expect("file should be staged");
        index.write().expect("index should be written");
        let tree_id = index.write_tree().expect("tree should be written");
        let tree = repo.find_tree(tree_id).expect("tree should be found");
        let signature = Signature::now("Qovery", "qovery@example.com").expect("signature should be created");
        let commit_id = repo
            .commit(Some("HEAD"), &signature, &signature, "initial commit", &tree, &[])
            .expect("commit should be created");

        let content = file_content_at_commit(&repo, &commit_id.to_string(), Path::new("clusters/cluster-a.yaml"))
            .expect("file should be read from commit");

        assert_eq!(
            String::from_utf8(content).expect("content should be utf-8"),
            "kind: Cluster\nmetadata:\n  name: test\n"
        );
    }

    #[test]
    fn test_git_submodule_with_ssh_key() {
        // Unique Key only valid for the submodule and in read access only
        // https://github.com/Qovery/dumb-logger/settings/keys
        let commit_id = "9a9c1f4373c8128151a9def9ea3d838fa2ed33e8";
        let ssh_key = String::from_utf8(general_purpose::STANDARD.decode("LS0tLS1CRUdJTiBPUEVOU1NIIFBSSVZBVEUgS0VZLS0tLS0KYjNCbGJuTnphQzFyWlhrdGRqRUFBQUFBQkc1dmJtVUFBQUFFYm05dVpRQUFBQUFBQUFBQkFBQUFNd0FBQUF0emMyZ3RaVwpReU5UVXhPUUFBQUNBTzZlaGNrV0JrNlcwd3lTZ0FIY0dSY3JneW1IVThqRWVKRm5yQ2k1ZjZaQUFBQUpERlV0TVZ4VkxUCkZRQUFBQXR6YzJndFpXUXlOVFV4T1FBQUFDQU82ZWhja1dCazZXMHd5U2dBSGNHUmNyZ3ltSFU4akVlSkZuckNpNWY2WkEKQUFBRUQ0aGwvTmk0aGgvK3oxUm4wdWtMcm5mQ0xrN1BUWmErbVNQYk01ZS9aS0pnN3A2RnlSWUdUcGJUREpLQUFkd1pGeQp1REtZZFR5TVI0a1dlc0tMbC9wa0FBQUFDbVZ5WldKbFFITjBlWGdCQWdNPQotLS0tLUVORCBPUEVOU1NIIFBSSVZBVEUgS0VZLS0tLS0K").unwrap()).unwrap();
        let invalid_ssh_key = String::from_utf8(general_purpose::STANDARD.decode("LS0tLS1CRUdJTiBPUEVOU1NIIFBSSVZBVEUgS0VZLS0tLS0KYjNCbGJuTnphQzFyWlhrdGRqRUFBQUFBQ21GbGN6STFOaTFqZEhJQUFBQUdZbU55ZVhCMEFBQUFHQUFBQUJCNzZzbWIzVgp5WFB3SE12dm8zWTB5M0FBQUFFQUFBQUFFQUFBR1hBQUFBQjNOemFDMXljMkVBQUFBREFRQUJBQUFCZ1FDOVZHbm13cjZCClRHdWxzODhEaXRXaE5IUUoxMjV0eGxHa2EzNDNxUVB2S3dSc2VxN05SdFAzY2IxbDRMZytzdWozZ0lQYU5yM295SlBoRDIKZmIxbzF1cUFiOStkbWhwQXc4L1lCa05NZkRrdDRTWEpGZjZ3dUZwa1p4SHF3czNZUXF6cjhicVJaaHA0bXlnc2VwNFVHOApBaGxVMG5CUXFBREFhS3dBcmpLeUdBeWwwenRDYVdObm9sOVRZSmZuNEpOQW5YUDFONmMxMUVaRm5wKzJsMTVoSVdNd2NKClpCMnFFeTFSZzFVNXpuOVNSOURIVXhvN2p0ZkkrdWJWbHdnelBQaDVjZzAydVc0K0JwcFg1UGlpZ04rQlBNajc3WEJ0VTQKZzU3MmRDZHBSRjk3NjJ5SDBsY21nSkRqVnhnOTludVVGRDlwVG9nUTRrUENrdUluNmcxS3JObFdqY1R2c1hFS2JVS0xqawpkQkR2Yk1tbzZBaHJXRFhDSjZqRUN0T2Jka29XMGVjTGU4cXB3Nmh5N1NmdWppSm9QbnVsazRWenMwR2xPa3VPU0JIUmhJClhSc25NaFNiNnh2dDl6QldJcklvZDZoWnhuQ0V2SWRESzlacVBnOXJpbXc4bG8rUkFwdm1ySnRINUhsbFJiYWh4K2RUU1cKM2hCa1BlMnNDL1UvRUFBQVdBVXBEOTFIQTAzSnQyNFFSSFVXRDAvVTJGMTBzZE5WN0w4bkhMeVNibFBnSFhMc3lpSTFxOQo0NXBOUEQyNElBakNzQ08rVHREcXc3MDhlNXliUWhXUCsybkxtdGQwclEyTXh3SnZwUjlGcEV6UDFyejRYUDVUbzZDN3N1CmZpd0JPZWd6bjhQT1hGSmRvRk9Ud3E3dWhaM201NE93NHZvZkFKSHdtYWtwTGZMd2R1TnQ3S1RNQkVpT3VlM0ZXTGtCR0wKQUE1RGtoYVlpVGgyajB2YU9jUWhxZVphVEp6V2tidUcvb29DK1cwcTVXcFNZdFlxREFhWEh0bG8rZGtOMFEzZVVhcm1FTQpGcy9tdEpha3dhOVhCMVgzMndKbUpIdmN0OG4vVzA1T0N5V0U1Y2szeitRQVB3a2pGK0hKOGlOZDluVk5zckx1T010a2VQCk1aMTZreTg5WUVSZVQ1QXRJU1lRd0JQU2tsTFZKL3VaOCszK2Vyc3JrOW1aakw3ZXpISnV4ZysxUmR1T3BPeWpXMTRoTGYKblJQTDlKOXgvZWZ2MFV0L3BpR3M5NEFRcFFVZnJFdXpjL1dmejRocUtzVUxnT0VnblZBWXpuSksyWHJGeTN4aWlKVkFVUQpZcm4xak9lU1oyTWV0cjJvd05VdVM3cEhGTHZIWURRWklURmxVaFlOYUx0ejV5WU9HTCtFbEVxQm4wT1FFenNESDhROEpFCk5jWGVxUjFRTE4rTUJaMFZqQ2Q3T0ExTGpXZVVrdjNMaFJER3lPS3RjWk5OeFl5MkgwRWlmYzIvRHpLMnlpcVRQWUdMbHYKOWhZTlZZcC8xOGxhUkFOL040MlVDMjRmS0hFZ2lYVTNnL3RCZkZmbEFBWThKSE9sQUJEdXFWYjJkWHZKdXFLeUJMUElqVQo5cVl5VXNOVXhWS2M2ZWh4VU4wcVlnTmV2Z0JmMXVSZkxCY2c3SjVJVDZQQ2dSa3lNenBRakY1RkhuM0J6SVMrb3ZFSnNaCk5LNklYbDJIY3FncExTWUFkTFZlZEZOUzlkVU01blpMdlJEMjkyc0FQWm5aaU91Z3pwSWNrMllFcXpscjc2NXlUakRJdWgKR3kvdFlBQ3FIZHV4S2pMdGc0OXpjZjdNN2xESGNuVEY1MlJsazEyR2x1emZGK1dhZDF3eUFKVnNyUmtqVFZYVHhnTEV6MQo4SzF0WUtVOWoyc3grUE1Vd0JxM3lQR2lTaEgydWp6em82SUc1cnVYSTAwZXVkT2t1NVVrSHhBVnJneUI1S0M2VFRMR1BYCnhQMFN5Zk12dXJycDdvMnhsK2dkSVc0c0dudEJ2V0RHRVFSY0RxbWdLV0tuNTNsbmg5U1Urcmh2UkdhRFJueENuYkNwUEUKTE82V0lKUXVPQm54bzhWcGU0R2JLc2NmSktKSzlZV2ZIOFEvYzBncnE0ZDh5ZmRwUG1uc3hHOEpoTFVuMEhpRFEzQytaMgpzU1RPeU85TDAySUZIdDdIUEY2OWRWR3c3M0pPU1FiL05GK2g5cGRVazBScGNRdGFaTm9TMHg2a3RCQXljK0o0VUpUYTliCkdENWRaSE1KVHBvcWFZUDV0dFlnMjlBQkpUUURMa0tnbWxWRGNtK28zRTN3cTlySWFXMlhpNDQrc3RnTVJVS1J5R041d1EKM2xTWjk1QXBpWFlpRkNONUVrWitUci96TDAraVdwUHRCRzlJZmlGbmlqVlVYUnpEWHZxeGE1QTQ1YUlNWDhad2U5ckxFdAphaVRaOUI5d2tVb0tYdXlDU3plQXhMTGU2aG8wLzBDbmhSR3NoVGg1UDd6aFA4bVExRGZMYlFCRU0zOHJMWlplMExVVVhZCkZpZkFXc3BFRDk2VjBMckhxRkd0Z0dzd1NQcWRBRzBPTDBWekRUbFRucDJVWDY0SEhjUzF2MUMyQnNxbllWbkJNL3p5aUYKQXhabDB4cGRPUVVuKzV2V2VHUXZsQkhGeU0vQmtXRVhMbjc1YVNQL3JwcnlZeGdOeWx2M2NiRWNYZXoyWXdLM2UrN1NnZAoxRzFZUVVtNStqNy90Q0x5aFluL1VjRzJhTHJNc3pRY1FoWTE4Sk9IOXF6a2FacWdYckFybnE0dWluT25sbFBKaGJ3ZTVrCmgvMmdyTlVqbEsrRHYxQ2dGZUVDcm9yRHo4L3ZxZW1QNXdVWWF5bFNWWVZ3UHM1bkxDQWUrVlNobFlIOXlNb3JwanNXc3MKYlg0UlAvVGd3TmNtRnBuZ21kTXppNmtIUXhSc2pUT3VxZ3Vsb01FUVZmQ3JkNGxBeWp3eVhRaEcrd2dWMXBuempCZlR4eQpZeFBrc1VGaTg3aEVkZ1RPZ2M5MHlNamVoVGhHOGRMWGEvd0NOU0hLZ1pBbFBZbWdLd2ZvcFlBMjQxdUlxR2J0WUtqSTFSCnVHU2JqSU80dUVYbkJ5eWVZTnA3Z29iR2NVc1BGV0doY1FPV05QZnl5K1crQ0xhKzVpYkJCZEF2NStVdlZZUHFGMHhTNy8KUm1TbW9BPT0KLS0tLS1FTkQgT1BFTlNTSCBQUklWQVRFIEtFWS0tLS0t").unwrap()).unwrap();
        let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_submodule".to_string());
        let get_credentials = |user: &str| {
            vec![
                (
                    CredentialType::SSH_MEMORY,
                    Cred::ssh_key_from_memory(user, None, &invalid_ssh_key, Some("toto")).unwrap(),
                ),
                (
                    CredentialType::SSH_MEMORY,
                    Cred::ssh_key_from_memory(user, None, &ssh_key, None).unwrap(),
                ),
                (
                    CredentialType::SSH_MEMORY,
                    Cred::ssh_key_from_memory(user, None, &invalid_ssh_key, Some("toto")).unwrap(),
                ),
            ]
        };
        let repo = clone_at_commit(
            &Url::parse("https://github.com/Qovery/engine-testing.git").unwrap(),
            commit_id,
            Path::new(&clone_dir.path),
            &get_credentials,
            false,
        );
        assert!(repo.is_ok());
        assert!(PathBuf::from(format!("{}/dumb-logger/README.md", clone_dir.path())).exists());

        // Valid commit
        let repo = Repository::open(&clone_dir.path);
        assert!(repo.is_ok());
        assert_eq!(repo.unwrap().head().unwrap().target().unwrap().to_string(), commit_id);
    }

    #[test]
    fn test_git_submodule_skipped_when_disabled() {
        // Same repo as test_git_submodule_with_ssh_key: its submodule requires an SSH key.
        // With skip_submodules, the clone must succeed without any credentials and
        // the submodule directory must stay empty.
        let commit_id = "9a9c1f4373c8128151a9def9ea3d838fa2ed33e8";
        let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_submodule_skip".to_string());
        let repo = clone_at_commit(
            &Url::parse("https://github.com/Qovery/engine-testing.git").unwrap(),
            commit_id,
            Path::new(&clone_dir.path),
            &|_| Vec::new(),
            true,
        );
        assert!(repo.is_ok());
        assert!(!PathBuf::from(format!("{}/dumb-logger/README.md", clone_dir.path())).exists());

        let repo = Repository::open(&clone_dir.path);
        assert!(repo.is_ok());
        assert_eq!(repo.unwrap().head().unwrap().target().unwrap().to_string(), commit_id);
    }
}
