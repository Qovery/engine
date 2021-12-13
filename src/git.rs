use std::path::Path;

use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::ResetType::Hard;
use git2::{Error, Repository};
use url::Url;

pub struct Credentials {
    pub login: String,
    pub password: String,
}

pub fn clone<P>(repository_url: &str, into_dir: P, credentials: &Option<Credentials>) -> Result<Repository, Error>
where
    P: AsRef<Path>,
{
    let mut url = Url::parse(repository_url)
        .map_err(|err| Error::from_str(format!("Invalid repository url {}: {}", repository_url, err).as_str()))?;

    if url.scheme() != "https" {
        return Err(Error::from_str("Repository URL have to start with https://"));
    }

    if let Some(Credentials { login, password }) = credentials {
        url.set_username(login).map_err(|_| Error::from_str("Invalid login"))?;
        url.set_password(Some(password))
            .map_err(|_| Error::from_str("Invalid password"))?;
    };

    RepoBuilder::new().clone(url.as_str(), into_dir.as_ref())
}

pub fn checkout(repo: &Repository, commit_id: &str) -> Result<(), Error> {
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

pub fn checkout_submodules(repo: &Repository) -> Result<(), Error> {
    for mut submodule in repo.submodules()? {
        info!("getting submodule {:?} from {:?}", submodule.name(), submodule.url());
        submodule.update(true, None)?
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::git::{checkout, clone, Credentials};

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
        let repo = clone("git@github.com:Qovery/engine.git", "/tmp", &None);
        assert!(matches!(repo, Err(e) if e.message().contains("Invalid repository")));

        // Repository must be empty
        let repo = clone("https://github.com/Qovery/engine.git", "/tmp", &None);
        assert!(matches!(repo, Err(e) if e.message().contains("'/tmp' exists and is not an empty directory")));

        // Working case
        {
            let clone_dir = DirectoryToDelete {
                path: "/tmp/engine_test_clone",
            };
            let repo = clone("https://github.com/Qovery/engine.git", clone_dir.path, &None);
            assert!(matches!(repo, Ok(_repo)));
        }

        // Invalid credentials
        {
            let clone_dir = DirectoryToDelete {
                path: "/tmp/engine_test_clone",
            };
            let creds = Some(Credentials {
                login: "FAKE".to_string(),
                password: "FAKE".to_string(),
            });
            let repo = clone("https://gitlab.com/qovery/q-core.git", clone_dir.path, &creds);
            assert!(matches!(repo, Err(repo) if repo.message().contains("authentication")));
        }

        /*
        // Test with bitbucket credentials
        // This is a toy account feel free to trash it
        {
            let clone_dir = DirectoryToDelete {
                path: "/tmp/engine_test_clone",
            };
            let creds = Some(Credentials {
                login: "{a45d7986-7994-43a9-a961-044799e761d7}".to_string(),
                password: "3uDbu-i3kdanLRV6iSSWzWDJf4oUQu2hbUQ250DMezFEkkmz3oxPRiAcj7RuLrNgmKu7qx6XA820uvvyfUCdx06bt4VCaOZQkEwkWVksNpAkPE1Lw8gPcnEK".to_string(),
            });
            let repo = clone(
                "https://bitbucket.org/erebe/attachment-parser.git",
                clone_dir.path,
                &creds,
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
        let repo = clone("https://github.com/Qovery/engine.git", clone_dir.path, &None).unwrap();

        // Invalid commit for this repository
        let check = checkout(&repo, "c2c2101f8e4c4ffadb326dc440ba8afb4aeb1310");
        assert!(matches!(check, Err(_err)));

        // Valid commit
        let commit = "9da0f98b5eb719643263abba062041708fa20d31";
        assert_ne!(repo.head().unwrap().target().unwrap().to_string(), commit);
        let check = checkout(&repo, commit);
        assert!(matches!(check, Ok(())));
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), commit);
    }
}
