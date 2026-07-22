use std::env;
use std::fs;
use std::path::PathBuf;
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

fn inspect_push() {
    let current_directory = env::current_dir().unwrap_or_else(|error| fail(&error.to_string()));
    let expected_component_directory = required_path("EXPECTED_COMPONENT_DIR");
    if current_directory == expected_component_directory {
        fail("executable component was pushed without an isolated staging directory");
    }

    if !current_directory.join("config/runtime-values/model.pkl").is_file() {
        fail("staged component does not contain its Pkl model");
    }

    let expected_contract = required_path("EXPECTED_CONTRACT");
    let staged_contract = current_directory.join("config/runtime-values/contract.pkl");
    let expected = fs::read(&expected_contract)
        .unwrap_or_else(|error| fail(&format!("failed to read {}: {error}", expected_contract.display())));
    let staged = fs::read(&staged_contract)
        .unwrap_or_else(|error| fail(&format!("failed to read {}: {error}", staged_contract.display())));
    if staged != expected {
        fail("staged Pkl contract differs from the canonical contract");
    }

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
