use std::fs;
use std::process::Command;
use tera::Context;

/// Renders an actual template of the legacy `q-ingress-tls` chart in isolation, as the old focused Tera tests did.
pub fn render_router_template(template: &str, context: &Context) -> Result<String, tera::Error> {
    render_template_of_chart("q-ingress-tls", template, context)
}

/// Same as `render_router_template`, for the `q-ingress-tls-per-hostname` chart and its helpers.
#[allow(dead_code)]
pub fn render_per_hostname_router_template(template: &str, context: &Context) -> Result<String, tera::Error> {
    render_template_of_chart("q-ingress-tls-per-hostname", template, context)
}

fn render_template_of_chart(chart_name: &str, template: &str, context: &Context) -> Result<String, tera::Error> {
    let chart = tempfile::tempdir().expect("temporary chart");
    fs::create_dir(chart.path().join("templates")).expect("template directory");
    fs::write(
        chart.path().join("Chart.yaml"),
        "apiVersion: v2\nname: router-test\nversion: 0.1.0\n",
    )
    .expect("chart metadata");
    fs::write(chart.path().join("templates/template.yaml"), template).expect("native template");
    // Every helper file of the chart, so a template only sees the helpers of its own chart.
    let templates_directory = format!("{}/lib/common/charts/{chart_name}/templates", env!("CARGO_MANIFEST_DIR"));
    for entry in fs::read_dir(&templates_directory).expect("chart templates directory") {
        let path = entry.expect("chart template entry").path();
        let is_helper = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('_') && name.ends_with(".tpl"));
        if is_helper {
            fs::copy(
                &path,
                chart
                    .path()
                    .join("templates")
                    .join(path.file_name().expect("file name")),
            )
            .expect("native helpers");
        }
    }
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
