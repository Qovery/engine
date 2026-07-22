use platform_catalog_tests::{REGISTRY, assert_success, repository_path};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tempfile::TempDir;

const TEST_REGISTRY: &str = "registry.invalid/qovery";
const ZERO_DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

#[test]
fn executable_component_publication_uses_isolated_staging_and_injects_the_contract() {
    let temporary_directory = TempDir::new().expect("temporary directory must be created");
    let mock_bin_directory = temporary_directory.path().join("bin");
    fs::create_dir(&mock_bin_directory).expect("mock binary directory must be created");
    let mock_oras = mock_bin_directory.join("oras");
    fs::copy(env!("CARGO_BIN_EXE_mock_oras"), &mock_oras).expect("mock oras binary must be copied");
    let mut permissions = fs::metadata(&mock_oras)
        .expect("mock oras metadata must be readable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&mock_oras, permissions).expect("mock oras must be executable");

    let marker = temporary_directory.path().join("oras-push-inspected");
    let output_file = temporary_directory.path().join("platform-config-publish.json");
    let path = env::join_paths(
        std::iter::once(mock_bin_directory.clone()).chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
    )
    .expect("test PATH must be valid");

    let mut command = Command::new(repository_path("scripts/publish-platform-config.sh"));
    command
        .arg("cluster-agent")
        .env("PATH", path)
        .env(
            "EXPECTED_COMPONENT_DIR",
            repository_path("platform-catalog/components/cluster-agent"),
        )
        .env(
            "EXPECTED_CONTRACT",
            repository_path("platform-catalog/pkl/component-contract.pkl"),
        )
        .env("MOCK_MARKER", &marker)
        .env("PLATFORM_CONFIG_REGISTRY", TEST_REGISTRY)
        .env("PLATFORM_CONFIG_OUTPUT_FILE", &output_file);
    assert_success(&mut command);

    assert!(marker.is_file(), "mocked ORAS push was not called");
    let publications: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(&output_file).expect("publication output must be readable"))
            .expect("publication output must be valid JSON");
    assert_eq!(publications.len(), 1);
    let publication = &publications[0];
    assert_eq!(publication["component"].as_str(), Some("cluster-agent"));
    let version = publication["version"]
        .as_str()
        .expect("publication version must be a string");
    let version_digits = version.strip_prefix('v').unwrap_or_default();
    assert!(
        !version_digits.is_empty() && version_digits.chars().all(|character| character.is_ascii_digit()),
        "unexpected component version {version}"
    );
    assert_eq!(
        publication["ref"].as_str(),
        Some(format!("{TEST_REGISTRY}/platform-config/cluster-agent:{version}").as_str())
    );
    assert_eq!(publication["digest"].as_str(), Some(ZERO_DIGEST));

    // The public production registry is never contacted by this test.
    assert!(
        !publication["ref"]
            .as_str()
            .expect("publication ref must be a string")
            .starts_with(REGISTRY)
    );
}
