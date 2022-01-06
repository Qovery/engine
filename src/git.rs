use std::path::Path;

use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::ErrorCode::Auth;
use git2::ResetType::Hard;
use git2::{Cred, CredentialType, Error, RemoteCallbacks, Repository, SubmoduleUpdateOptions};
use url::Url;

// Credentials callback is called endlessly until the server return Auth Ok (or a definitive error)
// If auth is denied, it up to us to return a new credential to try different auth method
// or an error to specify that we have exhausted everything we are able to provide
fn authentication_callback<'a>(
    get_credentials: &'a impl Fn(&str) -> Vec<(CredentialType, Cred)>,
) -> impl FnMut(&str, Option<&str>, CredentialType) -> Result<Cred, Error> + 'a {
    let mut current_credentials: (String, Vec<(CredentialType, Cred)>) = ("".into(), vec![]);

    return move |remote_url, username_from_url, allowed_types| {
        // If we have changed remote, reset our available auth methods
        if remote_url != current_credentials.0 {
            current_credentials = (
                remote_url.to_string(),
                get_credentials(username_from_url.unwrap_or("git")),
            );
        }
        let auth_methods = &mut current_credentials.1;

        // Try all the auth method until one match allowed_types
        loop {
            let (cred_type, credential) = match auth_methods.pop() {
                Some(cred) => cred,
                None => {
                    let msg = format!(
                        "Invalid authentication: Exhausted all available auth method to fetch repository {}",
                        remote_url
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
    };
}

fn checkout(repo: &Repository, commit_id: &str) -> Result<(), Error> {
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

    repo.reset(&obj, Hard, Some(&mut checkout_opts))
}

fn clone<P>(
    repository_url: &str,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
) -> Result<Repository, Error>
where
    P: AsRef<Path>,
{
    let url = Url::parse(repository_url)
        .map_err(|err| Error::from_str(format!("Invalid repository url {}: {}", repository_url, err).as_str()))?;

    if url.scheme() != "https" {
        return Err(Error::from_str("Repository URL have to start with https://"));
    }

    // Prepare authentication callbacks.
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(authentication_callback(&get_credentials));

    // Prepare fetch options.
    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(callbacks);

    // Get our repository
    let mut repo = RepoBuilder::new();
    repo.fetch_options(fo);
    repo.clone(url.as_str(), into_dir.as_ref())
}

pub fn clone_at_commit<P>(
    repository_url: &str,
    commit_id: &str,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
) -> Result<Repository, Error>
where
    P: AsRef<Path>,
{
    // clone repository
    let repo = clone(repository_url, into_dir, get_credentials)?;

    // position the repo at the correct commit
    checkout(&repo, commit_id)?;

    // check submodules if needed
    {
        let submodules = repo.submodules()?;
        if !submodules.is_empty() {
            // for auth
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(authentication_callback(&get_credentials));

            let mut fo = git2::FetchOptions::new();
            fo.remote_callbacks(callbacks);
            let mut opts = SubmoduleUpdateOptions::new();
            opts.fetch(fo);

            for mut submodule in submodules {
                info!("getting submodule {:?} from {:?}", submodule.name(), submodule.url());
                submodule.update(true, Some(&mut opts))?
            }
        }
    }

    Ok(repo)
}

#[cfg(test)]
mod tests {
    use crate::git::{checkout, clone, clone_at_commit};
    use git2::{Cred, CredentialType};

    struct DirectoryToDelete<'a> {
        pub path: &'a str,
    }

    impl<'a> Drop for DirectoryToDelete<'a> {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.path);
        }
    }

    #[test]
    fn test_git_clone_repository() {
        // We only allow https:// at the moment
        let repo = clone("git@github.com:Qovery/engine.git", "/tmp", &|_| vec![]);
        assert!(matches!(repo, Err(e) if e.message().contains("Invalid repository")));

        // Repository must be empty
        let repo = clone("https://github.com/Qovery/engine-testing.git", "/tmp", &|_| vec![]);
        assert!(matches!(repo, Err(e) if e.message().contains("'/tmp' exists and is not an empty directory")));

        // Working case
        {
            let clone_dir = DirectoryToDelete {
                path: "/tmp/engine_test_clone",
            };
            let repo = clone(
                "https://github.com/Qovery/engine-testing.git",
                clone_dir.path,
                &|_| vec![],
            );
            assert!(matches!(repo, Ok(_repo)));
        }

        // Invalid credentials
        {
            let clone_dir = DirectoryToDelete {
                path: "/tmp/engine_test_clone",
            };
            let get_credentials = |_: &str| {
                vec![(
                    CredentialType::USER_PASS_PLAINTEXT,
                    Cred::userpass_plaintext("FAKE", "FAKE").unwrap(),
                )]
            };
            let repo = clone("https://gitlab.com/qovery/q-core.git", clone_dir.path, &get_credentials);
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
    fn test_git_checkout() {
        let clone_dir = DirectoryToDelete {
            path: "/tmp/engine_test_checkout",
        };
        let repo = clone(
            "https://github.com/Qovery/engine-testing.git",
            clone_dir.path,
            &|_| vec![],
        )
        .unwrap();

        // Invalid commit for this repository
        let check = checkout(&repo, "c2c2101f8e4c4ffadb326dc440ba8afb4aeb1310");
        assert!(matches!(check, Err(_err)));

        // Valid commit
        let commit = "9a9c1f4373c8128151a9def9ea3d838fa2ed33e8";
        assert_ne!(repo.head().unwrap().target().unwrap().to_string(), commit);
        let check = checkout(&repo, commit);
        assert!(matches!(check, Ok(())));
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), commit);
    }

    #[test]
    fn test_git_submodule_with_ssh_key() {
        // Unique Key only valid for the submodule and in read access only
        // https://github.com/Qovery/dumb-logger/settings/keys
        let ssh_key = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
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
i8Lcpwg96ZVfHBAAAACmVyZWJlQHN0eXg=
-----END OPENSSH PRIVATE KEY-----
        "#
        .trim();

        let invalid_ssh_key = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
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
RmSmoA==
-----END OPENSSH PRIVATE KEY-----
        "#
        .trim();
        let clone_dir = DirectoryToDelete {
            path: "/tmp/engine_test_submodule",
        };
        let get_credentials = |user: &str| {
            vec![
                (
                    CredentialType::SSH_MEMORY,
                    Cred::ssh_key_from_memory(user, None, invalid_ssh_key, Some("toto")).unwrap(),
                ),
                (
                    CredentialType::SSH_MEMORY,
                    Cred::ssh_key_from_memory(user, None, &ssh_key, None).unwrap(),
                ),
                (
                    CredentialType::SSH_MEMORY,
                    Cred::ssh_key_from_memory(user, None, invalid_ssh_key, Some("toto")).unwrap(),
                ),
            ]
        };
        let repo = clone_at_commit(
            "https://github.com/Qovery/engine-testing.git",
            "9a9c1f4373c8128151a9def9ea3d838fa2ed33e8",
            clone_dir.path,
            &get_credentials,
        );
        assert!(matches!(repo, Ok(_)));
    }
}
