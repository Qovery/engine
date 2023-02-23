use crate::cmd::command::{CommandError, CommandKiller, ExecutableCommand, QoveryCommand};
use itertools::Itertools;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::process::ExitStatus;

#[derive(thiserror::Error, Debug)]
pub enum GitLfsError {
    #[error("Git-lfs terminated with an unknown error: {raw_error:?}")]
    ExecutionError { raw_error: Error },

    #[error("Git-lfs terminated with a non success exit status code: {exit_status:?}")]
    ExitStatusError { exit_status: ExitStatus },

    #[error("Git-lfs aborted due to user cancel request: {raw_error_message:?}")]
    Aborted { raw_error_message: String },

    #[error("Git-lfs command terminated due to timeout: {raw_error_message:?}")]
    Timeout { raw_error_message: String },
}

impl From<Error> for GitLfsError {
    fn from(value: Error) -> Self {
        GitLfsError::ExecutionError { raw_error: value }
    }
}

#[derive(Debug, Default)]
pub struct GitLfs {
    common_envs: Vec<(String, String)>,
}

impl GitLfs {
    pub fn new(login: String, password: String) -> Self {
        GitLfs {
            // You need to set credentials helper to be able to pass credentials to git by env vars
            // git config --global credential.helper '!f() { sleep 1; echo "username=${GIT_USER}"; echo "password=${GIT_PASSWORD}"; }; f'
            common_envs: vec![
                ("GIT_USER".to_string(), login),
                ("GIT_PASSWORD".to_string(), password),
                //("GIT_TRACE".to_string(), "1".to_string())
            ],
        }
    }

    fn get_all_envs<'a>(&'a self, envs: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all_envs: Vec<(&str, &str)> = self.common_envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all_envs.append(&mut envs.to_vec());

        all_envs
    }

    pub fn files_size_estimate_in_kb<P>(
        &self,
        repo_path: P,
        commit: &str,
        cmd_killer: &CommandKiller,
    ) -> Result<u64, GitLfsError>
    where
        P: AsRef<Path>,
    {
        let mut output: Vec<String> = Vec::with_capacity(25);
        let mut stderr = String::new();
        git_lfs_exec(
            &[
                "-C",
                repo_path.as_ref().to_string_lossy().as_ref(),
                "lfs",
                "ls-files",
                "-s",
                commit,
            ],
            &self.get_all_envs(&[]),
            &mut |line| output.push(line),
            &mut |line| stderr.push_str(&line),
            cmd_killer,
        )?;

        let mut total_size: u64 = 0;
        for line in output {
            let Some(size) = line.split('(').last().map(|x| x.trim_end_matches(')')) else { continue };
            let Some((size, unit)) = size.split(' ').collect_tuple() else { continue };

            let size: u64 = size.parse::<f32>().unwrap().round() as u64;
            match unit {
                "B" => total_size += 1,
                "KB" => total_size += size,
                "MB" => total_size += size * 1024,
                "GB" => total_size += size * 1024 * 1024,
                "TB" => total_size += size * 1024 * 1024 * 1024,
                "PB" => total_size += size * 1024 * 1024 * 1024 * 1024,
                _ => {
                    let msg = format!("Unknown unit when using git-lfs: {unit}");
                    error!("{}", msg);
                    return Err(GitLfsError::ExecutionError {
                        raw_error: Error::new(ErrorKind::Other, msg),
                    });
                }
            }
        }

        Ok(total_size)
    }

    pub fn checkout_files_for_commit<P>(
        &self,
        repo_path: P,
        commit: &str,
        cmd_killer: &CommandKiller,
    ) -> Result<(), GitLfsError>
    where
        P: AsRef<Path>,
    {
        git_lfs_exec(
            &[
                "-C",
                repo_path.as_ref().to_string_lossy().as_ref(),
                "lfs",
                "fetch",
                "origin",
                commit,
            ],
            &self.get_all_envs(&[]),
            &mut |line| info!("{line}"),
            &mut |line| warn!("{line}"),
            cmd_killer,
        )?;

        git_lfs_exec(
            &["-C", repo_path.as_ref().to_string_lossy().as_ref(), "lfs", "checkout"],
            &self.get_all_envs(&[]),
            &mut |line| info!("{line}"),
            &mut |line| warn!("{line}"),
            cmd_killer,
        )?;

        Ok(())
    }
}

fn git_lfs_exec<F, X>(
    args: &[&str],
    envs: &[(&str, &str)],
    stdout_output: &mut F,
    stderr_output: &mut X,
    cmd_killer: &CommandKiller,
) -> Result<(), GitLfsError>
where
    F: FnMut(String),
    X: FnMut(String),
{
    let mut cmd = QoveryCommand::new("git", args, envs);
    let ret = cmd.exec_with_abort(stdout_output, stderr_output, cmd_killer);

    match ret {
        Ok(_) => Ok(()),
        Err(CommandError::TimeoutError(msg)) => Err(GitLfsError::Timeout { raw_error_message: msg }),
        Err(CommandError::Killed(msg)) => Err(GitLfsError::Aborted { raw_error_message: msg }),
        Err(CommandError::ExitStatusError(err)) => Err(GitLfsError::ExitStatusError { exit_status: err }),
        Err(CommandError::ExecutionError(err)) => Err(GitLfsError::ExecutionError { raw_error: err }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;
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
    const VALID_COMMIT: &str = "43d890f5d3ff78e906d7e884e38c8175eadfd642";

    #[test]
    fn test_size_estimate() {
        let repo_dir = DirectoryForTests::new_with_random_suffix("/tmp/tmp_git".to_string());
        let repo_path = repo_dir.path();

        git::clone_at_commit(
            &"https://github.com/Qovery/engine-testing-lfs.git".parse().unwrap(),
            VALID_COMMIT,
            &repo_path,
            &|_| Vec::new(),
        )
        .unwrap();
        let cmd = GitLfs::default();

        // Fake commit is nok
        {
            let size_estimate = cmd.files_size_estimate_in_kb(
                &repo_path,
                "ffffffffffffffffffffffffffffffffffffffff",
                &CommandKiller::never(),
            );
            assert!(matches!(size_estimate, Err(GitLfsError::ExitStatusError { .. })));
        }

        // Valid commit is ok
        {
            let size_estimate = cmd.files_size_estimate_in_kb(&repo_path, VALID_COMMIT, &CommandKiller::never());
            assert!(matches!(size_estimate, Ok(38912)));
        }
    }

    // We don't pay for git lfs on github, so we have a quota of 1GB per 3 day, and with CI we reach it.
    // Ignore the test to avoid having it failing the CI
    #[ignore]
    #[test]
    fn test_checkout_files_for_commit() {
        // Repo that does not support lfs, should not return an error
        {
            let repo_dir = DirectoryForTests::new_with_random_suffix("/tmp/tmp_git".to_string());
            let repo_path = repo_dir.path();

            git::clone_at_commit(
                &"https://github.com/Qovery/engine-testing.git".parse().unwrap(),
                "9df822462e3e7215548e492bc2c15a50a92fed39",
                &repo_path,
                &|_| Vec::new(),
            )
            .unwrap();

            let cmd = GitLfs::default();
            let ret = cmd.checkout_files_for_commit(
                &repo_path,
                "9df822462e3e7215548e492bc2c15a50a92fed39",
                &CommandKiller::never(),
            );
            matches!(ret, Ok(_));
        }

        // Repo that support lfs, should properly get the files
        {
            let repo_dir = DirectoryForTests::new_with_random_suffix("/tmp/tmp_git".to_string());
            let repo_path = repo_dir.path();

            git::clone_at_commit(
                &"https://github.com/Qovery/engine-testing-lfs.git".parse().unwrap(),
                VALID_COMMIT,
                &repo_path,
                &|_| Vec::new(),
            )
            .unwrap();

            let file = std::fs::read(format!("{repo_path}/31eovo.mp4")).unwrap();
            assert_ne!(file.len(), 4048871);

            let cmd = GitLfs::default();
            let ret = cmd.checkout_files_for_commit(&repo_path, VALID_COMMIT, &CommandKiller::never());
            matches!(ret, Ok(_));
            let file = std::fs::read(format!("{repo_path}/31eovo.mp4")).unwrap();
            assert_eq!(file.len(), 4048871);
        }

        // Invalid commit
        {
            let repo_dir = DirectoryForTests::new_with_random_suffix("/tmp/tmp_git".to_string());
            let repo_path = repo_dir.path();

            git::clone_at_commit(
                &"https://github.com/Qovery/engine-testing-lfs.git".parse().unwrap(),
                VALID_COMMIT,
                &repo_path,
                &|_| Vec::new(),
            )
            .unwrap();

            let cmd = GitLfs::default();
            let ret = cmd.checkout_files_for_commit(
                &repo_path,
                "ffffffffffffffffffffffffffffffffffffffff",
                &CommandKiller::never(),
            );
            matches!(ret, Err(_));
            let file = std::fs::read(format!("{repo_path}/31eovo.mp4")).unwrap();
            assert_ne!(file.len(), 4048871);
        }
    }
}
