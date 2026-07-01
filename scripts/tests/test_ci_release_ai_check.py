import contextlib
import sys
import os
import tempfile
import unittest
import json
import base64
from io import StringIO
from unittest.mock import patch, MagicMock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from ci_release_ai_check import (
    parse_cluster_ids, query_loki, analyze_with_claude, retry,
    LOKI_LIMIT, MAX_BATCH_CHARS, MAX_BATCH_LIMIT, CLAUDE_MODEL,
    CLAUDE_VERDICT_MODEL, PRICING_PER_MTK, _merge_usage, _call_claude,
    _apply_severity_overrides, _reconcile_verdict_severity,
    parse_qovery_cluster_names, QOVERY_ORGS,
    _grafana_url, normalize_diff, fingerprint_and_group,
)


class TestParseClusterIds(unittest.TestCase):
    HEADER = "OrganizationId                       | OrganizationName | OrganizationPlan | ClusterId                            | ClusterName"

    def test_finds_single_uuid(self):
        text = (
            self.HEADER + "\n"
            "b34c8ec8-665c-4214-8aee-99c31d3144ce | Hologic          | USER_2025        | b79ca196-0c10-41ed-b717-d7da84625c4b | hologic\n"
        )
        self.assertEqual(
            parse_cluster_ids(text),
            ["b79ca196-0c10-41ed-b717-d7da84625c4b"],
        )

    def test_does_not_include_organization_ids(self):
        text = (
            self.HEADER + "\n"
            "b34c8ec8-665c-4214-8aee-99c31d3144ce | Hologic          | USER_2025        | b79ca196-0c10-41ed-b717-d7da84625c4b | hologic\n"
        )
        result = parse_cluster_ids(text)
        self.assertNotIn("b34c8ec8-665c-4214-8aee-99c31d3144ce", result)
        self.assertIn("b79ca196-0c10-41ed-b717-d7da84625c4b", result)

    def test_deduplicates_cluster_ids(self):
        text = (
            self.HEADER + "\n"
            "b34c8ec8-665c-4214-8aee-99c31d3144ce | OrgA | USER_2025 | b79ca196-0c10-41ed-b717-d7da84625c4b | cluster1\n"
            "b34c8ec8-665c-4214-8aee-99c31d3144ce | OrgA | USER_2025 | b79ca196-0c10-41ed-b717-d7da84625c4b | cluster1\n"
            "cccccccc-cccc-cccc-cccc-cccccccccccc | OrgB | TEAM      | aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa | cluster2\n"
        )
        result = parse_cluster_ids(text)
        self.assertEqual(len(result), 2)
        self.assertIn("b79ca196-0c10-41ed-b717-d7da84625c4b", result)
        self.assertIn("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", result)

    def test_empty_input_returns_empty_list(self):
        self.assertEqual(parse_cluster_ids(""), [])

    def test_no_table_returns_empty_list(self):
        self.assertEqual(parse_cluster_ids("no table here just text"), [])

    def test_preserves_insertion_order(self):
        text = (
            self.HEADER + "\n"
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa | OrgA | TEAM | 11111111-1111-1111-1111-111111111111 | first\n"
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb | OrgB | TEAM | 22222222-2222-2222-2222-222222222222 | second\n"
        )
        result = parse_cluster_ids(text)
        self.assertEqual(result[0], "11111111-1111-1111-1111-111111111111")
        self.assertEqual(result[1], "22222222-2222-2222-2222-222222222222")


class TestNormalizeDiff(unittest.TestCase):
    def test_replaces_uuids(self):
        text = 'cluster_id: "b79ca196-0c10-41ed-b717-d7da84625c4b"'
        result = normalize_diff(text)
        self.assertNotIn("b79ca196", result)
        self.assertIn("<UUID>", result)

    def test_replaces_aws_resource_ids(self):
        text = "igw-0dac8a25790e05bd9 nat-0aa84003e4e4f6937 pcx-02451bf7f2ebdf4cc"
        result = normalize_diff(text)
        self.assertNotIn("igw-", result)
        self.assertNotIn("nat-", result)
        self.assertNotIn("pcx-", result)
        self.assertEqual(result.count("<AWS_ID>"), 3)

    def test_replaces_transit_gateway_ids(self):
        text = "tgw-08d600f745695765f subnet-0f3abc123def sg-0ab12cd34ef"
        result = normalize_diff(text)
        self.assertEqual(result.count("<AWS_ID>"), 3)

    def test_replaces_arns(self):
        text = 'kms_master_key_id = "arn:aws:kms:eu-west-3:123456789012:key/abcdef01-2345-6789-abcd-ef0123456789"'
        result = normalize_diff(text)
        self.assertNotIn("arn:aws", result)
        self.assertIn("<ARN>", result)

    def test_replaces_hex_hashes(self):
        text = "image: 3fddccc4462b18140c8e4ae3b896a62027f86f39"
        result = normalize_diff(text)
        self.assertNotIn("3fddccc", result)
        self.assertIn("<HASH>", result)

    def test_replaces_cidrs(self):
        text = "cidr_block = 10.18.0.0/16"
        result = normalize_diff(text)
        self.assertNotIn("10.18.0.0/16", result)
        self.assertIn("<CIDR>", result)

    def test_replaces_ips(self):
        text = "address = 192.168.1.50"
        result = normalize_diff(text)
        self.assertNotIn("192.168.1.50", result)
        self.assertIn("<IP>", result)

    def test_identical_diffs_produce_same_normalized_output(self):
        diff_a = (
            'cluster_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"\n'
            "igw-0000000000000000a route removed\n"
            "image: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )
        diff_b = (
            'cluster_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"\n'
            "igw-0000000000000000b route removed\n"
            "image: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        )
        self.assertEqual(normalize_diff(diff_a), normalize_diff(diff_b))

    def test_structurally_different_diffs_produce_different_output(self):
        diff_a = "igw-0000000000000000a route removed"
        diff_b = "nat-0000000000000000b gateway added\nigw-111111111111111ab route removed"
        self.assertNotEqual(normalize_diff(diff_a), normalize_diff(diff_b))

    def test_preserves_non_sensitive_text(self):
        text = "resource aws_route_table eks_cluster will be updated in-place"
        self.assertEqual(normalize_diff(text), text)

    def test_replaces_qovery_cluster_short_ids(self):
        text = "aws_eks_cluster.eks_cluster: Refreshing state... [id=qovery-za0829ee2]"
        result = normalize_diff(text)
        self.assertNotIn("za0829ee2", result)

    def test_replaces_aws_account_ids(self):
        text = "data.aws_caller_identity.current: id=744881169338"
        result = normalize_diff(text)
        self.assertNotIn("744881169338", result)
        self.assertIn("<ACCOUNT_ID>", result)

    def test_replaces_aws_timestamp_ids(self):
        text = "id=qovery-cw-event-HealthEvent-20241107092757786300000002"
        result = normalize_diff(text)
        self.assertNotIn("20241107092757786300000002", result)
        self.assertIn("<AWS_TS_ID>", result)

    def test_replaces_sgrule_ids(self):
        text = "aws_security_group_rule.node_ingress_self: [id=sgrule-3188741747]"
        result = normalize_diff(text)
        self.assertNotIn("sgrule-3188741747", result)
        self.assertIn("<SGRULE>", result)

    def test_replaces_aws_regions(self):
        text = "https://sqs.eu-west-3.amazonaws.com/queue"
        result = normalize_diff(text)
        self.assertNotIn("eu-west-3", result)
        self.assertIn("<REGION>", result)

    def test_replaces_iso_timestamps(self):
        text = "time_static.on_cluster_create: [id=2022-03-30T19:58:12Z]"
        result = normalize_diff(text)
        self.assertNotIn("2022-03-30T19:58:12Z", result)
        self.assertIn("<TIMESTAMP>", result)

    def test_strips_terraform_refreshing_state_lines(self):
        text = (
            "aws_eks_cluster.eks: Refreshing state... [id=qovery-z123]\n"
            "  + resource will be created\n"
            "data.aws_caller_identity.current: Read complete after 0s [id=123]\n"
            "  - resource will be destroyed"
        )
        result = normalize_diff(text)
        self.assertNotIn("Refreshing state", result)
        self.assertNotIn("Read complete after", result)
        self.assertIn("resource will be created", result)
        self.assertIn("resource will be destroyed", result)

    def test_collapses_consecutive_blank_lines(self):
        text = "line1\n\n\n\nline2\n\n\nline3"
        result = normalize_diff(text)
        self.assertNotIn("\n\n\n", result)
        self.assertIn("line1\n\nline2\n\nline3", result)

    def test_clusters_with_same_structure_different_ids_group_together(self):
        diff_a = (
            'qovery-za0829ee2 s3 bucket update\n'
            'image: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n'
            'arn:aws:kms:eu-west-3:111111111111:key/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
        )
        diff_b = (
            'qovery-z0686f751 s3 bucket update\n'
            'image: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n'
            'arn:aws:kms:eu-west-1:222222222222:key/bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'
        )
        self.assertEqual(normalize_diff(diff_a), normalize_diff(diff_b))


class TestFingerprintAndGroup(unittest.TestCase):
    def test_identical_diffs_grouped_together(self):
        cluster_logs = {
            "cluster-a": 'image: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa route igw-00000000000000001',
            "cluster-b": 'image: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb route igw-00000000000000002',
        }
        groups = fingerprint_and_group(cluster_logs)
        self.assertEqual(len(groups), 1)
        self.assertEqual(len(groups[0]["cluster_ids"]), 2)

    def test_different_diffs_in_separate_groups(self):
        cluster_logs = {
            "cluster-a": "image update only",
            "cluster-b": "route table destroyed\nnat gateway removed",
        }
        groups = fingerprint_and_group(cluster_logs)
        self.assertEqual(len(groups), 2)

    def test_sorted_by_size_descending(self):
        cluster_logs = {
            "cluster-a": "common change",
            "cluster-b": "common change",
            "cluster-c": "common change",
            "cluster-d": "rare change",
        }
        groups = fingerprint_and_group(cluster_logs)
        self.assertEqual(len(groups), 2)
        self.assertEqual(len(groups[0]["cluster_ids"]), 3)
        self.assertEqual(len(groups[1]["cluster_ids"]), 1)

    def test_is_common_true_when_over_50_percent(self):
        cluster_logs = {
            "cluster-a": "common",
            "cluster-b": "common",
            "cluster-c": "common",
            "cluster-d": "rare",
        }
        groups = fingerprint_and_group(cluster_logs)
        self.assertTrue(groups[0]["is_common"])
        self.assertFalse(groups[1]["is_common"])

    def test_is_common_false_when_exactly_50_percent(self):
        cluster_logs = {
            "cluster-a": "pattern-one",
            "cluster-b": "pattern-one",
            "cluster-c": "pattern-two",
            "cluster-d": "pattern-two",
        }
        groups = fingerprint_and_group(cluster_logs)
        self.assertFalse(groups[0]["is_common"])
        self.assertFalse(groups[1]["is_common"])

    def test_representative_logs_are_original_not_normalized(self):
        original = 'cluster_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa" image: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
        cluster_logs = {"cluster-a": original}
        groups = fingerprint_and_group(cluster_logs)
        self.assertEqual(groups[0]["representative_logs"], original)

    def test_cluster_ids_sorted_within_group(self):
        cluster_logs = {
            "cluster-z": "same",
            "cluster-a": "same",
            "cluster-m": "same",
        }
        groups = fingerprint_and_group(cluster_logs)
        self.assertEqual(groups[0]["cluster_ids"], ["cluster-a", "cluster-m", "cluster-z"])

    def test_single_cluster(self):
        cluster_logs = {"only-one": "some diff"}
        groups = fingerprint_and_group(cluster_logs)
        self.assertEqual(len(groups), 1)
        self.assertTrue(groups[0]["is_common"])
        self.assertEqual(groups[0]["cluster_ids"], ["only-one"])


class TestQueryLoki(unittest.TestCase):
    def _make_loki_response(self, lines):
        """Helper: build a Loki query_range JSON response."""
        values = [["1234567890000000000", line] for line in lines]
        body = json.dumps({
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{"stream": {"container": "qovery-engine"}, "values": values}],
            },
        }).encode()
        mock_resp = MagicMock()
        mock_resp.read.return_value = body
        mock_resp.__enter__ = lambda s: s
        mock_resp.__exit__ = MagicMock(return_value=False)
        return mock_resp

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_returns_concatenated_log_lines(self, mock_urlopen):
        mock_urlopen.return_value = self._make_loki_response(
            ["line one", "line two", "line three"]
        )
        logs, _ = query_loki(
            "c79e5e94-5187-4c0a-94fa-c6a42c68ce2b", "user", "pass", 1000, 2000
        )
        self.assertEqual(logs, "line one\nline two\nline three")

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_url_contains_cluster_id_filter(self, mock_urlopen):
        mock_urlopen.return_value = self._make_loki_response([])
        query_loki("c79e5e94-5187-4c0a-94fa-c6a42c68ce2b", "user", "pass", 1000, 2000)
        call_args = mock_urlopen.call_args
        request = call_args[0][0]
        self.assertIn("cluster_id", request.full_url)
        self.assertIn("c79e5e94-5187-4c0a-94fa-c6a42c68ce2b", request.full_url)
        self.assertIn("infra-diff-terraform", request.full_url)
        self.assertIn("infra-diff-helm", request.full_url)
        self.assertIn("message", request.full_url)

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_terraform_only_filter_excludes_helm(self, mock_urlopen):
        mock_urlopen.return_value = self._make_loki_response([])
        query_loki("c79e5e94-5187-4c0a-94fa-c6a42c68ce2b", "user", "pass", 1000, 2000,
                   analyze_terraform=True, analyze_helm=False)
        url = mock_urlopen.call_args[0][0].full_url
        self.assertIn("infra-diff-terraform", url)
        self.assertNotIn("infra-diff-helm", url)

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_helm_only_filter_excludes_terraform(self, mock_urlopen):
        mock_urlopen.return_value = self._make_loki_response([])
        query_loki("c79e5e94-5187-4c0a-94fa-c6a42c68ce2b", "user", "pass", 1000, 2000,
                   analyze_terraform=False, analyze_helm=True)
        url = mock_urlopen.call_args[0][0].full_url
        self.assertIn("infra-diff-helm", url)
        self.assertNotIn("infra-diff-terraform", url)

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_sets_basic_auth_header(self, mock_urlopen):
        mock_urlopen.return_value = self._make_loki_response([])
        query_loki("c79e5e94-5187-4c0a-94fa-c6a42c68ce2b", "myuser", "mypass", 1000, 2000)
        request = mock_urlopen.call_args[0][0]
        expected = "Basic " + base64.b64encode(b"myuser:mypass").decode()
        self.assertEqual(request.get_header("Authorization"), expected)

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_empty_result_returns_empty_string(self, mock_urlopen):
        mock_urlopen.return_value = self._make_loki_response([])
        logs, _ = query_loki("c79e5e94-5187-4c0a-94fa-c6a42c68ce2b", "u", "p", 1000, 2000)
        self.assertEqual(logs, "")

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_returns_truncated_flag_when_at_limit(self, mock_urlopen):
        # First page: exactly LOKI_LIMIT lines (triggers pagination)
        values = [[str(i), f"log line {i}"] for i in range(LOKI_LIMIT)]
        full_page = MagicMock()
        full_page.read.return_value = json.dumps({
            "data": {"result": [{"values": values}]}
        }).encode()
        full_page.__enter__ = lambda s: s
        full_page.__exit__ = MagicMock(return_value=False)

        # Second page: empty (signals end of data)
        empty_page = MagicMock()
        empty_page.read.return_value = json.dumps({"data": {"result": []}}).encode()
        empty_page.__enter__ = lambda s: s
        empty_page.__exit__ = MagicMock(return_value=False)

        mock_urlopen.side_effect = [full_page, empty_page]

        logs, truncated = query_loki("cluster-1", "user", "pass", 0, 1)
        self.assertTrue(truncated)
        self.assertEqual(len(logs.splitlines()), LOKI_LIMIT)

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_returns_not_truncated_when_under_limit(self, mock_urlopen):
        values = [[str(i), f"log line {i}"] for i in range(10)]
        fake_response = MagicMock()
        fake_response.read.return_value = json.dumps({
            "data": {"result": [{"values": values}]}
        }).encode()
        fake_response.__enter__ = lambda s: s
        fake_response.__exit__ = MagicMock(return_value=False)
        mock_urlopen.return_value = fake_response

        logs, truncated = query_loki("cluster-1", "user", "pass", 0, 1)
        self.assertFalse(truncated)
        self.assertEqual(len(logs.splitlines()), 10)


class TestAnalyzeWithClaude(unittest.TestCase):
    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_prompt_requests_json_and_sends_full_logs(self, mock_urlopen):
        fake_response = MagicMock()
        fake_response.read.return_value = json.dumps({
            "content": [{"text": '{"severity":"info","findings":[]}'}]
        }).encode()
        fake_response.__enter__ = lambda s: s
        fake_response.__exit__ = MagicMock(return_value=False)
        mock_urlopen.return_value = fake_response

        long_logs = "x" * 20000  # longer than old 8000 limit
        result = analyze_with_claude(long_logs, "fake-key")

        # Verify the full log was sent (no truncation)
        call_args = mock_urlopen.call_args
        sent_body = json.loads(call_args[0][0].data)
        prompt_text = sent_body["messages"][0]["content"]
        self.assertIn("x" * 20000, prompt_text)
        self.assertNotIn("TRUNCATED", prompt_text)

        # Verify JSON instruction in prompt
        self.assertIn("JSON", prompt_text)

        # Verify response is parsed as dict
        self.assertEqual(result["severity"], "info")

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_uses_haiku_model(self, mock_urlopen):
        fake_response = MagicMock()
        fake_response.read.return_value = json.dumps({
            "content": [{"text": '{"severity":"info","findings":[]}'}]
        }).encode()
        fake_response.__enter__ = lambda s: s
        fake_response.__exit__ = MagicMock(return_value=False)
        mock_urlopen.return_value = fake_response
        analyze_with_claude("logs", "key")
        request = mock_urlopen.call_args[0][0]
        body = json.loads(request.data)
        self.assertEqual(body["model"], CLAUDE_MODEL)

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_sets_anthropic_headers(self, mock_urlopen):
        fake_response = MagicMock()
        fake_response.read.return_value = json.dumps({
            "content": [{"text": '{"severity":"info","findings":[]}'}]
        }).encode()
        fake_response.__enter__ = lambda s: s
        fake_response.__exit__ = MagicMock(return_value=False)
        mock_urlopen.return_value = fake_response
        analyze_with_claude("logs", "my-api-key")
        request = mock_urlopen.call_args[0][0]
        self.assertEqual(request.get_header("X-api-key"), "my-api-key")
        self.assertIsNotNone(request.get_header("Anthropic-version"))

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_strips_markdown_fences(self, mock_urlopen):
        fake_response = MagicMock()
        fake_response.read.return_value = json.dumps({
            "content": [{"text": '```json\n{"severity":"review","findings":[]}\n```'}]
        }).encode()
        fake_response.__enter__ = lambda s: s
        fake_response.__exit__ = MagicMock(return_value=False)
        mock_urlopen.return_value = fake_response
        result = analyze_with_claude("logs", "key")
        self.assertEqual(result["severity"], "review")

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_includes_cluster_count_in_prompt(self, mock_urlopen):
        fake_response = MagicMock()
        fake_response.read.return_value = json.dumps({
            "content": [{"text": '{"severity":"info","findings":[]}'}]
        }).encode()
        fake_response.__enter__ = lambda s: s
        fake_response.__exit__ = MagicMock(return_value=False)
        mock_urlopen.return_value = fake_response
        analyze_with_claude("logs", "key", cluster_count=74)
        request = mock_urlopen.call_args[0][0]
        body = json.loads(request.data)
        self.assertIn("74 cluster(s)", body["messages"][0]["content"])

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_batches_large_logs_into_multiple_calls(self, mock_urlopen):
        # 2 * MAX_BATCH_CHARS + 1 chars → 3 batch calls + 1 synthesis = 4 total
        def make_mock(body):
            m = MagicMock()
            m.read.return_value = body
            m.__enter__ = lambda s: s
            m.__exit__ = MagicMock(return_value=False)
            return m

        batch_body = json.dumps({
            "content": [{"text": '{"severity":"review","findings":[{"severity":"review","category":"helm_values","description":"x"}]}'}]
        }).encode()
        synthesis_body = json.dumps({
            "content": [{"text": '{"severity":"review","findings":[{"severity":"review","category":"helm_values","description":"consolidated"}]}'}]
        }).encode()

        mock_urlopen.side_effect = [
            make_mock(batch_body),
            make_mock(batch_body),
            make_mock(batch_body),
            make_mock(synthesis_body),
        ]

        result = analyze_with_claude("x" * (MAX_BATCH_CHARS * 2 + 1), "key")

        self.assertEqual(mock_urlopen.call_count, 4)
        self.assertEqual(result["findings"][0]["description"], "consolidated")

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_synthesis_prompt_includes_all_batch_findings(self, mock_urlopen):
        def make_mock(body):
            m = MagicMock()
            m.read.return_value = body
            m.__enter__ = lambda s: s
            m.__exit__ = MagicMock(return_value=False)
            return m

        def batch_body(desc):
            return json.dumps({
                "content": [{"text": f'{{"severity":"review","findings":[{{"severity":"review","category":"other","description":"{desc}"}}]}}'}]
            }).encode()

        synthesis_body = json.dumps({
            "content": [{"text": '{"severity":"review","findings":[]}'}]
        }).encode()

        mock_urlopen.side_effect = [
            make_mock(batch_body("finding-a")),
            make_mock(batch_body("finding-b")),
            make_mock(synthesis_body),
        ]

        analyze_with_claude("x" * (MAX_BATCH_CHARS + 1), "key")

        last_request = mock_urlopen.call_args_list[-1][0][0]
        prompt = json.loads(last_request.data)["messages"][0]["content"]
        self.assertIn("finding-a", prompt)
        self.assertIn("finding-b", prompt)


from ci_release_ai_check import main, GRAFANA_BASE_URL


class TestGrafanaUrl(unittest.TestCase):

    def test_contains_cluster_id(self):
        url = _grafana_url("abc-123", 1_000_000_000_000_000_000, 2_000_000_000_000_000_000)
        self.assertIn("abc-123", url)
        self.assertIn(GRAFANA_BASE_URL, url)

    def test_contains_from_and_to(self):
        url = _grafana_url("c1", 1_000_000_000_000_000_000, 2_000_000_000_000_000_000)
        self.assertIn("from=", url)
        self.assertIn("to=", url)
        self.assertIn("timezone=utc", url)

    def test_iso_format(self):
        # 1_000_000_000 seconds = 2001-09-09T01:46:40.000Z
        url = _grafana_url("c1", 1_000_000_000 * 10**9, 2_000_000_000 * 10**9)
        self.assertIn("2001-09-09T01:46:40.000Z", url)


class TestMain(unittest.TestCase):
    @patch("ci_release_ai_check.generate_summary")
    @patch("ci_release_ai_check.analyze_with_claude")
    @patch("ci_release_ai_check.query_loki")
    def test_groups_identical_clusters_and_analyzes_once(
        self, mock_loki, mock_claude, mock_summary
    ):
        mock_loki.return_value = ("same diff logs for all", False)
        mock_claude.return_value = {"severity": "info", "findings": []}
        mock_summary.return_value = {"verdict": "All clear", "verdict_severity": "info", "actions": []}

        dry_run = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        dry_run.write(
            "| ClusterId | Name |\n"
            "| 00000000-0000-0000-0000-000000000001 | c1 |\n"
            "| 00000000-0000-0000-0000-000000000002 | c2 |\n"
            "| 00000000-0000-0000-0000-000000000003 | c3 |\n"
        )
        dry_run.close()

        timestamps = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        timestamps.write("start_ns=1000\nend_ns=2000\n")
        timestamps.close()

        with patch.dict(os.environ, {
            "LOKI_USERNAME": "u",
            "LOKI_PASSWORD": "p",
            "ANTHROPIC_API_KEY": "k",
        }):
            with contextlib.redirect_stdout(StringIO()):
                main(dry_run.name, timestamps.name)

        # 3 clusters with identical logs → 1 Claude analysis call
        mock_claude.assert_called_once()

        os.unlink(dry_run.name)
        os.unlink(timestamps.name)

    @patch("ci_release_ai_check.generate_summary")
    @patch("ci_release_ai_check.analyze_with_claude")
    @patch("ci_release_ai_check.query_loki")
    def test_different_diffs_analyzed_separately(
        self, mock_loki, mock_claude, mock_summary
    ):
        mock_loki.side_effect = [
            ("common diff", False),
            ("common diff", False),
            ("different diff with extra route deletion", False),
        ]
        mock_claude.return_value = {"severity": "info", "findings": []}
        mock_summary.return_value = {"verdict": "All clear", "verdict_severity": "info", "actions": []}

        dry_run = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        dry_run.write(
            "| ClusterId | Name |\n"
            "| 00000000-0000-0000-0000-000000000001 | c1 |\n"
            "| 00000000-0000-0000-0000-000000000002 | c2 |\n"
            "| 00000000-0000-0000-0000-000000000003 | c3 |\n"
        )
        dry_run.close()

        timestamps = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        timestamps.write("start_ns=1000\nend_ns=2000\n")
        timestamps.close()

        with patch.dict(os.environ, {
            "LOKI_USERNAME": "u",
            "LOKI_PASSWORD": "p",
            "ANTHROPIC_API_KEY": "k",
        }):
            with contextlib.redirect_stdout(StringIO()):
                main(dry_run.name, timestamps.name)

        # 2 unique patterns → 2 Claude analysis calls
        self.assertEqual(mock_claude.call_count, 2)

        os.unlink(dry_run.name)
        os.unlink(timestamps.name)

    @patch("ci_release_ai_check.generate_summary")
    @patch("ci_release_ai_check.query_loki")
    def test_all_clusters_errored_prints_summary(self, mock_loki, mock_summary):
        mock_loki.side_effect = ConnectionError("Loki down")
        mock_summary.return_value = {"verdict": "error", "verdict_severity": "unknown", "actions": []}

        dry_run = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        dry_run.write("| ClusterId |\n| 00000000-0000-0000-0000-000000000001 |\n")
        dry_run.close()

        timestamps = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        timestamps.write("start_ns=1000\nend_ns=2000\n")
        timestamps.close()

        captured = StringIO()
        sys.stdout = captured
        try:
            with patch.dict(os.environ, {
                "LOKI_USERNAME": "u",
                "LOKI_PASSWORD": "p",
                "ANTHROPIC_API_KEY": "k",
            }):
                main(dry_run.name, timestamps.name)
        finally:
            sys.stdout = sys.__stdout__

        output = captured.getvalue()
        self.assertIn("VERDICT", output)
        self.assertIn("UNKNOWN", output)

        os.unlink(dry_run.name)
        os.unlink(timestamps.name)

    @patch("ci_release_ai_check.generate_summary")
    @patch("ci_release_ai_check.analyze_with_claude")
    @patch("ci_release_ai_check.query_loki")
    def test_skips_cluster_when_too_many_batches(self, mock_loki, mock_claude, mock_summary):
        mock_summary.return_value = {"verdict": "N/A", "verdict_severity": "unknown", "actions": []}
        large_logs = "x" * (MAX_BATCH_CHARS * (MAX_BATCH_LIMIT + 1) + 1)
        mock_loki.return_value = (large_logs, False)

        dry_run = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        dry_run.write("| ClusterId |\n| 00000000-0000-0000-0000-000000000001 |\n")
        dry_run.close()

        timestamps = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        timestamps.write("start_ns=1000000000000000000\nend_ns=2000000000000000000\n")
        timestamps.close()

        captured = StringIO()
        sys.stdout = captured
        try:
            with patch.dict(os.environ, {
                "LOKI_USERNAME": "u",
                "LOKI_PASSWORD": "p",
                "ANTHROPIC_API_KEY": "k",
            }):
                main(dry_run.name, timestamps.name)
        finally:
            sys.stdout = sys.__stdout__

        output = captured.getvalue()
        mock_claude.assert_not_called()
        self.assertIn("UNKNOWN", output)
        self.assertIn("too large", output.lower())

        os.unlink(dry_run.name)
        os.unlink(timestamps.name)

    def test_no_cluster_ids_exits_early(self):
        dry_run = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        dry_run.write("no table here\n")
        dry_run.close()

        timestamps = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        timestamps.write("start_ns=1000\nend_ns=2000\n")
        timestamps.close()

        captured_err = StringIO()
        sys.stderr = captured_err
        try:
            with patch.dict(os.environ, {
                "LOKI_USERNAME": "u",
                "LOKI_PASSWORD": "p",
                "ANTHROPIC_API_KEY": "k",
            }):
                main(dry_run.name, timestamps.name)
        finally:
            sys.stderr = sys.__stderr__

        output = captured_err.getvalue()
        self.assertIn("WARNING", output)

        os.unlink(dry_run.name)
        os.unlink(timestamps.name)


class TestRetry(unittest.TestCase):
    def test_succeeds_first_try(self):
        call_count = 0

        def succeed():
            nonlocal call_count
            call_count += 1
            return "ok"

        result = retry(succeed, max_attempts=3, delay=0)
        self.assertEqual(result, "ok")
        self.assertEqual(call_count, 1)

    def test_succeeds_after_failures(self):
        call_count = 0

        def fail_twice():
            nonlocal call_count
            call_count += 1
            if call_count < 3:
                raise ConnectionError("fail")
            return "recovered"

        result = retry(fail_twice, max_attempts=3, delay=0)
        self.assertEqual(result, "recovered")
        self.assertEqual(call_count, 3)

    def test_raises_after_all_attempts_exhausted(self):
        def always_fail():
            raise ConnectionError("permanent")

        with self.assertRaises(ConnectionError):
            retry(always_fail, max_attempts=3, delay=0)


def _claude_resp(text, usage=None):
    """Build a mocked Anthropic Messages API response."""
    body = {"content": [{"text": text}]}
    if usage is not None:
        body["usage"] = usage
    m = MagicMock()
    m.read.return_value = json.dumps(body).encode()
    m.__enter__ = lambda s: s
    m.__exit__ = MagicMock(return_value=False)
    return m


@contextlib.contextmanager
def _run_main(dry_run_text):
    """Run main() over a dry-run table, yielding the parsed JSON report.

    Handles tempfile setup, env patching, stdout suppression, and cleanup so the
    end-to-end tests only state their mocks and assertions.
    """
    paths = []
    try:
        dry_run = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        dry_run.write(dry_run_text)
        dry_run.close()
        timestamps = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False)
        timestamps.write("start_ns=1000\nend_ns=2000\n")
        timestamps.close()
        out = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False)
        out.close()
        paths = [dry_run.name, timestamps.name, out.name]
        env = {"LOKI_USERNAME": "u", "LOKI_PASSWORD": "p", "ANTHROPIC_API_KEY": "k"}
        with patch.dict(os.environ, env), contextlib.redirect_stdout(StringIO()):
            main(dry_run.name, timestamps.name, json_output=out.name)
        with open(out.name) as f:
            yield json.load(f)
    finally:
        for p in paths:
            os.unlink(p)


_ONE_CLUSTER_TABLE = "| ClusterId |\n| 00000000-0000-0000-0000-000000000001 |\n"
_TWO_CLUSTER_TABLE = (
    "| ClusterId |\n"
    "| 00000000-0000-0000-0000-000000000001 |\n"
    "| 00000000-0000-0000-0000-000000000002 |\n"
)


class TestUpstreamMerge(unittest.TestCase):
    @patch("ci_release_ai_check.generate_summary")
    @patch("ci_release_ai_check.analyze_with_claude")
    @patch("ci_release_ai_check.query_loki")
    def test_same_issue_across_fingerprints_merged_before_verdict(self, mock_loki, mock_claude, mock_summary):
        c1 = "00000000-0000-0000-0000-000000000001"
        c2 = "00000000-0000-0000-0000-000000000002"
        # Distinct diffs -> two fingerprint groups -> analyze runs twice.
        logs_by_cluster = {c1: "route change alpha", c2: "route change beta"}
        mock_loki.side_effect = lambda cluster_id, *a, **k: (logs_by_cluster[cluster_id], False)

        # Both groups produce the SAME normalized-title finding -> must merge to one.
        def fake_analyze(logs, api_key, usage=None, cluster_count=1):
            return {"severity": "review", "findings": [{
                "severity": "review", "title": "New S3 bucket policies enforcing HTTPS-only access",
                "source": "terraform", "resource": "aws_s3_bucket.logs",
                "category": "iam_change", "impact": "i", "action": "a",
            }]}
        mock_claude.side_effect = fake_analyze

        captured = {}
        def fake_summary(findings, non_analyzed, api_key, usage=None):
            captured["n_findings"] = len(findings)
            return {"verdict": "ok", "verdict_severity": "review",
                    "severity_overrides": {"0": "review"},
                    "actions": [{"finding_id": 0, "text": "[REVIEW] [TERRAFORM] verify"}]}
        mock_summary.side_effect = fake_summary

        with _run_main(_TWO_CLUSTER_TABLE) as report:
            # Verdict pass saw ONE merged finding, not two per-fingerprint findings.
            self.assertEqual(captured["n_findings"], 1)
            self.assertEqual(len(report["findings"]), 1)
            self.assertEqual(sorted(report["findings"][0]["affected_clusters"]), sorted([c1, c2]))


class TestMergeUsage(unittest.TestCase):
    def test_merge_into_empty(self):
        dst = {}
        _merge_usage(dst, {"m1": {"input_tokens": 10, "output_tokens": 2}})
        self.assertEqual(dst, {"m1": {"input_tokens": 10, "output_tokens": 2}})

    def test_merge_same_model_accumulates(self):
        dst = {"m1": {"input_tokens": 10, "output_tokens": 2}}
        _merge_usage(dst, {"m1": {"input_tokens": 5, "output_tokens": 3}})
        self.assertEqual(dst["m1"], {"input_tokens": 15, "output_tokens": 5})

    def test_merge_distinct_models_kept_separate(self):
        dst = {}
        _merge_usage(dst, {"m1": {"input_tokens": 1, "output_tokens": 1}})
        _merge_usage(dst, {"m2": {"input_tokens": 2, "output_tokens": 2}})
        self.assertEqual(set(dst), {"m1", "m2"})
        self.assertEqual(dst["m1"], {"input_tokens": 1, "output_tokens": 1})
        self.assertEqual(dst["m2"], {"input_tokens": 2, "output_tokens": 2})


class TestCallClaudeUsage(unittest.TestCase):
    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_records_usage_under_default_model(self, mock_urlopen):
        mock_urlopen.return_value = _claude_resp('{"ok":1}', {"input_tokens": 10, "output_tokens": 5})
        usage = {}
        _call_claude("p", "key", usage)
        self.assertEqual(usage, {CLAUDE_MODEL: {"input_tokens": 10, "output_tokens": 5}})

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_records_usage_under_explicit_model(self, mock_urlopen):
        mock_urlopen.return_value = _claude_resp('{"ok":1}', {"input_tokens": 3, "output_tokens": 7})
        usage = {}
        _call_claude("p", "key", usage, model=CLAUDE_VERDICT_MODEL)
        self.assertEqual(usage, {CLAUDE_VERDICT_MODEL: {"input_tokens": 3, "output_tokens": 7}})

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_accumulates_same_model_across_calls(self, mock_urlopen):
        mock_urlopen.side_effect = [
            _claude_resp('{}', {"input_tokens": 10, "output_tokens": 2}),
            _claude_resp('{}', {"input_tokens": 5, "output_tokens": 3}),
        ]
        usage = {}
        _call_claude("p", "k", usage)
        _call_claude("p", "k", usage)
        self.assertEqual(usage[CLAUDE_MODEL], {"input_tokens": 15, "output_tokens": 5})

    @patch("ci_release_ai_check.urllib.request.urlopen")
    def test_none_usage_acc_is_noop(self, mock_urlopen):
        mock_urlopen.return_value = _claude_resp('{"x":1}', {"input_tokens": 1, "output_tokens": 1})
        self.assertEqual(_call_claude("p", "k", None), {"x": 1})


class TestPricingTable(unittest.TestCase):
    def test_active_models_have_pricing(self):
        # Guard against bumping a model id without adding its rate.
        self.assertIn(CLAUDE_MODEL, PRICING_PER_MTK)
        self.assertIn(CLAUDE_VERDICT_MODEL, PRICING_PER_MTK)

    def test_rates_are_positive_pairs(self):
        for model, rate in PRICING_PER_MTK.items():
            self.assertEqual(len(rate), 2, f"{model} must have (input, output) rates")
            self.assertGreater(rate[0], 0)
            self.assertGreater(rate[1], 0)


class TestCostReporting(unittest.TestCase):
    @patch("ci_release_ai_check.generate_summary")
    @patch("ci_release_ai_check.analyze_with_claude")
    @patch("ci_release_ai_check.query_loki")
    def test_per_model_cost_in_json_output(self, mock_loki, mock_claude, mock_summary):
        mock_loki.return_value = ("some diff", False)

        # Analysis runs on CLAUDE_MODEL (Sonnet); populate the per-call usage dict.
        def fake_analyze(logs, api_key, usage=None, cluster_count=1):
            if usage is not None:
                _merge_usage(usage, {CLAUDE_MODEL: {"input_tokens": 1_000_000, "output_tokens": 200_000}})
            return {"severity": "info", "findings": []}
        mock_claude.side_effect = fake_analyze

        # Verdict runs on CLAUDE_VERDICT_MODEL (Opus); populate the shared usage_acc.
        def fake_summary(findings, non_analyzed, api_key, usage=None):
            if usage is not None:
                _merge_usage(usage, {CLAUDE_VERDICT_MODEL: {"input_tokens": 10_000, "output_tokens": 2_000}})
            return {"verdict": "ok", "verdict_severity": "info", "actions": []}
        mock_summary.side_effect = fake_summary

        with _run_main(_ONE_CLUSTER_TABLE) as report:
            usage = report["usage"]

        # Sonnet: 1M in * $3 + 0.2M out * $15 = $6.00 ; Opus: 0.01M * $5 + 0.002M * $25 = $0.10
        self.assertEqual(usage["by_model"][CLAUDE_MODEL]["estimated_cost_usd"], 6.0)
        self.assertEqual(usage["by_model"][CLAUDE_VERDICT_MODEL]["estimated_cost_usd"], 0.1)
        self.assertEqual(usage["estimated_cost_usd"], 6.1)
        self.assertEqual(usage["input_tokens"], 1_010_000)
        self.assertEqual(usage["output_tokens"], 202_000)
        self.assertEqual(usage["model_analysis"], CLAUDE_MODEL)
        self.assertEqual(usage["model_verdict"], CLAUDE_VERDICT_MODEL)


class TestApplySeverityOverrides(unittest.TestCase):
    def test_applies_override_by_string_id(self):
        findings = [{"id": 0, "severity": "review"}, {"id": 1, "severity": "info"}]
        _apply_severity_overrides(findings, {"0": "critical", "1": "review"})
        self.assertEqual(findings[0]["severity"], "critical")
        self.assertEqual(findings[1]["severity"], "review")

    def test_ignores_unknown_id(self):
        findings = [{"id": 0, "severity": "review"}]
        _apply_severity_overrides(findings, {"99": "critical"})
        self.assertEqual(findings[0]["severity"], "review")

    def test_ignores_invalid_severity(self):
        findings = [{"id": 0, "severity": "review"}]
        _apply_severity_overrides(findings, {"0": "bogus"})
        self.assertEqual(findings[0]["severity"], "review")

    def test_finding_without_override_unchanged(self):
        findings = [{"id": 0, "severity": "review"}, {"id": 1, "severity": "info"}]
        _apply_severity_overrides(findings, {"0": "critical"})
        self.assertEqual(findings[1]["severity"], "info")


class TestReconcileVerdictSeverity(unittest.TestCase):
    def test_escalates_to_worst_finding(self):
        findings = [{"severity": "info"}, {"severity": "critical"}]
        self.assertEqual(_reconcile_verdict_severity("review", findings), "critical")

    def test_does_not_downgrade(self):
        findings = [{"severity": "info"}]
        self.assertEqual(_reconcile_verdict_severity("critical", findings), "critical")

    def test_empty_findings_keeps_input(self):
        self.assertEqual(_reconcile_verdict_severity("unknown", []), "unknown")


class TestVerdictAuthoritative(unittest.TestCase):
    @patch("ci_release_ai_check.generate_summary")
    @patch("ci_release_ai_check.analyze_with_claude")
    @patch("ci_release_ai_check.query_loki")
    def test_verdict_escalation_surfaces_in_counts_and_findings(self, mock_loki, mock_claude, mock_summary):
        mock_loki.return_value = ("iam policy losing allow actions", False)
        # Analysis under-rates it as review (the rubric blind spot we diagnosed).
        mock_claude.return_value = {
            "severity": "review",
            "findings": [{
                "severity": "review", "title": "IAM policy losing Allow actions",
                "source": "terraform", "category": "iam_change",
                "impact": "Prometheus loses S3/KMS perms", "action": "review policy",
            }],
        }
        # Verdict escalates finding id 0 to critical; verdict_severity left lower to test reconcile.
        mock_summary.return_value = {
            "verdict": "Critical IAM risk on 1 cluster",
            "verdict_severity": "review",
            "severity_overrides": {"0": "critical"},
            "actions": ["[CRITICAL] [TERRAFORM] Review IAM policy on 1 cluster"],
        }

        with _run_main(_ONE_CLUSTER_TABLE) as report:
            # The escalation must flow into counts, the finding, and the verdict severity.
            self.assertEqual(report["severity_counts"]["critical"], 1)
            self.assertEqual(report["severity_counts"]["review"], 0)
            self.assertEqual(report["verdict_severity"], "critical")
            self.assertEqual(report["findings"][0]["severity"], "critical")


class TestParseQoveryClusterNames(unittest.TestCase):
    HEADER = "OrganizationId | OrganizationName | OrganizationPlan | ClusterId | ClusterName"

    def test_labels_qovery_cluster(self):
        text = (self.HEADER + "\n"
                "460616f0-94da-4d35-b631-6fa4ed08eb9a | x | y | eb1bc33b-68fb-441e-9854-b4fd369762a4 | c\n")
        self.assertEqual(parse_qovery_cluster_names(text),
                         {"eb1bc33b-68fb-441e-9854-b4fd369762a4": "Qovery test AWS"})

    def test_excludes_customer_org(self):
        # A customer org (not in QOVERY_ORGS) must never be surfaced.
        text = (self.HEADER + "\n"
                "11111111-1111-1111-1111-111111111111 | Cust | y | 22222222-2222-2222-2222-222222222222 | c\n")
        self.assertEqual(parse_qovery_cluster_names(text), {})

    def test_no_org_column_returns_empty(self):
        text = ("ClusterId | ClusterName\n"
                "eb1bc33b-68fb-441e-9854-b4fd369762a4 | c\n")
        self.assertEqual(parse_qovery_cluster_names(text), {})

    def test_empty_input(self):
        self.assertEqual(parse_qovery_cluster_names(""), {})

    def test_all_qovery_orgs_named(self):
        for oid, name in QOVERY_ORGS.items():
            self.assertRegex(oid, r"^[0-9a-f-]{36}$")
            self.assertTrue(name and isinstance(name, str))


if __name__ == "__main__":
    unittest.main()
