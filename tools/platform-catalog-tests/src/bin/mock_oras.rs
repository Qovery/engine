use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

fn required_path(name: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| fail(&format!("{name} is not set")))
}

fn fail(message: &str) -> ! {
    eprintln!("ERROR: {message}");
    process::exit(1);
}

fn assert_file_matches(expected_path: &Path, staged_path: &Path, subject: &str) {
    let expected = fs::read(expected_path)
        .unwrap_or_else(|error| fail(&format!("failed to read {}: {error}", expected_path.display())));
    let staged = fs::read(staged_path)
        .unwrap_or_else(|error| fail(&format!("failed to read {}: {error}", staged_path.display())));
    if staged != expected {
        fail(&format!("staged {subject} differs from the canonical {subject}"));
    }
}

fn relative_files(directory: &Path, prefix: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| fail(&format!("failed to read {}: {error}", directory.display())));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|error| fail(&format!("unreadable entry under {}: {error}", directory.display())))
            .path();
        let relative = prefix.join(path.file_name().unwrap_or_else(|| fail("directory entry has no name")));
        if path.is_dir() {
            relative_files(&path, &relative, files);
        } else {
            files.push(relative);
        }
    }
}

/// The staged directory must be a byte-identical copy of the canonical one: a missing, stale, or
/// extraneous vendored SDK file in a published bundle is a publication failure.
fn assert_directory_matches(expected_directory: &Path, staged_directory: &Path, subject: &str) {
    if !staged_directory.is_dir() {
        fail(&format!("staged component does not contain the vendored {subject}"));
    }
    let mut expected_files = Vec::new();
    relative_files(expected_directory, Path::new(""), &mut expected_files);
    expected_files.sort();
    let mut staged_files = Vec::new();
    relative_files(staged_directory, Path::new(""), &mut staged_files);
    staged_files.sort();
    if expected_files != staged_files {
        fail(&format!("staged {subject} file set differs from the canonical {subject}"));
    }
    for relative in &expected_files {
        assert_file_matches(&expected_directory.join(relative), &staged_directory.join(relative), subject);
    }
}

fn inspect_push() {
    let current_directory = env::current_dir().unwrap_or_else(|error| fail(&error.to_string()));
    let expected_component_directory = required_path("EXPECTED_COMPONENT_DIR");
    if current_directory == expected_component_directory {
        fail("executable component was pushed without an isolated staging directory");
    }

    if !current_directory.join("config/runtime-values/model.pkl").is_file() {
        fail("staged component does not contain its Pkl model");
    }

    assert_file_matches(
        &required_path("EXPECTED_CONTRACT"),
        &current_directory.join("config/runtime-values/contract.pkl"),
        "Pkl contract",
    );
    assert_directory_matches(
        &required_path("EXPECTED_SDK_DIR"),
        &current_directory.join("config/runtime-values/sdk"),
        "Pkl SDK",
    );

    let marker = required_path("MOCK_MARKER");
    fs::write(&marker, [])
        .unwrap_or_else(|error| fail(&format!("failed to write marker {}: {error}", marker.display())));
}

fn main() {
    let arguments: Vec<String> = env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("push") => inspect_push(),
        Some("manifest") => {
            println!("{{\"digest\":\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"}}");
        }
        _ => fail(&format!("unexpected oras command: {}", arguments.join(" "))),
    }
}
