//! Drifts the SDK link check must reject, and the links `--write` must restore.

use platform_catalog_tests::{assert_success, repository_path, run};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const BUNDLE: &str = "platform-catalog/components/loki/config/runtime-values";
const STATIC_BUNDLE: &str = "platform-catalog/components/alloy/config/runtime-values";
const FIXTURE: &str = "platform-catalog/pkl/tests/fixtures/karpenter-v1/config/runtime-values";

fn write(root: &Path, relative: &str, content: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn catalog() -> TempDir {
    let root = TempDir::new().expect("temporary catalog must be created");
    let script = fs::read(repository_path("scripts/sync-platform-pkl-sdk.sh")).unwrap();
    write(root.path(), "scripts/sync-platform-pkl-sdk.sh", &script);
    write(root.path(), "platform-catalog/pkl/contract.pkl", b"// contract\n");
    write(root.path(), "platform-catalog/pkl/sdk/request.pkl", b"// request\n");
    write(root.path(), &format!("{BUNDLE}/model.pkl"), b"");
    write(root.path(), &format!("{STATIC_BUNDLE}/managed-values.yaml"), b"");
    write(root.path(), &format!("{FIXTURE}/model.pkl"), b"");
    root
}

fn sync(root: &Path, mode: &str) -> Command {
    let mut command = Command::new("bash");
    command.arg(root.join("scripts/sync-platform-pkl-sdk.sh")).arg(mode);
    command
}

#[test]
fn write_links_each_executable_bundle_and_check_accepts_the_result() {
    let root = catalog();
    assert_success(&mut sync(root.path(), "--write"));
    for (link, target) in [
        (format!("{BUNDLE}/contract.pkl"), "../../../../pkl/contract.pkl"),
        (format!("{BUNDLE}/sdk"), "../../../../pkl/sdk"),
        (format!("{FIXTURE}/contract.pkl"), "../../../../../contract.pkl"),
        (format!("{FIXTURE}/sdk"), "../../../../../sdk"),
    ] {
        assert_eq!(fs::read_link(root.path().join(&link)).unwrap(), Path::new(target), "{link}");
    }
    assert!(fs::symlink_metadata(root.path().join(STATIC_BUNDLE).join("sdk")).is_err());

    let rewrite = assert_success(&mut sync(root.path(), "--write"));
    assert!(rewrite.stdout.is_empty(), "a second write must leave correct links untouched");
    assert_success(&mut sync(root.path(), "--check"));
}

#[derive(Debug, Clone, Copy)]
enum Drift {
    MissingLink,
    CopiedFile,
    CopiedDirectory,
    AbsoluteLink,
    LinkInStaticBundle,
    NestedLink,
    LinkedConfig,
}

impl Drift {
    fn apply(self, root: &Path) {
        let bundle = root.join(BUNDLE);
        match self {
            Drift::MissingLink => fs::remove_file(bundle.join("contract.pkl")).unwrap(),
            Drift::CopiedFile => {
                fs::remove_file(bundle.join("contract.pkl")).unwrap();
                fs::copy(root.join("platform-catalog/pkl/contract.pkl"), bundle.join("contract.pkl")).unwrap();
            }
            Drift::CopiedDirectory => {
                fs::remove_file(bundle.join("sdk")).unwrap();
                write(&bundle, "sdk/request.pkl", b"// request\n");
                write(&bundle, "sdk/extraneous.pkl", b"");
            }
            Drift::AbsoluteLink => {
                fs::remove_file(bundle.join("sdk")).unwrap();
                symlink(root.join("platform-catalog/pkl/sdk"), bundle.join("sdk")).unwrap();
            }
            Drift::LinkInStaticBundle => symlink("../../../../pkl/sdk", root.join(STATIC_BUNDLE).join("sdk")).unwrap(),
            Drift::NestedLink => {
                fs::create_dir(bundle.join("storage")).unwrap();
                symlink("../../../../../pkl/sdk", bundle.join("storage/sdk")).unwrap();
            }
            Drift::LinkedConfig => {
                let config = root.join("platform-catalog/components/alloy/config");
                fs::rename(&config, root.join("platform-catalog/alloy-config")).unwrap();
                symlink("../../alloy-config", &config).unwrap();
            }
        }
    }

    fn diagnostic(self) -> &'static str {
        match self {
            Drift::MissingLink => "loki/config/runtime-values/contract.pkl is missing",
            Drift::CopiedFile => "loki/config/runtime-values/contract.pkl is a file or directory instead of a link",
            Drift::CopiedDirectory => "loki/config/runtime-values/sdk is a file or directory instead of a link",
            Drift::AbsoluteLink => "/platform-catalog/pkl/sdk instead of ../../../../pkl/sdk",
            Drift::LinkInStaticBundle => "alloy/config/runtime-values/sdk is an unmanaged symbolic link",
            Drift::NestedLink => "loki/config/runtime-values/storage/sdk is an unmanaged symbolic link",
            Drift::LinkedConfig => "components/alloy/config is an unmanaged symbolic link",
        }
    }

    fn repaired_by_write(self) -> bool {
        !matches!(self, Drift::LinkInStaticBundle | Drift::NestedLink | Drift::LinkedConfig)
    }
}

#[test]
fn check_rejects_every_drift_and_write_repairs_the_managed_links() {
    for drift in [
        Drift::MissingLink,
        Drift::CopiedFile,
        Drift::CopiedDirectory,
        Drift::AbsoluteLink,
        Drift::LinkInStaticBundle,
        Drift::NestedLink,
        Drift::LinkedConfig,
    ] {
        let root = catalog();
        assert_success(&mut sync(root.path(), "--write"));
        drift.apply(root.path());

        let check = run(&mut sync(root.path(), "--check"));
        let stderr = String::from_utf8_lossy(&check.stderr);
        assert!(!check.status.success(), "{drift:?} passed the check");
        assert!(
            stderr.contains(drift.diagnostic()),
            "{drift:?}: unexpected diagnostic:\n{stderr}"
        );

        let repair = run(&mut sync(root.path(), "--write"));
        assert_eq!(repair.status.success(), drift.repaired_by_write(), "{drift:?}");
        if drift.repaired_by_write() {
            assert_success(&mut sync(root.path(), "--check"));
        }
    }
}
