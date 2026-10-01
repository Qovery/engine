use std::fs;
use std::process::Command;
use tera::Context;

/// Renders an actual router template in isolation, as the old focused Tera tests did.
pub fn render_router_template(template: &str, context: &Context) -> Result<String, tera::Error> {
    let chart = tempfile::tempdir().expect("temporary chart");
    fs::create_dir(chart.path().join("templates")).expect("template directory");
    fs::write(
        chart.path().join("Chart.yaml"),
        "apiVersion: v2\nname: router-test\nversion: 0.1.0\n",
    )
    .expect("chart metadata");
    fs::write(chart.path().join("templates/template.yaml"), template).expect("native template");
    fs::write(
        chart.path().join("templates/_helpers.tpl"),
        include_str!("../../lib/common/charts/q-ingress-tls/templates/_helpers.tpl"),
    )
    .expect("native helpers");
    fs::write(
        chart.path().join("values.yaml"),
        serde_json::to_string(&context.clone().into_json()).expect("serialize values"),
    )
    .expect("chart values");
    let output = Command::new("helm")
        .args(["template", "test"])
        .arg(chart.path())
        .output()
        .expect("helm must be installed to test chart rendering");
    if !output.status.success() {
        return Err(tera::Error::msg(String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8(output.stdout).expect("Helm output must be UTF-8"))
}
