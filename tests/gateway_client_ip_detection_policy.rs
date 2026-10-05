use std::process::Command;

use serde::Deserialize;
use serde_yaml::Value;

const CHART_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/lib/common/bootstrap/charts/qovery-cluster-gateway"
);

fn render_chart(x_forwarded_for_setting: Option<&str>) -> Vec<Value> {
    let mut command = Command::new("helm");
    command.args([
        "template",
        "client-ip-detection-test",
        CHART_PATH,
        "--namespace",
        "default",
    ]);
    if let Some(x_forwarded_for_setting) = x_forwarded_for_setting {
        command.args(["--set", x_forwarded_for_setting]);
    }

    let output = command
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

fn cluster_gateway_client_traffic_policy(manifests: &[Value]) -> &Value {
    manifests
        .iter()
        .find(|manifest| {
            manifest["kind"].as_str() == Some("ClientTrafficPolicy")
                && manifest["metadata"]["name"].as_str() == Some("qovery-cluster-public-gateway-client-traffic-policy")
        })
        .expect("gateway chart must render the cluster ClientTrafficPolicy")
}

#[test]
fn client_ip_detection_is_omitted_when_no_x_forwarded_for_setting_is_configured() {
    let manifests = render_chart(None);
    let policy = cluster_gateway_client_traffic_policy(&manifests);

    assert!(
        policy["spec"].get("clientIPDetection").is_none(),
        "an empty clientIPDetection object is invalid with Envoy Gateway v1.9.1"
    );
}

#[test]
fn client_ip_detection_uses_x_forwarded_for_when_trusted_hops_are_configured() {
    let manifests = render_chart(Some("gateway.qoveryPublic.xForwardedFor.numberTrustedHops=2"));
    let policy = cluster_gateway_client_traffic_policy(&manifests);

    assert_eq!(
        policy["spec"]["clientIPDetection"]["xForwardedFor"]["numTrustedHops"].as_i64(),
        Some(2)
    );
    assert_eq!(
        policy["spec"]["clientIPDetection"]
            .as_mapping()
            .map(|mapping| mapping.len()),
        Some(1),
        "Envoy Gateway v1.9.1 requires exactly one client IP detection strategy"
    );
}
