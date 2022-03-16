use std::path::Path;

use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::ErrorCode::Auth;
use git2::ResetType::Hard;
use git2::{Cred, CredentialType, Error, Object, Oid, RemoteCallbacks, Repository, SubmoduleUpdateOptions};
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

    let _ = repo.reset(&obj, Hard, Some(&mut checkout_opts))?;
    Ok(obj)
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

    if into_dir.as_ref().exists() {
        let _ = std::fs::remove_dir_all(into_dir.as_ref());
    }

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
    let _ = checkout(&repo, commit_id)?;

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

pub fn get_parent_commit_id<P>(
    repository_url: &str,
    commit_id: &str,
    into_dir: P,
    get_credentials: &impl Fn(&str) -> Vec<(CredentialType, Cred)>,
) -> Result<Option<String>, Error>
where
    P: AsRef<Path>,
{
    // clone repository
    let repo = clone(repository_url, into_dir, get_credentials)?;

    let oid = Oid::from_str(commit_id)?;
    let commit = match repo.find_commit(oid) {
        Ok(commit) => commit,
        Err(_) => return Ok(None),
    };

    Ok(commit.parent_ids().next().map(|x| x.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::git::{checkout, clone, clone_at_commit, get_parent_commit_id};
    use git2::{Cred, CredentialType};
    use uuid::Uuid;

    struct DirectoryForTests {
        path: String,
    }

    impl DirectoryForTests {
        /// Generates a dir path with a random suffix.
        /// Since tests are runs in parallel and eventually on the same node, it will avoid having directories collisions between tests running on the same node.
        pub fn new_with_random_suffix(base_path: String) -> Self {
            DirectoryForTests {
                path: format!("{}_{}", base_path, Uuid::new_v4().to_string()),
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
    fn test_git_clone_repository() {
        let repo_dir = DirectoryForTests::new_with_random_suffix("/tmp/tmp_git".to_string());
        let repo_path = repo_dir.path();

        // We only allow https:// at the moment
        let repo = clone("git@github.com:Qovery/engine.git", &repo_path, &|_| vec![]);
        assert!(matches!(repo, Err(e) if e.message().contains("Invalid repository")));

        // Repository must be empty
        let repo = clone("https://github.com/Qovery/engine-testing.git", &repo_path, &|_| vec![]);
        assert!(repo.is_ok()); // clone makes sure to empty the directory

        // Working case
        {
            let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_clone".to_string());
            let repo = clone(
                "https://github.com/Qovery/engine-testing.git",
                clone_dir.path(),
                &|_| vec![],
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
            let repo = clone(
                "https://gitlab.com/qovery/q-core.git",
                clone_dir.path(),
                &get_credentials,
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
        let repo = clone(
            "https://github.com/Qovery/engine-testing.git",
            clone_dir.path(),
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
        assert!(matches!(check, Ok(_)));
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), commit);
    }

    #[test]
    fn test_git_parent_id() {
        let clone_dir = DirectoryForTests::new_with_random_suffix("/tmp/engine_test_parent_id".to_string());
        let result = get_parent_commit_id(
            "https://github.com/Qovery/engine-testing.git",
            "964f02f3a3065bc7f6fb745d679b1ddb21153cc7",
            clone_dir.path(),
            &|_| vec![],
        )
        .unwrap()
        .unwrap();

        assert_eq!(result, "1538fb6333b86798f0cf865558a28e729a98dace".to_string());
    }

    #[test]
    fn test_git_parent_id_not_existing() {
        let clone_dir =
            DirectoryForTests::new_with_random_suffix("/tmp/engine_test_parent_id_not_existing".to_string());
        let result = get_parent_commit_id(
            "https://github.com/Qovery/engine-testing.git",
            "964f02f3a3065bc7f6fb745d679b1ddb21153cc0",
            clone_dir.path(),
            &|_| vec![],
        )
        .unwrap();

        assert_eq!(result, None);
    }

    #[test]
    #[ignore] // TODO: fix or remove
    fn test_git_submodule_with_ssh_key() {
        // Unique Key only valid for the submodule and in read access only
        // https://github.com/Qovery/dumb-logger/settings/keys
        let ssh_key = String::from_utf8(base64::decode("LS0tLS1CRUdJTiBPUEVOU1NIIFBSSVZBVEUgS0VZLS0tLS0NCmIzQmxibk56YUMxclpYa3RkakVBQUFBQUJHNXZibVVBQUFBRWJtOXVaUUFBQUFBQUFBQUJBQUFCbHdBQUFBZHpjMmd0Y24NCk5oQUFBQUF3RUFBUUFBQVlFQTFGcS95ZGF6dU84T3ZRdjVUNEdxbndOMjhZV0EzaXlqanREMFdSQXhtdDZEV3lJRlVYZ1gNClZFZ1ZVYnZyYndKNGJQa0tTbkdqd1hZRUdJYkdYa0hKUTdvWTVSMnB6b1hqUkVYTzIzZEZ2aVp4bUpOcVdEVVJqSHhjc1INCndOYWxiOFVZZVBCRVI4TEQzWWpQd0lYNXdCWm5VSjZLWTJFbXhjSlBVUnV4bUlyTjI4QndiZ3FiejJPU3NJdWg4a1ZwSngNCldheitFc3JNM282NHpHMm0wa0dxMVI1VHE0enBPRWliUk1iY1ZXTldKUzRZR29JczdsRzB0ZHZndktNRnJsWktzSUw1Y2ENCkFOQzRXTlROMm1DVVFrVGpGSDVySDlDa0ZBZjZaZ0lqYklvN0s3TTc0L1B5RVhEcStyRW5vRWdzeEkzRi9NZHMydGM2RWkNClJaY2JrUmRLVnpaUzJCMXdKNDhrOGR3Sml5VytKSWY4ejEzK2FiUXVPNGR5MWRnM2gwbEZ6dm9qaVYxTjNBRXdHcmhjZEUNClo3TXNaeThKM3JvRElZSWZCczdkbmh2T1FrME1taEpKSEpMaVlEZWZCYUk4MVdGTGlqekUxejhqMG90cExlNkt0SVhQYk8NCmV5WWdod0U2aDlhSmNrOEU3WklYMjc4MGRQMW93T2g1dC9VaE0vdjFBQUFGZ082eU9GenVzamhjQUFBQUIzTnphQzF5YzINCkVBQUFHQkFOUmF2OG5XczdqdkRyMEwrVStCcXA4RGR2R0ZnTjRzbzQ3UTlGa1FNWnJlZzFzaUJWRjRGMVJJRlZHNzYyOEMNCmVHejVDa3B4bzhGMkJCaUd4bDVCeVVPNkdPVWRxYzZGNDBSRnp0dDNSYjRtY1ppVGFsZzFFWXg4WExFY0RXcFcvRkdIancNClJFZkN3OTJJejhDRitjQVdaMUNlaW1OaEpzWENUMUVic1ppS3pkdkFjRzRLbTg5amtyQ0xvZkpGYVNjVm1zL2hMS3pONk8NCnVNeHRwdEpCcXRVZVU2dU02VGhJbTBURzNGVmpWaVV1R0JxQ0xPNVJ0TFhiNEx5akJhNVdTckNDK1hHZ0RRdUZqVXpkcGcNCmxFSkU0eFIrYXgvUXBCUUgrbVlDSTJ5S095dXpPK1B6OGhGdzZ2cXhKNkJJTE1TTnhmekhiTnJYT2hJa1dYRzVFWFNsYzINClV0Z2RjQ2VQSlBIY0NZc2x2aVNIL005ZC9tbTBManVIY3RYWU40ZEpSYzc2STRsZFRkd0JNQnE0WEhSR2V6TEdjdkNkNjYNCkF5R0NId2JPM1o0YnprSk5ESm9TU1J5UzRtQTNud1dpUE5WaFM0bzh4TmMvSTlLTGFTM3VpclNGejJ6bnNtSUljQk9vZlcNCmlYSlBCTzJTRjl1L05IVDlhTURvZWJmMUlUUDc5UUFBQUFNQkFBRUFBQUdCQUxhR1pqRkwvV0NwQWtjV0lxM25LMHZRZzQNCjBuamxQcGxKQXVKTWprOVc1RGNpNkQrSVJGTC9BK29TeUcxTit2Qk9uTnliMmhIZnNzd0dxQWRjTVEwcmtISFZ6WitWbk4NCmxVSGFxdW5UQkR4aitPSUhXN0lEczFqSWtEZWZnQngyTmh5eDR3anRBTHBhVW1ja1B1SkhTcURSV3JvQkc1c01Uc3RwWmwNCnNtb0diTmxFK0o1dE9lMnhqYVYzNzdRNVd4L0FIemd0T09RemZNL3lTZjMzTDhCS1Y0a3J4eXV3ZW95T1Q5OU9ia0ltaUUNCnpTMEQxVERuUStmSTNjdm1aL3lvcDZ0clA0a01wdWtWdC93ZUhFWU5nZkdPdHVHMndwU3oyRmpNcUcyT1NFd3ZpRXM3U0YNCmlwTGNWc2dpUzg3ckI5ZFBRejFYTGhhdW9MTDliY3BlOE9sZW50VkI5VHFaU1lqaTJoeUNtZG5id25CS2QyMGVaUlh0S3QNCnh3SUpDdkpESGwyWk9wTVVUcnIydFcwSkVFZU1QSDJWMCs4amg3aGxlQ0NLcDhmdE1pcGVuWTdvelR1M1JVTUdNcjB4eTINCmhUalVJNkVGU0ppVGlKVE9ibGVhcGVPMVE1czdHaU5ibmdZQXFhN3h3RmJuYllrODJ3ekxPbzdEUjYzODhJbzVQcEFRQUENCkFNQUtXbURSMWU5bXlncm8wZmtQUDQ3dGsxMnF5bWpkQzVtRU1SNm9TOTNMbGRaK1ptKzBxVlBxN1BSQ3JPZlpLcFJSQ1UNCmJOUkM0ZFJhUHk0ek85cEdqdzE3ZlhjUGxGQzRaQUN1anhnRzhvazdYNEdGVlZEQ2lySFRySFhWN0ozNUtPMnR5MloyR2UNCms2L0dhMUpCMlBLN0tJZFlnMWpjY3lUR0FsZTlmcjIyU21nZHVoUmt2WlZsVU9mMHp2ZDhERzlVcktYUURWTERHd1QrWlkNClp2ODhYdGduZzZneU1jZXhZaHZZY04yMUo4ay9wNmM1ZGVuUXNNL0QxN0Qyck9iNE1BQUFEQkFPcDBJWitTVWxXY0xzbjMNCmVwQk1pTVAwdm5LUTI4UUd4NDl1bW14VXdhMTI0djk5YzhtTXZ5TXJPYnFsODdjZjQwWTlqdUhsSGZKSzd0MXhNdE5qU3QNCkJWRlNjU2E5Sk56S0hKRTJaYlJma1d1ZXpScytGbytKcjU0YVppQjNvcjNFeUtaamNZY2RFTG5ROHNjNmJXd25Ic29WSHkNCmNpTThtcUhudHRqeXJPZFdJRi9CTURlYjF5WkliYlQ0aWN3Y1N2TEJOVE95dllwakg1RWNsTXdXcWlsQ2NxVVJyTmtZVXMNCnJWZkFabDZuUmE5N0FNNDd6THhBT0RZT1FzbjZhdk5RQUFBTUVBNTk2ejRYZkxrQ09MT3drUi85NS90WEYzS3p4MjFsdC8NCllBVExmRlBKbHdNaGRxN1d2VG9LZWxNV0QwNUxXYlZxYitNOGU3SWZSQlducEp0V1RxMVBCY3ltT2k1TkprSmZnWWhqdGgNCjlqT1k4WTVCWWlvcENRUUFtTWc3SHF3a0xUSUdUU25IdDN5ZGFTK21TaVFTQUhLb1VKbmp4cEdLQ3ZyVGk5eHdxTFpZT1YNClZvOHFCZ003M1c1TWUyQWI0YnpPaEt4Tm9iTFpqWkxqZDJoeHRyWENJaityRXVRa09NT1hGTmR6NkFDR0hwQ09KTGp4clUNCmk4TGNwd2c5NlpWZkhCQUFBQUNtVnlaV0psUUhOMGVYZz0NCi0tLS0tRU5EIE9QRU5TU0ggUFJJVkFURSBLRVktLS0tLQ==").unwrap()).unwrap();
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
            "https://github.com/Qovery/engine-testing.git",
            "9a9c1f4373c8128151a9def9ea3d838fa2ed33e8",
            clone_dir.path(),
            &get_credentials,
        );
        assert!(matches!(repo, Ok(_)));
    }
}
