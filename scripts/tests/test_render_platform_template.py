import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
class RenderPlatformTemplateTest(unittest.TestCase):
    registry = "public.ecr.aws/r3m4q3r9"
    digest = "sha256:" + "a" * 64

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.temp = Path(self.temp_dir.name)
        self.source = ROOT / "platform-catalog/templates/qovery-cluster-v0/template.yaml"
        self.destination = self.temp / "template.yaml"
        self.config_output = self.temp / "platform-config-publish.json"
        self.chart_output = self.temp / "frozen-charts-publish.json"
        self.config_entries = [
            self.config_entry("qovery-operator", "v1"),
            self.config_entry("cluster-agent", "v2"),
            self.config_entry("shell-agent", "v2"),
            self.config_entry("loki", "v5"),
        ]
        self.chart_entries = [
            self.chart_entry("qovery-operator", "0.2.0"),
            self.chart_entry("qovery-cluster-agent", "0.1.0"),
            self.chart_entry("qovery-shell-agent", "0.1.0"),
            self.chart_entry("loki", "6.55.0"),
        ]

    def test_renders_every_config_pin_from_verified_publication_outputs(self):
        self.write_outputs()

        self.render()

        rendered = self.destination.read_text(encoding="utf-8")
        self.assertNotIn("__PUBLISHED_CONFIG_DIGEST__", rendered)
        self.assertEqual(rendered.count(self.digest), 4)
        self.assertIn("repository: oci://public.ecr.aws/r3m4q3r9/charts/", rendered)

    def test_missing_referenced_config_fails_before_a_template_is_written(self):
        self.config_entries = self.config_entries[1:]
        self.write_outputs()

        result = self.render(check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("configRef qovery-operator:v1 has no verified publication", result.stderr)
        self.assertFalse(self.destination.exists())

    def test_wrong_chart_version_fails_complete_graph_validation(self):
        self.chart_entries[0] = self.chart_entry("qovery-operator", "9.9.9")
        self.write_outputs()

        result = self.render(check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("chart qovery-operator:0.2.0 has no verified publication", result.stderr)

    def test_catalog_coordinate_must_match_the_template_identity(self):
        self.write_outputs()

        result = self.render(version="0.2.0", check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("catalog expects qovery-cluster-v0:0.2.0", result.stderr)

    def render(self, version="0.1.0", check=True):
        return subprocess.run(
            [
                ROOT / "scripts/publish-platform-catalog.sh",
                "render",
                self.source,
                self.config_output,
                self.chart_output,
                self.destination,
                "qovery-cluster-v0",
                version,
                self.registry,
            ],
            check=check,
            capture_output=True,
            text=True,
        )

    def write_outputs(self):
        self.config_output.write_text(json.dumps(self.config_entries), encoding="utf-8")
        self.chart_output.write_text(json.dumps(self.chart_entries), encoding="utf-8")

    def config_entry(self, component, version):
        return {
            "component": component,
            "version": version,
            "ref": f"{self.registry}/platform-config/{component}:{version}",
            "digest": self.digest,
        }

    def chart_entry(self, chart, version):
        return {
            "chart": chart,
            "version": version,
            "ref": f"{self.registry}/charts/{chart}:{version}",
            "digest": self.digest,
        }


if __name__ == "__main__":
    unittest.main()
