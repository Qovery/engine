use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::ErrorCode::Auth;
use git2::ResetType::Hard;
use git2::{
    AutotagOption, CertificateCheckStatus, Cred, CredentialType, Error, FetchOptions, Object, RemoteCallbacks,
    Repository, SubmoduleUpdateOptions,
};
use url::Url;

pub fn clone_at_commit<P>(
    repository_url: &Url,
    commit_id: &str,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
) -> Result<(), Error>
where
    P: AsRef<Path>,
{
    let repo = fetch(repository_url, into_dir, get_credentials, commit_id)?;

    // position the repo at the correct commit
    let _ = checkout(&repo, commit_id)?;

    // check submodules if needed
    {
        let submodules = repo.submodules()?;
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
                submodule.update(true, Some(&mut opts))?
            }
        }
    }

    Ok(())
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

fn fetch<P>(
    repository_url: &Url,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
    commit_id: &str,
) -> Result<Repository, Error>
where
    P: AsRef<Path>,
{
    if repository_url.scheme() != "https" {
        return Err(Error::from_str("Repository URL have to start with https://"));
    }

    // Prepare authentication callbacks.
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(authentication_callback(&get_credentials));

    // Prepare fetch options.
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(callbacks);
    fo.depth(1);
    fo.update_fetchhead(false);
    fo.download_tags(AutotagOption::None);

    // Get our repository
    if into_dir.as_ref().exists() {
        let _ = std::fs::remove_dir_all(into_dir.as_ref());
    }

    let repo = Repository::init(into_dir)?;
    remote_fetch(repository_url, &commit_id, &mut fo, &repo)?;

    Ok(repo)
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
    use crate::git::{checkout, clone_at_commit, fetch};
    use git2::{Cred, CredentialType, Repository};
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
    fn test_git_submodule_with_ssh_key() {
        // Unique Key only valid for the submodule and in read access only
        // https://github.com/Qovery/dumb-logger/settings/keys
        let commit_id = "9a9c1f4373c8128151a9def9ea3d838fa2ed33e8";
        let ssh_key = String::from_utf8(base64::decode("LS0tLS1CRUdJTiBPUEVOU1NIIFBSSVZBVEUgS0VZLS0tLS0KYjNCbGJuTnphQzFyWlhrdGRqRUFBQUFBQkc1dmJtVUFBQUFFYm05dVpRQUFBQUFBQUFBQkFBQUFNd0FBQUF0emMyZ3RaVwpReU5UVXhPUUFBQUNBTzZlaGNrV0JrNlcwd3lTZ0FIY0dSY3JneW1IVThqRWVKRm5yQ2k1ZjZaQUFBQUpERlV0TVZ4VkxUCkZRQUFBQXR6YzJndFpXUXlOVFV4T1FBQUFDQU82ZWhja1dCazZXMHd5U2dBSGNHUmNyZ3ltSFU4akVlSkZuckNpNWY2WkEKQUFBRUQ0aGwvTmk0aGgvK3oxUm4wdWtMcm5mQ0xrN1BUWmErbVNQYk01ZS9aS0pnN3A2RnlSWUdUcGJUREpLQUFkd1pGeQp1REtZZFR5TVI0a1dlc0tMbC9wa0FBQUFDbVZ5WldKbFFITjBlWGdCQWdNPQotLS0tLUVORCBPUEVOU1NIIFBSSVZBVEUgS0VZLS0tLS0K").unwrap()).unwrap();
        let invalid_ssh_key = String::from_utf8(base64::decode("LS0tLS1CRUdJTiBPUEVOU1NIIFBSSVZBVEUgS0VZLS0tLS0KYjNCbGJuTnphQzFyWlhrdGRqRUFBQUFBQ21GbGN6STFOaTFqZEhJQUFBQUdZbU55ZVhCMEFBQUFHQUFBQUJCNzZzbWIzVgp5WFB3SE12dm8zWTB5M0FBQUFFQUFBQUFFQUFBR1hBQUFBQjNOemFDMXljMkVBQUFBREFRQUJBQUFCZ1FDOVZHbm13cjZCClRHdWxzODhEaXRXaE5IUUoxMjV0eGxHa2EzNDNxUVB2S3dSc2VxN05SdFAzY2IxbDRMZytzdWozZ0lQYU5yM295SlBoRDIKZmIxbzF1cUFiOStkbWhwQXc4L1lCa05NZkRrdDRTWEpGZjZ3dUZwa1p4SHF3czNZUXF6cjhicVJaaHA0bXlnc2VwNFVHOApBaGxVMG5CUXFBREFhS3dBcmpLeUdBeWwwenRDYVdObm9sOVRZSmZuNEpOQW5YUDFONmMxMUVaRm5wKzJsMTVoSVdNd2NKClpCMnFFeTFSZzFVNXpuOVNSOURIVXhvN2p0ZkkrdWJWbHdnelBQaDVjZzAydVc0K0JwcFg1UGlpZ04rQlBNajc3WEJ0VTQKZzU3MmRDZHBSRjk3NjJ5SDBsY21nSkRqVnhnOTludVVGRDlwVG9nUTRrUENrdUluNmcxS3JObFdqY1R2c1hFS2JVS0xqawpkQkR2Yk1tbzZBaHJXRFhDSjZqRUN0T2Jka29XMGVjTGU4cXB3Nmh5N1NmdWppSm9QbnVsazRWenMwR2xPa3VPU0JIUmhJClhSc25NaFNiNnh2dDl6QldJcklvZDZoWnhuQ0V2SWRESzlacVBnOXJpbXc4bG8rUkFwdm1ySnRINUhsbFJiYWh4K2RUU1cKM2hCa1BlMnNDL1UvRUFBQVdBVXBEOTFIQTAzSnQyNFFSSFVXRDAvVTJGMTBzZE5WN0w4bkhMeVNibFBnSFhMc3lpSTFxOQo0NXBOUEQyNElBakNzQ08rVHREcXc3MDhlNXliUWhXUCsybkxtdGQwclEyTXh3SnZwUjlGcEV6UDFyejRYUDVUbzZDN3N1CmZpd0JPZWd6bjhQT1hGSmRvRk9Ud3E3dWhaM201NE93NHZvZkFKSHdtYWtwTGZMd2R1TnQ3S1RNQkVpT3VlM0ZXTGtCR0wKQUE1RGtoYVlpVGgyajB2YU9jUWhxZVphVEp6V2tidUcvb29DK1cwcTVXcFNZdFlxREFhWEh0bG8rZGtOMFEzZVVhcm1FTQpGcy9tdEpha3dhOVhCMVgzMndKbUpIdmN0OG4vVzA1T0N5V0U1Y2szeitRQVB3a2pGK0hKOGlOZDluVk5zckx1T010a2VQCk1aMTZreTg5WUVSZVQ1QXRJU1lRd0JQU2tsTFZKL3VaOCszK2Vyc3JrOW1aakw3ZXpISnV4ZysxUmR1T3BPeWpXMTRoTGYKblJQTDlKOXgvZWZ2MFV0L3BpR3M5NEFRcFFVZnJFdXpjL1dmejRocUtzVUxnT0VnblZBWXpuSksyWHJGeTN4aWlKVkFVUQpZcm4xak9lU1oyTWV0cjJvd05VdVM3cEhGTHZIWURRWklURmxVaFlOYUx0ejV5WU9HTCtFbEVxQm4wT1FFenNESDhROEpFCk5jWGVxUjFRTE4rTUJaMFZqQ2Q3T0ExTGpXZVVrdjNMaFJER3lPS3RjWk5OeFl5MkgwRWlmYzIvRHpLMnlpcVRQWUdMbHYKOWhZTlZZcC8xOGxhUkFOL040MlVDMjRmS0hFZ2lYVTNnL3RCZkZmbEFBWThKSE9sQUJEdXFWYjJkWHZKdXFLeUJMUElqVQo5cVl5VXNOVXhWS2M2ZWh4VU4wcVlnTmV2Z0JmMXVSZkxCY2c3SjVJVDZQQ2dSa3lNenBRakY1RkhuM0J6SVMrb3ZFSnNaCk5LNklYbDJIY3FncExTWUFkTFZlZEZOUzlkVU01blpMdlJEMjkyc0FQWm5aaU91Z3pwSWNrMllFcXpscjc2NXlUakRJdWgKR3kvdFlBQ3FIZHV4S2pMdGc0OXpjZjdNN2xESGNuVEY1MlJsazEyR2x1emZGK1dhZDF3eUFKVnNyUmtqVFZYVHhnTEV6MQo4SzF0WUtVOWoyc3grUE1Vd0JxM3lQR2lTaEgydWp6em82SUc1cnVYSTAwZXVkT2t1NVVrSHhBVnJneUI1S0M2VFRMR1BYCnhQMFN5Zk12dXJycDdvMnhsK2dkSVc0c0dudEJ2V0RHRVFSY0RxbWdLV0tuNTNsbmg5U1Urcmh2UkdhRFJueENuYkNwUEUKTE82V0lKUXVPQm54bzhWcGU0R2JLc2NmSktKSzlZV2ZIOFEvYzBncnE0ZDh5ZmRwUG1uc3hHOEpoTFVuMEhpRFEzQytaMgpzU1RPeU85TDAySUZIdDdIUEY2OWRWR3c3M0pPU1FiL05GK2g5cGRVazBScGNRdGFaTm9TMHg2a3RCQXljK0o0VUpUYTliCkdENWRaSE1KVHBvcWFZUDV0dFlnMjlBQkpUUURMa0tnbWxWRGNtK28zRTN3cTlySWFXMlhpNDQrc3RnTVJVS1J5R041d1EKM2xTWjk1QXBpWFlpRkNONUVrWitUci96TDAraVdwUHRCRzlJZmlGbmlqVlVYUnpEWHZxeGE1QTQ1YUlNWDhad2U5ckxFdAphaVRaOUI5d2tVb0tYdXlDU3plQXhMTGU2aG8wLzBDbmhSR3NoVGg1UDd6aFA4bVExRGZMYlFCRU0zOHJMWlplMExVVVhZCkZpZkFXc3BFRDk2VjBMckhxRkd0Z0dzd1NQcWRBRzBPTDBWekRUbFRucDJVWDY0SEhjUzF2MUMyQnNxbllWbkJNL3p5aUYKQXhabDB4cGRPUVVuKzV2V2VHUXZsQkhGeU0vQmtXRVhMbjc1YVNQL3JwcnlZeGdOeWx2M2NiRWNYZXoyWXdLM2UrN1NnZAoxRzFZUVVtNStqNy90Q0x5aFluL1VjRzJhTHJNc3pRY1FoWTE4Sk9IOXF6a2FacWdYckFybnE0dWluT25sbFBKaGJ3ZTVrCmgvMmdyTlVqbEsrRHYxQ2dGZUVDcm9yRHo4L3ZxZW1QNXdVWWF5bFNWWVZ3UHM1bkxDQWUrVlNobFlIOXlNb3JwanNXc3MKYlg0UlAvVGd3TmNtRnBuZ21kTXppNmtIUXhSc2pUT3VxZ3Vsb01FUVZmQ3JkNGxBeWp3eVhRaEcrd2dWMXBuempCZlR4eQpZeFBrc1VGaTg3aEVkZ1RPZ2M5MHlNamVoVGhHOGRMWGEvd0NOU0hLZ1pBbFBZbWdLd2ZvcFlBMjQxdUlxR2J0WUtqSTFSCnVHU2JqSU80dUVYbkJ5eWVZTnA3Z29iR2NVc1BGV0doY1FPV05QZnl5K1crQ0xhKzVpYkJCZEF2NStVdlZZUHFGMHhTNy8KUm1TbW9BPT0KLS0tLS1FTkQgT1BFTlNTSCBQUklWQVRFIEtFWS0tLS0t").unwrap()).unwrap();
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
        );
        assert!(repo.is_ok());
        assert!(PathBuf::from(format!("{}/dumb-logger/README.md", clone_dir.path())).exists());

        // Valid commit
        let repo = Repository::open(&clone_dir.path);
        assert!(repo.is_ok());
        assert_eq!(repo.unwrap().head().unwrap().target().unwrap().to_string(), commit_id);
    }
}
