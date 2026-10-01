use std::process::Command;

use serde::Deserialize;
use serde_yaml::Value;

const CHART_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/lib/common/bootstrap/charts/qovery-cluster-gateway"
);
const HTTP_LISTENER: &str = "default/qovery-cluster-public-gateway/http";
const HTTPS_LISTENER: &str = "default/qovery-cluster-public-gateway/https";

fn render_chart(compression_enabled: bool) -> Vec<Value> {
    let output = Command::new("helm")
        .args([
            "template",
            "compression-test",
            CHART_PATH,
            "--namespace",
            "default",
            "--set",
            "gateway.qoveryPublic.enable=true",
            "--set",
            if compression_enabled {
                "gateway.qoveryPublic.compression.enable=true"
            } else {
                "gateway.qoveryPublic.compression.enable=false"
            },
        ])
        .output()
        .expect("helm must be available to render the Qovery cluster gateway chart");

    assert!(
        output.status.success(),
        "chart rendering failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_yaml::Deserializer::from_slice(&output.stdout)
        .map(|document| Value::deserialize(document).expect("rendered chart must contain valid YAML"))
        .filter(|document| !document.is_null())
        .collect()
}

fn compression_patch(manifests: &[Value]) -> Option<&Value> {
    manifests.iter().find(|manifest| {
        manifest["kind"].as_str() == Some("EnvoyPatchPolicy")
            && manifest["metadata"]["name"].as_str() == Some("qovery-cluster-public-gateway-compression-patch")
    })
}

#[test]
fn compression_patch_creates_response_direction_config_that_excludes_206() {
    let manifests = render_chart(true);
    let patch = compression_patch(&manifests).expect("compression must render an EnvoyPatchPolicy");
    let json_patches = patch["spec"]["jsonPatches"]
        .as_sequence()
        .expect("EnvoyPatchPolicy must contain JSON patches");

    assert_eq!(json_patches.len(), 2, "one patch is required for each public listener");

    for (json_patch, listener_name) in json_patches.iter().zip([HTTP_LISTENER, HTTPS_LISTENER]) {
        assert_eq!(json_patch["name"].as_str(), Some(listener_name));
        assert_eq!(json_patch["operation"]["op"].as_str(), Some("add"));
        assert_eq!(
            json_patch["operation"]["path"].as_str(),
            Some("/response_direction_config"),
            "the patch must create the missing parent object before setting its fields"
        );
        assert_eq!(
            json_patch["operation"]["value"]["uncompressible_response_codes"]
                .as_sequence()
                .and_then(|codes| codes.first())
                .and_then(Value::as_u64),
            Some(206)
        );
    }
}

#[test]
fn compression_patch_is_absent_when_compression_is_disabled() {
    let manifests = render_chart(false);

    assert!(
        compression_patch(&manifests).is_none(),
        "compression disabled must not enable EnvoyPatchPolicy"
    );
}
