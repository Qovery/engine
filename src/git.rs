use std::path::Path;

use git2::build::RepoBuilder;
use git2::{Error, FetchOptions, Repository};

/// TODO support SSH repository_url - we assume that the repository URL starts with HTTPS
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

pub struct Credentials {
    pub login: String,
    pub password: String,
}
