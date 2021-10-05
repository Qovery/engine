use std::path::Path;

use git2::build::RepoBuilder;
use git2::{Error, Oid, Repository};

/// TODO support SSH repository_url - we assume that the repository URL starts with HTTPS
/// TODO support git submodules
pub fn clone<P>(repository_url: &str, into_dir: P, credentials: &Option<Credentials>) -> Result<Repository, Error>
where
    P: AsRef<Path>,
{
    let final_repository_url = match credentials {
        Some(c) => format!(
            "https://{}:{}@{}",
            c.login,
            c.password,
            repository_url.replace("https://", "")
        ),
        None => repository_url.to_string(),
    };

    RepoBuilder::new().clone(final_repository_url.as_str(), into_dir.as_ref())
}

pub fn checkout(repo: &Repository, commit_id: &str, repo_url: &str) -> Result<(), Error> {
    let oid = match Oid::from_str(commit_id) {
        Err(e) => {
            let x = git2::Error::from_str(
                format!(
                    "Error while trying to validate commit ID {} on repository {}: {}",
                    &commit_id, &repo_url, &e
                )
                .as_ref(),
            );
            return Err(x);
        }
        Ok(o) => o,
    };

    let _ = match repo.find_commit(oid) {
        Err(e) => {
            let mut x = git2::Error::from_str(
                format!("Commit ID {} on repository {} was not found", &commit_id, &repo_url).as_ref(),
            );
            x.set_code(e.code());
            x.set_class(e.class());
            return Err(x);
        }
        Ok(c) => c,
    };

    let obj = match repo.revparse_single(commit_id) {
        Err(e) => {
            let x = git2::Error::from_str(
                format!(
                    "Wasn't able to use git object commit ID {} on repository {}: {}",
                    &commit_id, &repo_url, &e
                )
                .as_ref(),
            );
            return Err(x);
        }
        Ok(o) => o,
    };

    let _ = repo.checkout_tree(&obj, None);

    repo.set_head(&("refs/heads/".to_owned() + commit_id))
}

pub fn checkout_submodules(repo: &Repository) -> Result<(), Error> {
    match repo.submodules() {
        Ok(submodules) => {
            for mut submodule in submodules {
                info!("getting submodule {:?} from {:?}", submodule.name(), submodule.url());

                if let Err(e) = submodule.update(true, None) { 
                    return Err(e)
                }
            }
        }
        Err(err) => return Err(err),
    }

    Ok(())
}

pub struct Credentials {
    pub login: String,
    pub password: String,
}
