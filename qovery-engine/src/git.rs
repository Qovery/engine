use std::path::Path;

use git2::build::RepoBuilder;
use git2::{Error, Oid, Repository};

/// TODO support SSH repository_url - we assume that the repository URL starts with HTTPS
/// TODO support git submodules
pub fn clone<P>(
    repository_url: &str,
    into_dir: P,
    credentials: &Option<Credentials>,
) -> Result<Repository, Error>
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

pub fn checkout(repo: &Repository, commit_id: &str) -> Result<(), Error> {
    let oid = Oid::from_str(&commit_id).unwrap();
    let commit = repo.find_commit(oid).unwrap();

    let branch = repo.branch(commit_id, &commit, false);

    let obj = repo
        .revparse_single(&("refs/heads/".to_owned() + &commit_id))
        .unwrap();

    repo.checkout_tree(&obj, None);

    repo.set_head(&("refs/heads/".to_owned() + &commit_id))
}

pub struct Credentials {
    pub login: String,
    pub password: String,
}
