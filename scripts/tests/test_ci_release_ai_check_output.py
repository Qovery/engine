import os
import sys
import unittest
from io import StringIO

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from ci_release_ai_check_output import (
    SEVERITY_EMOJI, SEVERITY_COLOR, color, strip_ansi, should_use_color,
    _normalize_title, merge_findings, finding_group_key, DefaultRenderer,
)


def _finding(severity="review", title="t", source="terraform", clusters=None, category="other", impact="i", action="a", resource=""):
    return {
        "severity": severity, "title": title, "source": source, "category": category,
        "impact": impact, "action": action, "affected_clusters": clusters or ["c1"],
        "resource": resource,
    }


class TestNormalizeTitle(unittest.TestCase):
    def test_strips_bucket_names(self):
        a = _normalize_title("HTTPS-only policy on qovery-logs-za0829ee2")
        b = _normalize_title("HTTPS-only policy on qovery-logs-zeb1bc33b")
        self.assertEqual(a, b)

    def test_strips_uuids_and_short_cluster_ids(self):
        a = _normalize_title("OIDC thumbprint changing on e3b8921c-889b-462e-b19e-023a1c4b1f40")
        b = _normalize_title("OIDC thumbprint changing on ze331ea83")
        self.assertEqual(a, b)

    def test_strips_version_numbers(self):
        a = _normalize_title("Engine version v1.301.0 -> v1.312.0")
        b = _normalize_title("Engine version v2.0.1 -> v2.5.9")
        self.assertEqual(a, b)

    def test_different_issues_stay_distinct(self):
        https = _normalize_title("New S3 bucket policies enforcing HTTPS-only access")
        iam = _normalize_title("IAM policy losing S3 and KMS Allow actions")
        kms = _normalize_title("KMS key being replaced on the Loki bucket")
        self.assertNotEqual(https, iam)
        self.assertNotEqual(https, kms)
        self.assertNotEqual(iam, kms)


class TestGroupKey(unittest.TestCase):
    def test_same_issue_different_cluster_tokens_same_key(self):
        a = finding_group_key(_finding(title="HTTPS policy on qovery-logs-za0829ee2"))
        b = finding_group_key(_finding(title="HTTPS policy on qovery-logs-zeb1bc33b"))
        self.assertEqual(a, b)

    def test_distinct_issues_distinct_keys(self):
        # The regression: these previously both collapsed to ("terraform", "s3_encryption").
        https = finding_group_key(_finding(title="New S3 bucket policies enforcing HTTPS-only access"))
        iam = finding_group_key(_finding(title="IAM policy losing S3 and KMS Allow actions"))
        self.assertNotEqual(https, iam)

    def test_source_is_part_of_key(self):
        tf = finding_group_key(_finding(title="same words", source="terraform"))
        helm = finding_group_key(_finding(title="same words", source="helm"))
        self.assertNotEqual(tf, helm)

    def test_same_resource_and_category_paraphrased_titles_same_key(self):
        # The regression: independent Claude calls paraphrase the same issue
        # ("modified"/"update"/"expanded"/"narrowed"...), so a title-based key left
        # ~50 near-duplicate findings for ONE trust-policy change. When the model
        # attributes a finding to a concrete resource, key on that instead of prose.
        a = finding_group_key(_finding(title="IAM trust policy modified for Prometheus IRSA role",
                                       resource="aws_iam_role.iam_eks_prometheus", category="iam_change"))
        b = finding_group_key(_finding(title="IAM trust policy update for Prometheus/Thanos IRSA role",
                                       resource="aws_iam_role.iam_eks_prometheus", category="iam_change"))
        self.assertEqual(a, b)

    def test_same_resource_different_category_distinct_keys(self):
        a = finding_group_key(_finding(resource="aws_iam_role.x", category="iam_change"))
        b = finding_group_key(_finding(resource="aws_iam_role.x", category="resource_deletion"))
        self.assertNotEqual(a, b)

    def test_resource_key_source_still_matters(self):
        tf = finding_group_key(_finding(resource="cert-manager", category="helm_values", source="terraform"))
        helm = finding_group_key(_finding(resource="cert-manager", category="helm_values", source="helm"))
        self.assertNotEqual(tf, helm)

    def test_resource_key_and_title_key_never_collide(self):
        # A resource string that happens to equal another finding's normalized title
        # must not fold the two findings together.
        by_resource = finding_group_key(_finding(title="anything", resource="same words", category="other"))
        by_title = finding_group_key(_finding(title="same words", resource=""))
        self.assertNotEqual(by_resource, by_title)


class TestMergeFindings(unittest.TestCase):
    def test_merges_paraphrased_findings_on_same_resource(self):
        merged = merge_findings([
            _finding(title="IAM trust policy modified for Prometheus IRSA role", clusters=["c1"],
                     resource="aws_iam_role.iam_eks_prometheus", category="iam_change"),
            _finding(title="IAM trust policy update for Prometheus IRSA role", clusters=["c2", "c3"],
                     resource="aws_iam_role.iam_eks_prometheus", category="iam_change"),
            _finding(title="IAM role trust policy expanded for Thanos service accounts", clusters=["c4"],
                     resource="aws_iam_role.iam_eks_prometheus", category="iam_change"),
        ])
        self.assertEqual(len(merged), 1)
        self.assertEqual(sorted(merged[0]["affected_clusters"]), ["c1", "c2", "c3", "c4"])
        self.assertEqual(merged[0]["resource"], "aws_iam_role.iam_eks_prometheus")

    def test_different_resources_stay_separate_even_with_same_title(self):
        # Over-separation is the safe failure mode: two resources sharing a title are
        # two findings, each keeping its own resource attribution.
        merged = merge_findings([
            _finding(title="S3 bucket encryption updated", clusters=["c1"], resource="aws_s3.loki"),
            _finding(title="S3 bucket encryption updated", clusters=["c2", "c3"], resource="aws_s3.prometheus"),
        ])
        self.assertEqual(len(merged), 2)
        self.assertEqual(merged[0]["resource"], "aws_s3.prometheus")  # largest group first
        self.assertEqual(merged[1]["resource"], "aws_s3.loki")

    def test_unions_clusters_across_paraphrased_titles_without_resource(self):
        # Findings with no resource attribution still merge via the normalized title.
        merged = merge_findings([
            _finding(title="HTTPS policy on qovery-logs-za0829ee2", clusters=["c1"]),
            _finding(title="HTTPS policy on qovery-logs-zeb1bc33b", clusters=["c2", "c3"]),
        ])
        self.assertEqual(len(merged), 1)
        self.assertEqual(sorted(merged[0]["affected_clusters"]), ["c1", "c2", "c3"])

    def test_mixed_resource_and_no_resource_split_but_same_resource_merges(self):
        # Same resource+category merges regardless of title wording; the member with
        # no resource attribution keys by title and stays its own finding.
        merged = merge_findings([
            _finding(severity="critical", title="kms key deletion", clusters=["c1"], resource="aws_kms.a"),
            _finding(severity="review", title="kms key being removed", clusters=["c2", "c3"], resource="aws_kms.a"),
            _finding(severity="review", title="kms key deletion", clusters=["c4"], resource=""),
        ])
        self.assertEqual(len(merged), 2)
        by_resource = next(f for f in merged if f["resource"] == "aws_kms.a")
        self.assertEqual(by_resource["severity"], "critical")
        self.assertEqual(sorted(by_resource["affected_clusters"]), ["c1", "c2", "c3"])

    def test_distinct_issues_stay_separate_sorted_by_size(self):
        merged = merge_findings([
            _finding(title="IAM policy losing Allow actions", clusters=["c1"]),
            _finding(title="New S3 bucket policies enforcing HTTPS-only access", clusters=["c2", "c3"]),
        ])
        self.assertEqual(len(merged), 2)
        # largest group first
        self.assertEqual(len(merged[0]["affected_clusters"]), 2)

    def test_idempotent(self):
        once = merge_findings([
            _finding(title="HTTPS policy on qovery-logs-za0829ee2", clusters=["c1"]),
            _finding(title="HTTPS policy on qovery-logs-zeb1bc33b", clusters=["c2"]),
        ])
        twice = merge_findings(once)
        self.assertEqual(len(twice), 1)
        self.assertEqual(sorted(twice[0]["affected_clusters"]), ["c1", "c2"])

    def test_representative_is_most_severe_not_largest(self):
        # A benign finding on many clusters must NOT mask a critical one that normalizes
        # to the same title (e.g. "max_size 6 -> 0" vs "6 -> 4", digits stripped). The
        # critical member's severity AND wording must survive to the verdict pass.
        merged = merge_findings([
            _finding(severity="critical", title="node group max_size 6 to 0",
                     clusters=["c1"], impact="scales pool to zero", action="STOP"),
            _finding(severity="review", title="node group max_size 6 to 4",
                     clusters=["c2", "c3"], impact="routine downscale", action="verify"),
        ])
        self.assertEqual(len(merged), 1)
        self.assertEqual(merged[0]["severity"], "critical")
        self.assertEqual(merged[0]["impact"], "scales pool to zero")
        self.assertEqual(merged[0]["action"], "STOP")
        self.assertEqual(sorted(merged[0]["affected_clusters"]), ["c1", "c2", "c3"])

    def test_metadata_comes_from_representative(self):
        # No Frankenstein merge: single-valued metadata (grafana_url, severity, category)
        # all come from the same representative member, so they never contradict the shown
        # text. (Resources are the exception — aggregated across members by design.)
        first = _finding(title="HTTPS policy tightened on logs bucket", clusters=["c1"],
                         resource="aws_s3.logs")
        first["grafana_url"] = "https://grafana/small"
        larger = _finding(title="HTTPS-only policy enforced on logs bucket", clusters=["c2", "c3"],
                          resource="aws_s3.logs")
        larger["grafana_url"] = "https://grafana/big"
        merged = merge_findings([first, larger])
        self.assertEqual(len(merged), 1)
        self.assertEqual(merged[0]["grafana_url"], "https://grafana/big")


class TestColorUtilities(unittest.TestCase):
    def test_severity_emoji_maps_all_levels(self):
        self.assertEqual(SEVERITY_EMOJI["critical"], "🔴")
        self.assertEqual(SEVERITY_EMOJI["review"], "🟡")
        self.assertEqual(SEVERITY_EMOJI["info"], "🟢")
        self.assertEqual(SEVERITY_EMOJI["unknown"], "⚪")

    def test_color_wraps_text_with_ansi(self):
        result = color("hello", "red")
        self.assertIn("\033[", result)
        self.assertIn("hello", result)
        self.assertTrue(result.endswith("\033[0m"))

    def test_strip_ansi_removes_escape_sequences(self):
        colored = color("hello", "red")
        self.assertEqual(strip_ansi(colored), "hello")

    def test_strip_ansi_preserves_plain_text(self):
        self.assertEqual(strip_ansi("hello world"), "hello world")

    def test_should_use_color_false_for_non_tty(self):
        fake_stream = StringIO()
        self.assertFalse(should_use_color(fake_stream))


from ci_release_ai_check_output import ProgressReporter


class TestProgressReporter(unittest.TestCase):
    def test_non_tty_prints_start_line(self):
        err = StringIO()
        p = ProgressReporter(stream=err, is_tty=False)
        p.start("download", 91)
        self.assertIn("Downloading logs for 91 clusters", err.getvalue())

    def test_non_tty_prints_finish_line(self):
        err = StringIO()
        p = ProgressReporter(stream=err, is_tty=False)
        p.start("download", 91)
        p.finish("download", "Downloaded 91 clusters (89 with logs, 2 no logs, 0 errors)")
        self.assertIn("Downloaded 91 clusters", err.getvalue())

    def test_tty_update_uses_carriage_return(self):
        err = StringIO()
        p = ProgressReporter(stream=err, is_tty=True)
        p.start("download", 91)
        p.update("download", 45, 91)
        output = err.getvalue()
        self.assertIn("\r", output)
        self.assertIn("45/91", output)

    def test_tty_finish_replaces_progress_line(self):
        err = StringIO()
        p = ProgressReporter(stream=err, is_tty=True)
        p.start("download", 91)
        p.update("download", 91, 91)
        p.finish("download", "Downloaded 91 clusters (89 with logs, 2 no logs, 0 errors)")
        output = err.getvalue()
        self.assertIn("✅", output)
        self.assertIn("Downloaded 91 clusters", output)

    def test_verbose_per_cluster_line(self):
        err = StringIO()
        p = ProgressReporter(stream=err, is_tty=False, verbose=True)
        p.verbose_detail("  [1/91] cluster-abc... 570 lines")
        self.assertIn("[1/91]", err.getvalue())

    def test_non_verbose_suppresses_detail(self):
        err = StringIO()
        p = ProgressReporter(stream=err, is_tty=False, verbose=False)
        p.verbose_detail("  [1/91] cluster-abc... 570 lines")
        self.assertEqual(err.getvalue(), "")


from ci_release_ai_check_output import DefaultRenderer

SAMPLE_REPORT = {
    "window": {
        "from_utc": "2026-04-14 07:58 UTC",
        "to_utc": "08:58 UTC",
        "start_ns": 1744617504000000000,
        "end_ns": 1744621104000000000,
    },
    "clusters_total": 10,
    "clusters_with_logs": 8,
    "clusters_no_logs": ["no-log-1", "no-log-2"],
    "clusters_errored": [],
    "clusters_skipped": [],
    "patterns_total": 3,
    "verdict": "Proceed with review",
    "verdict_severity": "review",
    "severity_counts": {"critical": 0, "review": 1, "info": 2, "unknown": 2},
    "findings": [
        {
            "severity": "review",
            "title": "Network route path changed",
            "impact": "traffic may shift from VPC peering to NAT",
            "source": "terraform",
            "resource": "aws_route_table.rt",
            "category": "network",
            "action": "verify connectivity expectations",
            "affected_clusters": ["cluster-a", "cluster-b"],
            "grafana_url": "https://grafana/d/x",
        },
        {
            "severity": "info",
            "title": "Cluster-agent image update",
            "impact": "expected rollout",
            "source": "helm",
            "category": "helm_values",
            "action": "none",
            "affected_clusters": ["cluster-c", "cluster-d", "cluster-e"],
            "grafana_url": "https://grafana/d/y",
        },
        {
            "severity": "info",
            "title": "S3 encryption normalization",
            "impact": "no functional change",
            "source": "terraform",
            "category": "other",
            "action": "none",
            "affected_clusters": ["cluster-f", "cluster-g", "cluster-h"],
            "grafana_url": "https://grafana/d/z",
        },
    ],
    "usage": {
        "model_analysis": "claude-haiku-4-5-20251001",
        "model_verdict": "claude-sonnet-4-6",
        "input_tokens": 142380,
        "output_tokens": 3210,
        "estimated_cost_usd": 0.1267,
    },
    "timing": {
        "download_s": 12.4,
        "grouping_s": 0.1,
        "analysis_s": 18.7,
        "verdict_s": 2.1,
        "total_s": 33.3,
    },
    "grafana_url": "https://qortal.qovery.com/grafana/d/ae51ecxhq2tj4a/infra-cluster-diff?orgId=1&from=2026-04-14T07:58:24.000Z&to=2026-04-14T08:58:24.000Z&timezone=utc",
}


class TestDefaultRenderer(unittest.TestCase):
    def _render(self, report=None):
        out = StringIO()
        r = DefaultRenderer(stream=out, use_color=False)
        r.render(report or SAMPLE_REPORT)
        return out.getvalue()

    def test_verdict_in_first_5_lines(self):
        output = self._render()
        lines = output.strip().splitlines()
        first_5 = "\n".join(lines[:5])
        self.assertIn("VERDICT", first_5)

    def test_summary_table_present(self):
        output = self._render()
        self.assertIn("Severity", output)
        self.assertIn("Patterns", output)
        self.assertIn("Clusters", output)
        self.assertIn("CRITICAL", output)
        self.assertIn("REVIEW", output)
        self.assertIn("INFO", output)
        self.assertIn("UNKNOWN", output)

    def test_review_findings_show_impact_and_action(self):
        output = self._render()
        self.assertIn("Network route path changed", output)
        self.assertIn("Impact:", output)
        self.assertIn("traffic may shift", output)
        self.assertIn("Action:", output)
        self.assertIn("verify connectivity", output)

    def test_review_findings_show_resource(self):
        output = self._render()
        self.assertIn("Resource:", output)
        self.assertIn("aws_route_table.rt", output)

    def test_finding_without_resource_omits_line(self):
        report = dict(SAMPLE_REPORT)
        report["findings"] = [_finding(title="No resource here", resource="")]
        output = self._render(report)
        self.assertIn("No resource here", output)
        self.assertNotIn("Resource:", output)

    def test_review_findings_show_all_cluster_ids(self):
        output = self._render()
        self.assertIn("cluster-a", output)
        self.assertIn("cluster-b", output)

    def test_info_findings_show_title_and_clusters(self):
        output = self._render()
        self.assertIn("Cluster-agent image update", output)
        self.assertIn("cluster-c", output)

    def test_blank_line_between_findings(self):
        report = dict(SAMPLE_REPORT)
        report["findings"] = [
            _finding(title="First review issue", severity="review", clusters=["c1"]),
            _finding(title="Second review issue", severity="review", clusters=["c2"]),
        ]
        output = self._render(report)
        lines = output.splitlines()
        idx = next(i for i, l in enumerate(lines) if "Second review issue" in l)
        self.assertEqual(lines[idx - 1], "")

    def test_unknown_section_shows_no_log_clusters(self):
        output = self._render()
        self.assertIn("no-log-1", output)
        self.assertIn("no-log-2", output)

    def test_footer_present(self):
        output = self._render()
        self.assertIn("grafana", output)
        self.assertIn("AI-generated analysis", output)

    def test_no_timing_or_cost_in_default(self):
        output = self._render()
        self.assertNotIn("Timing", output)
        self.assertNotIn("API cost", output)

    def test_omits_empty_severity_sections(self):
        report = dict(SAMPLE_REPORT)
        report["findings"] = []
        report["severity_counts"] = {"critical": 0, "review": 0, "info": 0, "unknown": 0}
        report["clusters_no_logs"] = []
        output = self._render(report)
        self.assertNotIn("Review findings", output)
        self.assertNotIn("Info findings", output)
        self.assertNotIn("Unknown", output)


LABELED_REPORT = {
    "window": {"from_utc": "2026-07-01 12:13 UTC", "to_utc": "12:45 UTC", "start_ns": 1, "end_ns": 2},
    "clusters_total": 3, "clusters_with_logs": 3,
    "clusters_no_logs": [], "clusters_errored": [], "clusters_skipped": [],
    "patterns_total": 3,
    "verdict": "Review required", "verdict_severity": "review",
    "severity_counts": {"critical": 0, "review": 3, "info": 0, "unknown": 0},
    # Findings 0 and 1 normalize to the same title (cluster tokens stripped) -> merge to R1.
    # Finding 2 is a distinct issue -> R2.
    "findings": [
        {"id": 0, "severity": "review", "title": "HTTPS policy on qovery-logs-za0829ee2",
         "impact": "i", "source": "terraform", "resource": "aws_s3_bucket.logs",
         "category": "other", "action": "a", "affected_clusters": ["c1"], "grafana_url": ""},
        {"id": 1, "severity": "review", "title": "HTTPS policy on qovery-logs-zeb1bc33b",
         "impact": "i", "source": "terraform", "resource": "aws_s3_bucket.logs",
         "category": "other", "action": "a", "affected_clusters": ["c2"], "grafana_url": ""},
        {"id": 2, "severity": "review", "title": "EKS node group max_size reduced",
         "impact": "i", "source": "terraform", "resource": "aws_eks_node_group.workers",
         "category": "node_pool", "action": "a", "affected_clusters": ["c3"], "grafana_url": ""},
    ],
    "usage": {}, "timing": {}, "grafana_url": "https://grafana/x", "qovery_cluster_names": {},
}


class TestFindingLabels(unittest.TestCase):
    def _render(self, report):
        out = StringIO()
        DefaultRenderer(stream=out, use_color=False).render(report)
        return out.getvalue()

    def test_findings_get_r_labels(self):
        out = self._render(LABELED_REPORT)
        self.assertIn("[R1]", out)
        self.assertIn("[R2]", out)


from ci_release_ai_check_output import VerboseRenderer

VERBOSE_REPORT = dict(SAMPLE_REPORT)
VERBOSE_REPORT["findings"] = [
    {
        "severity": "review",
        "title": "Network route path changed",
        "impact": "traffic may shift from VPC peering to NAT",
        "source": "terraform",
        "category": "network",
        "action": "verify connectivity expectations",
        "affected_clusters": ["cluster-a", "cluster-b"],
        "grafana_url": "https://grafana/d/x",
        "sample_diff": "[terraform] route removed\n- cidr_block = 10.0.0.0/16\n+ cidr_block = 10.1.0.0/16",
    },
    {
        "severity": "info",
        "title": "Cluster-agent image update",
        "impact": "expected rollout",
        "source": "helm",
        "category": "helm_values",
        "action": "none",
        "affected_clusters": ["cluster-c"],
        "grafana_url": "https://grafana/d/y",
        "sample_diff": "[helm] image tag updated",
    },
]


class TestVerboseRenderer(unittest.TestCase):
    def _render(self, report=None):
        out = StringIO()
        r = VerboseRenderer(stream=out, use_color=False)
        r.render(report or VERBOSE_REPORT)
        return out.getvalue()

    def test_shows_timing_block(self):
        output = self._render()
        self.assertIn("Timing", output)
        self.assertIn("Download:", output)
        self.assertIn("12.4s", output)
        self.assertIn("Total:", output)

    def test_shows_api_cost_block(self):
        output = self._render()
        self.assertIn("API cost", output)
        self.assertIn("claude-haiku-4-5-20251001", output)
        self.assertIn("142,380", output)

    def test_review_findings_include_sample_diff(self):
        output = self._render()
        self.assertIn("Sample diff:", output)
        self.assertIn("route removed", output)

    def test_info_findings_show_impact_and_action(self):
        output = self._render()
        self.assertIn("Cluster-agent image update", output)
        self.assertIn("Impact:", output)
        self.assertIn("expected rollout", output)

    def test_sample_diff_truncated_to_10_lines(self):
        report = dict(VERBOSE_REPORT)
        long_diff = "\n".join([f"line {i}" for i in range(20)])
        report["findings"] = [{
            "severity": "review",
            "title": "Big diff",
            "impact": "lots of changes",
            "source": "terraform",
            "category": "other",
            "action": "review",
            "affected_clusters": ["c1"],
            "grafana_url": "",
            "sample_diff": long_diff,
        }]
        output = self._render(report)
        self.assertIn("line 0", output)
        self.assertIn("line 9", output)
        self.assertNotIn("line 10", output)
        self.assertIn("...", output)


import json
import tempfile
from ci_release_ai_check_output import JsonRenderer, create_renderer


class TestJsonRenderer(unittest.TestCase):
    def test_renders_valid_json_to_stream(self):
        out = StringIO()
        r = JsonRenderer(stream=out)
        r.render(SAMPLE_REPORT)
        data = json.loads(out.getvalue())
        self.assertEqual(data["version"], 1)
        self.assertEqual(data["verdict"], "Proceed with review")
        self.assertEqual(data["verdict_severity"], "review")

    def test_json_contains_all_top_level_keys(self):
        out = StringIO()
        r = JsonRenderer(stream=out)
        r.render(SAMPLE_REPORT)
        data = json.loads(out.getvalue())
        expected_keys = {
            "version", "window", "verdict", "verdict_severity",
            "clusters_total", "clusters_with_logs", "clusters_no_logs",
            "clusters_errored", "clusters_skipped", "patterns_total",
            "severity_counts", "findings", "usage", "timing",
            "grafana_url", "qovery_cluster_names",
        }
        self.assertEqual(set(data.keys()), expected_keys)

    def test_json_to_file(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            path = f.name

        try:
            r = JsonRenderer(file_path=path)
            r.render(SAMPLE_REPORT)
            with open(path) as f:
                data = json.loads(f.read())
            self.assertEqual(data["version"], 1)
            self.assertEqual(data["clusters_total"], 10)
        finally:
            os.unlink(path)

    def test_findings_include_resource(self):
        out = StringIO()
        r = JsonRenderer(stream=out)
        r.render(SAMPLE_REPORT)
        data = json.loads(out.getvalue())
        self.assertEqual(data["findings"][0]["resource"], "aws_route_table.rt")

    def test_findings_exclude_sample_diff(self):
        report = dict(SAMPLE_REPORT)
        report["findings"] = [{
            "severity": "review",
            "title": "test",
            "impact": "test",
            "source": "terraform",
            "category": "other",
            "action": "test",
            "affected_clusters": ["c1"],
            "grafana_url": "",
            "sample_diff": "should not appear",
        }]
        out = StringIO()
        r = JsonRenderer(stream=out)
        r.render(report)
        data = json.loads(out.getvalue())
        self.assertNotIn("sample_diff", data["findings"][0])


class TestCreateRenderer(unittest.TestCase):
    def test_default_mode(self):
        r, p, stdout_r = create_renderer(verbose=False, json_output=None)
        self.assertIsInstance(r, DefaultRenderer)
        self.assertIsInstance(p, ProgressReporter)
        self.assertIsNone(stdout_r)

    def test_verbose_mode(self):
        r, p, _ = create_renderer(verbose=True, json_output=None)
        self.assertIsInstance(r, VerboseRenderer)
        self.assertTrue(p._verbose)

    def test_json_stdout_mode(self):
        r, p, stdout_r = create_renderer(verbose=False, json_output=True)
        self.assertIsInstance(r, JsonRenderer)
        self.assertIsNone(stdout_r)

    def test_json_file_mode(self):
        r, p, stdout_r = create_renderer(verbose=False, json_output="/tmp/test.json")
        self.assertIsInstance(r, JsonRenderer)
        self.assertEqual(r._file_path, "/tmp/test.json")
        self.assertIsInstance(stdout_r, DefaultRenderer)

    def test_json_file_returns_verbose_renderer_for_stdout(self):
        r, _, stdout_r = create_renderer(verbose=True, json_output="/tmp/test.json")
        self.assertIsInstance(stdout_r, VerboseRenderer)

    def test_verbose_json_gives_json_renderer(self):
        r, p, _ = create_renderer(verbose=True, json_output=True)
        self.assertIsInstance(r, JsonRenderer)
        self.assertTrue(p._verbose)


class TestClusterLabeling(unittest.TestCase):
    def test_internal_cluster_labeled_customer_bare(self):
        out = StringIO()
        r = DefaultRenderer(stream=out, use_color=False)
        r._qovery_cluster_names = {"int-cluster": "Qovery test AWS"}
        r._render_cluster_list(["int-cluster", "cust-cluster"])
        s = out.getvalue()
        self.assertIn("int-cluster — Qovery test AWS", s)
        self.assertIn("cust-cluster", s)
        self.assertNotIn("cust-cluster —", s)  # customer never labeled

    def test_no_map_leaves_all_bare(self):
        out = StringIO()
        r = DefaultRenderer(stream=out, use_color=False)  # render() not called → no map
        r._render_cluster_list(["c1"])
        s = out.getvalue()
        self.assertIn("Clusters: c1", s)
        self.assertNotIn("—", s)


if __name__ == "__main__":
    unittest.main()
