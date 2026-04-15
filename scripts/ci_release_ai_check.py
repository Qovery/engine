#!/usr/bin/env python3
"""CI Release AI Check — downloads Loki diff logs per cluster and runs Claude analysis."""

import base64
import hashlib
import json
import os
import re
import sys
import time
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone

from ci_release_ai_check_output import create_renderer, strip_ansi

UUID_PATTERN = re.compile(
    r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
    re.IGNORECASE,
)
LOKI_BASE_URL = "https://loki-engine-infra-static.z6d9f665a.rustrocks.space"
LOKI_LIMIT = 5000
RETRY_MAX_ATTEMPTS = 3
RETRY_DELAY_SECONDS = 2
MAX_BATCH_CHARS = 400_000
MAX_BATCH_LIMIT = 4
GRAFANA_BASE_URL = "https://qortal.qovery.com/grafana/d/ae51ecxhq2tj4a/infra-cluster-diff"
GRAFANA_TF_FILTER = "(%5C%2B%20%7C-%20%7C~%20%7C-%2F%5C%2B%20)"
CLAUDE_MODEL = "claude-sonnet-4-6"
CLAUDE_VERDICT_MODEL = "claude-sonnet-4-6"

ANALYSIS_RESPONSE_SCHEMA = (
    '{"severity": "<critical|review|info>", '
    '"findings": [{"severity": "<critical|review|info>", '
    '"title": "<short title>", '
    '"source": "<terraform|helm>", '
    '"category": "<resource_deletion|iam_change|node_pool|version_downgrade|network|helm_values|other>", '
    '"impact": "<why it matters>", '
    '"action": "<what operator should do>", '
    '"description": "<short description>"}]}'
)
ANALYSIS_SCHEMA_NOTE = (
    f"Return ONLY valid JSON (no markdown fences, no extra text) with this structure:\n"
    f"{ANALYSIS_RESPONSE_SCHEMA}\n\n"
    f'Set top-level severity to the worst finding severity, or "info" if no findings.\n'
    f'Set "source" to "terraform" or "helm" based on which diff the finding comes from.'
)

NORMALIZE_PATTERNS = [
    # ARNs (before UUIDs — ARNs contain account IDs and sometimes UUIDs)
    (re.compile(r'arn:aws[a-zA-Z-]*:[a-zA-Z0-9-]+:[a-zA-Z0-9-]*:\d{12}:[^\s,"}\]\']+'), "<ARN>"),
    # UUIDs (also catches KMS key IDs)
    (re.compile(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}', re.IGNORECASE), "<UUID>"),
    # 40-char hex hashes (git SHAs, container image tags)
    (re.compile(r'\b[0-9a-f]{40}\b'), "<HASH>"),
    # Qovery cluster short IDs (z + first 8 hex chars of UUID, e.g. za0829ee2)
    (re.compile(r'\bz[0-9a-f]{8,9}\b'), "<ZCLUSTER>"),
    # AWS resource IDs (igw-xxx, nat-xxx, pcx-xxx, sg-xxx, subnet-xxx, etc.)
    (re.compile(r'\b(?:igw|nat|pcx|sg|subnet|rtb|tgw|vpc|eni|vol|snap|ami|lt|eipalloc|rtbassoc|acl|cgw|vgw|vpce)-[0-9a-f]{7,17}\b'), "<AWS_ID>"),
    # EC2 instance IDs
    (re.compile(r'\bi-[0-9a-f]{8,17}\b'), "<AWS_ID>"),
    # Security group rule IDs
    (re.compile(r'\bsgrule-\d+\b'), "<SGRULE>"),
    # AWS account IDs (12-digit numbers)
    (re.compile(r'\b\d{12}\b'), "<ACCOUNT_ID>"),
    # AWS timestamp-based suffixes on resource names (20260324151943099200000001)
    (re.compile(r'\b\d{20,}\b'), "<AWS_TS_ID>"),
    # CIDR blocks (before bare IPs)
    (re.compile(r'\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}/\d{1,2}'), "<CIDR>"),
    # IP addresses
    (re.compile(r'\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b'), "<IP>"),
    # AWS regions (eu-west-1, us-east-2, ap-southeast-1, etc.)
    (re.compile(r'\b(?:us|eu|ap|sa|ca|me|af)-(?:east|west|north|south|central|northeast|southeast|northwest|southwest)-\d\b'), "<REGION>"),
    # ISO timestamps (2022-03-30T19:58:12Z)
    (re.compile(r'\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z\b'), "<TIMESTAMP>"),
    # Data source numeric IDs (arbitrary large numbers in [id=NNNN])
    (re.compile(r'(?<=\[id=)\d{6,}(?=\])'), "<NUMERIC_ID>"),
    # Terraform timing noise (after 0s, after 1s, etc.)
    (re.compile(r'\bafter \d+s\b'), "after <N>s"),
]

# Lines matching these patterns are terraform state-refresh noise, not actual diffs.
# Stripping them before fingerprinting dramatically improves grouping.
STRIP_LINE_PATTERNS = [
    re.compile(r'Refreshing state\.\.\. \[id='),
    re.compile(r'Read complete after'),
]

# Prefix added by the LogQL line_format — stripped for fingerprinting, kept for analysis
SOURCE_TAG_PATTERN = re.compile(r'^\[(terraform|helm)\] ', re.MULTILINE)


def normalize_diff(text: str) -> str:
    """Replace cluster-specific values with placeholders for fingerprinting."""
    lines = []
    prev_blank = False
    for line in text.splitlines():
        if any(p.search(line) for p in STRIP_LINE_PATTERNS):
            continue
        line = SOURCE_TAG_PATTERN.sub("", line)
        is_blank = not line.strip()
        if is_blank and prev_blank:
            continue
        prev_blank = is_blank
        lines.append(line)
    text = "\n".join(lines).strip()
    for pattern, replacement in NORMALIZE_PATTERNS:
        text = pattern.sub(replacement, text)
    return text


def fingerprint_and_group(cluster_logs: dict) -> list:
    """Group clusters by normalized diff fingerprint.

    Returns list of groups sorted by size (largest first), each a dict with:
        fingerprint, cluster_ids, representative_logs, is_common
    """
    groups = {}
    for cluster_id, logs in cluster_logs.items():
        fp = hashlib.sha256(normalize_diff(logs).encode()).hexdigest()
        if fp not in groups:
            groups[fp] = {"cluster_ids": [], "representative_logs": logs}
        groups[fp]["cluster_ids"].append(cluster_id)

    total = len(cluster_logs)
    result = []
    for fp, data in groups.items():
        result.append({
            "fingerprint": fp,
            "cluster_ids": sorted(data["cluster_ids"]),
            "representative_logs": data["representative_logs"],
            "is_common": len(data["cluster_ids"]) > total / 2,
        })
    result.sort(key=lambda g: len(g["cluster_ids"]), reverse=True)
    return result


def _ns_to_iso(ns: int) -> str:
    dt = datetime.fromtimestamp(ns / 1e9, tz=timezone.utc)
    return f"{dt.strftime('%Y-%m-%dT%H:%M:%S.')}{dt.microsecond // 1000:03d}Z"


def _grafana_url(cluster_id: str, start_ns: int, end_ns: int) -> str:
    """Build a Grafana infra-cluster-diff URL for manual review."""
    return (
        f"{GRAFANA_BASE_URL}?orgId=1"
        f"&from={_ns_to_iso(start_ns)}&to={_ns_to_iso(end_ns)}"
        f"&timezone=utc&var-cluster={cluster_id}&var-tffilter={GRAFANA_TF_FILTER}"
    )


def _grafana_base_url(start_ns: int, end_ns: int) -> str:
    """Build a Grafana URL with time range but no cluster filter."""
    return (
        f"{GRAFANA_BASE_URL}?orgId=1"
        f"&from={_ns_to_iso(start_ns)}&to={_ns_to_iso(end_ns)}"
        f"&timezone=utc&var-cluster=&var-tffilter={GRAFANA_TF_FILTER}"
    )


def _extract_json(text: str) -> dict:
    """Extract and parse the first complete JSON object from text.

    Handles Claude responses that include extra prose before or after the JSON,
    or that wrap the JSON in markdown code fences.
    """
    text = text.strip()
    if text.startswith("```"):
        lines = text.split("\n", 1)
        if len(lines) > 1:
            text = lines[1]
            if "```" in text:
                text = text[: text.rfind("```")]
            text = text.strip()
    start = text.find("{")
    if start == -1:
        raise ValueError(f"No JSON object found in response: {text[:200]!r}")
    obj, _ = json.JSONDecoder().raw_decode(text, start)
    return obj


def retry(fn, max_attempts=RETRY_MAX_ATTEMPTS, delay=RETRY_DELAY_SECONDS):
    """Call fn() up to max_attempts times, sleeping delay seconds between retries."""
    last_error = None
    for attempt in range(max_attempts):
        try:
            return fn()
        except Exception as e:
            last_error = e
            if attempt < max_attempts - 1:
                print(f"  Attempt {attempt + 1}/{max_attempts} failed: {e}, retrying in {delay}s...", file=sys.stderr)
                time.sleep(delay)
    raise last_error


def parse_cluster_ids(text: str) -> list:
    """Extract unique ClusterIds from the qovery admin cluster deploy table output.

    The output is a pipe-separated table with a header row containing 'ClusterId'.
    Returns an empty list if no table is found.
    """
    text = strip_ansi(text)
    cluster_ids = []
    cluster_col_idx = None

    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue

        if "|" not in stripped:
            continue

        parts = [p.strip() for p in stripped.split("|")]

        # Find the header row to determine ClusterId column index
        if cluster_col_idx is None:
            if "ClusterId" in parts:
                cluster_col_idx = parts.index("ClusterId")
            continue

        # Data rows: extract value at ClusterId column
        if len(parts) > cluster_col_idx:
            value = parts[cluster_col_idx].strip()
            if UUID_PATTERN.fullmatch(value) and value not in cluster_ids:
                cluster_ids.append(value)

    return cluster_ids


def parse_timestamps(timestamps_path: str) -> tuple:
    """Parse start_ns and end_ns from the timestamps file produced by the dry-run job.

    File format (one key=value per line):
        start_ns=1234567890123456789
        end_ns=1234567890123456789
    """
    values = {}
    with open(timestamps_path) as f:
        for line in f:
            line = line.strip()
            if "=" in line:
                key, val = line.split("=", 1)
                values[key.strip()] = int(val.strip())
    return values["start_ns"], values["end_ns"]


def _query_loki_page(logql: str, auth_header: str, start_ns: int, end_ns: int) -> list:
    """Fetch one page of Loki results. Returns list of (timestamp_ns, line) tuples."""
    params = urllib.parse.urlencode({
        "query": logql,
        "start": str(start_ns),
        "end": str(end_ns),
        "limit": str(LOKI_LIMIT),
        "direction": "forward",
    })
    url = f"{LOKI_BASE_URL}/loki/api/v1/query_range?{params}"

    req = urllib.request.Request(
        url, headers={"Authorization": auth_header}
    )

    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read())

    entries = []
    for stream in data.get("data", {}).get("result", []):
        for ts, line in stream.get("values", []):
            entries.append((int(ts), line))
    return entries


def query_loki(
    cluster_id: str,
    username: str,
    password: str,
    start_ns: int,
    end_ns: int,
    analyze_terraform: bool = True,
    analyze_helm: bool = True,
) -> tuple:
    """Query Loki for diff logs, paginating until all lines are fetched. Returns (log_text, truncated_flag)."""
    if analyze_terraform and analyze_helm:
        diff_filter = '|~ "infra-diff-terraform|infra-diff-helm"'
    elif analyze_terraform:
        diff_filter = '|= "infra-diff-terraform"'
    else:
        diff_filter = '|= "infra-diff-helm"'
    logql = (
        '{container="qovery-engine"}'
        f' |= `cluster_id: "{cluster_id}"`'
        f' {diff_filter}'
        ' | regexp `step: "(?P<step>[^"]+)".*message: (?P<message>.*)$`'
        ' | line_format "[{{ regexReplaceAll `infra-diff-` .step `` }}] {{.message}}"'
    )
    credentials = base64.b64encode(f"{username}:{password}".encode()).decode()
    auth_header = f"Basic {credentials}"

    all_entries = []
    current_start = start_ns
    truncated = False

    while True:
        entries = _query_loki_page(logql, auth_header, current_start, end_ns)
        all_entries.extend(entries)

        if len(entries) < LOKI_LIMIT:
            break

        truncated = True
        current_start = entries[-1][0] + 1

    lines = [line for _, line in all_entries]
    return "\n".join(lines), truncated


def _call_claude(prompt: str, api_key: str, usage_acc: dict = None, model: str = None) -> dict:
    """Send a prompt to Claude and return the parsed JSON response."""
    payload = json.dumps({
        "model": model or CLAUDE_MODEL,
        "max_tokens": 8192,
        "messages": [{"role": "user", "content": prompt}],
    }).encode()
    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages",
        data=payload,
        headers={
            "x-api-key": api_key,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json",
        },
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        data = json.loads(resp.read())
    if usage_acc is not None:
        usage = data.get("usage", {})
        usage_acc["input_tokens"] += usage.get("input_tokens", 0)
        usage_acc["output_tokens"] += usage.get("output_tokens", 0)
    return _extract_json(data["content"][0]["text"])


def _split_at_line_boundaries(text: str, max_chars: int) -> list:
    """Split text into chunks of at most max_chars, breaking at newlines."""
    if not text:
        return [text]
    batches = []
    start = 0
    while start < len(text):
        end = start + max_chars
        if end >= len(text):
            batches.append(text[start:])
            break
        nl = text.rfind("\n", start, end)
        if nl > start:
            end = nl + 1
        batches.append(text[start:end])
        start = end
    return batches or [text]


def analyze_with_claude(logs: str, api_key: str, usage_acc: dict = None, cluster_count: int = 1) -> dict:
    """Analyze logs with Claude, batching every MAX_BATCH_CHARS chars, then synthesizing."""
    batches = _split_at_line_boundaries(logs, MAX_BATCH_CHARS)
    total = len(batches)

    all_findings = []
    last_result: dict = {}
    for idx, chunk in enumerate(batches, 1):
        if total > 1:
            print(f" batch {idx}/{total}...", end="", flush=True, file=sys.stderr)
        batch_note = (
            f"NOTE: This is log batch {idx} of {total} — full logs split due to size.\n\n"
            if total > 1 else ""
        )
        cluster_desc = (
            f"representing a diff pattern shared by {cluster_count} cluster(s)"
            if cluster_count > 1
            else "on a single cluster"
        )
        prompt = (
            f"You are a Kubernetes infrastructure safety reviewer. Below are logs from a "
            f"dry-run deployment {cluster_desc}, containing terraform plan diffs "
            f"and helm chart diffs (lines tagged infra-diff-terraform or infra-diff-helm).\n\n"
            f"{batch_note}"
            f"Your job: identify anything dangerous, destructive, or unexpected.\n\n"
            f"CRITICAL (outage risk):\n"
            f"- Resource destroy/replace: look for 'will be destroyed', 'must be replaced'\n"
            f"- Routes ACTUALLY removed: a cidr_block appears in '-' lines but NOT in '+' lines\n"
            f"- Security group rules deleted without replacement\n"
            f"- Node pool replacements or count reductions\n\n"
            f"REVIEW (needs human validation):\n"
            f"- IAM policy changes\n"
            f"- Unexpected version downgrades\n"
            f"- Helm chart values that look wrong or risky\n"
            f"- null_resource replacements that trigger side effects (EXCEPT set_subnetwork_vpc_log_flow — see below)\n\n"
            f"NOT dangerous (classify as info, not review):\n"
            f"- Container image tag updates\n"
            f"- In-place S3 encryption reconfiguration\n"
            f"- Route reordering: ONLY classify as info if EVERY cidr_block that appears in '-' lines "
            f"also appears in a '+' line. If even one cidr_block appears ONLY in '-' lines with no "
            f"matching '+' entry, that is a REAL route deletion — classify as CRITICAL. "
            f"Attribute name changes (vpc_peering_connection_id vs gateway_id) do not matter; "
            f"what matters is whether the CIDR itself is present on both sides.\n"
            f"- Tag-only changes (metadata)\n"
            f"- null_resource.set_subnetwork_vpc_log_flow replacements: this resource uses "
            f"always_run = timestamp() intentionally — it always replaces on every apply and "
            f"only reconfigures GCP VPC flow logs via gcloud commands. Classify as info.\n"
            f"- null_resource.autoscaling_suspend_workers_nodes_* replacements: this resource uses "
            f"always_run = timestamp() intentionally — it always replaces on every apply and "
            f"only suspends AZRebalance on the EKS node group autoscaling group, which is harmless. Classify as info.\n\n"
            f"{ANALYSIS_SCHEMA_NOTE}\n\n"
            f"Logs:\n{chunk}"
        )
        last_result = retry(lambda p=prompt: _call_claude(p, api_key, usage_acc))
        all_findings.extend(last_result.get("findings", []))

    if total == 1:
        return last_result

    print(f" synthesizing...", end="", flush=True, file=sys.stderr)
    synthesis_prompt = (
        f"You are a Kubernetes infrastructure safety reviewer consolidating findings from "
        f"{total} log batches (pattern shared by {cluster_count} cluster(s)).\n\n"
        f"Deduplicate similar findings, keep the most specific description per issue, "
        f"and rank by severity.\n\n"
        f"{ANALYSIS_SCHEMA_NOTE}\n\n"
        f"Raw findings from all batches:\n{json.dumps(all_findings)}"
    )
    return retry(lambda: _call_claude(synthesis_prompt, api_key, usage_acc))


def generate_summary(group_results: list, non_analyzed: dict, api_key: str, usage_acc: dict = None) -> dict:
    """Call Claude to produce an executive summary from per-group analysis results.

    group_results: list of dicts with keys:
        group (dict with cluster_ids, is_common), analysis (dict or None), error (str or None)
    non_analyzed: dict with keys:
        no_logs (list of cluster_ids), errors (list of {cluster_id, error}),
        skipped (list of {cluster_id, grafana_url})
    """
    input_data = []
    for r in group_results:
        entry = {
            "cluster_count": len(r["group"]["cluster_ids"]),
            "is_common_pattern": r["group"]["is_common"],
        }
        if r.get("error"):
            entry["status"] = "error"
            entry["error"] = r["error"]
        elif r.get("analysis"):
            entry["status"] = "analyzed"
            entry["severity"] = r["analysis"]["severity"]
            entry["findings"] = r["analysis"].get("findings", [])
        input_data.append(entry)

    n_errors = len(non_analyzed.get("errors", []))
    n_no_logs = len(non_analyzed.get("no_logs", []))
    n_skipped = len(non_analyzed.get("skipped", []))

    prompt = (
        "You are summarizing the safety analysis of a dry-run deployment across multiple "
        "Kubernetes clusters. Clusters with identical diffs were grouped and analyzed once. "
        "Below is the JSON array of per-group results.\n\n"
        f"Additionally: {n_no_logs} cluster(s) had no logs, {n_errors} had errors, "
        f"{n_skipped} were skipped (too large).\n\n"
        "Return ONLY valid JSON (no markdown fences) with this structure:\n"
        '{"verdict": "<one-line overall safety verdict>", '
        '"verdict_severity": "<critical|review|info>", '
        '"actions": ["[SEVERITY] [TOOL] action text"]}\n\n'
        "Format each action as: [CRITICAL] or [REVIEW] (severity), then [TERRAFORM] or [HELM] (tool), then the action text.\n"
        "Example: \"[REVIEW] [TERRAFORM] Verify route table changes on 2 clusters\"\n"
        "Only include actions for critical and review severity findings. "
        "Do NOT generate actions for info severity items — those are routine and need no operator input.\n"
        "Be concise. Focus on whether the deployment is safe to proceed.\n\n"
        f"Per-group results:\n{json.dumps(input_data)}"
    )

    return _call_claude(prompt, api_key, usage_acc, model=CLAUDE_VERDICT_MODEL)


def main(
    dry_run_output_path: str,
    timestamps_path: str,
    analyze_terraform: bool = True,
    analyze_helm: bool = True,
    verbose: bool = False,
    json_output=None,
) -> None:
    renderer, progress, stdout_renderer = create_renderer(
        verbose=verbose, json_output=json_output,
    )

    loki_username = os.environ["LOKI_USERNAME"]
    loki_password = os.environ["LOKI_PASSWORD"]
    api_key = os.environ["ANTHROPIC_API_KEY"]

    usage_acc = {"input_tokens": 0, "output_tokens": 0}
    timings = {}

    start_ns, end_ns = parse_timestamps(timestamps_path)
    start_human = datetime.fromtimestamp(start_ns / 1e9, tz=timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    end_human = datetime.fromtimestamp(end_ns / 1e9, tz=timezone.utc).strftime("%H:%M UTC")

    with open(dry_run_output_path) as f:
        content = f.read()

    cluster_ids = parse_cluster_ids(content)
    if not cluster_ids:
        print("WARNING: no cluster IDs found in dry-run output, nothing to analyze.", file=sys.stderr)
        return

    total_clusters = len(cluster_ids)

    # Phase 1: Download
    t0 = time.monotonic()
    cluster_logs = {}
    non_analyzed = {"no_logs": [], "errors": [], "skipped": []}

    def _download_one(cluster_id):
        logs, truncated = retry(
            lambda cid=cluster_id: query_loki(
                cid, loki_username, loki_password, start_ns, end_ns,
                analyze_terraform=analyze_terraform, analyze_helm=analyze_helm,
            )
        )
        return logs.strip(), truncated

    progress.start("download", total_clusters)
    with ThreadPoolExecutor(max_workers=10) as pool:
        futures = {pool.submit(_download_one, cid): cid for cid in cluster_ids}
        for idx, future in enumerate(as_completed(futures), 1):
            cluster_id = futures[future]
            try:
                logs_stripped, truncated = future.result()
                line_count = len(logs_stripped.splitlines()) if logs_stripped else 0
                trunc_note = ", truncated" if truncated else ""
                progress.verbose_detail(f"  [{idx}/{total_clusters}] {cluster_id}... {line_count} lines{trunc_note}")
            except Exception as e:
                non_analyzed["errors"].append({"cluster_id": cluster_id, "error": str(e)})
                progress.verbose_detail(f"  [{idx}/{total_clusters}] {cluster_id}... FAILED: {e}")
                progress.update("download", idx, total_clusters)
                continue

            if not logs_stripped:
                non_analyzed["no_logs"].append(cluster_id)
                progress.update("download", idx, total_clusters)
                continue

            batch_count = (len(logs_stripped) + MAX_BATCH_CHARS - 1) // MAX_BATCH_CHARS
            if batch_count > MAX_BATCH_LIMIT:
                grafana = _grafana_url(cluster_id, start_ns, end_ns)
                non_analyzed["skipped"].append({"cluster_id": cluster_id, "grafana_url": grafana})
                progress.verbose_detail(f"  [{idx}/{total_clusters}] {cluster_id}... skipped (too large, {batch_count} batches)")
                progress.update("download", idx, total_clusters)
                continue

            cluster_logs[cluster_id] = logs_stripped
            progress.update("download", idx, total_clusters)

    n_with_logs = len(cluster_logs)
    n_no_logs = len(non_analyzed["no_logs"])
    n_errors = len(non_analyzed["errors"])
    progress.finish("download", f"Downloaded {total_clusters} clusters ({n_with_logs} with logs, {n_no_logs} no logs, {n_errors} errors)")
    timings["download_s"] = time.monotonic() - t0

    # Phase 2: Fingerprint and group
    t0 = time.monotonic()
    if not cluster_logs:
        groups = []
        n_groups = 0
    else:
        groups = fingerprint_and_group(cluster_logs)
        n_groups = len(groups)
    timings["grouping_s"] = time.monotonic() - t0

    # Phase 3: Analyze
    t0 = time.monotonic()
    group_results = []

    if n_groups > 0:
        def _analyze_one(idx_group):
            idx, group = idx_group
            local_usage = {"input_tokens": 0, "output_tokens": 0}
            analysis = retry(
                lambda g=group: analyze_with_claude(
                    g["representative_logs"], api_key, local_usage,
                    cluster_count=len(g["cluster_ids"]),
                )
            )
            return idx, group, analysis, local_usage

        progress.start("analysis", n_groups)
        with ThreadPoolExecutor(max_workers=10) as pool:
            futures = {pool.submit(_analyze_one, (i, g)): i for i, g in enumerate(groups, 1)}
            for completed, future in enumerate(as_completed(futures), 1):
                idx = futures[future]
                group = groups[idx - 1]
                n = len(group["cluster_ids"])
                label = "Common pattern" if group["is_common"] else f"Pattern {idx}"
                try:
                    _, _, analysis, local_usage = future.result()
                    usage_acc["input_tokens"] += local_usage["input_tokens"]
                    usage_acc["output_tokens"] += local_usage["output_tokens"]
                    sev = analysis.get("severity", "?")
                    progress.verbose_detail(f"  [{label}] {n} cluster{'s' if n != 1 else ''} — {sev}")
                    group_results.append({"group": group, "analysis": analysis, "error": None})
                except Exception as e:
                    progress.verbose_detail(f"  [{label}] {n} cluster{'s' if n != 1 else ''} — FAILED: {e}")
                    group_results.append({"group": group, "analysis": None, "error": str(e)})
                progress.update("analysis", completed, n_groups)

        sev_counts_analysis = {"critical": 0, "review": 0, "info": 0}
        for r in group_results:
            s = (r.get("analysis") or {}).get("severity", "info")
            if s in sev_counts_analysis:
                sev_counts_analysis[s] += len(r["group"]["cluster_ids"])
        progress.finish("analysis", f"Analyzed {n_groups} patterns ({sev_counts_analysis['critical']} critical, {sev_counts_analysis['review']} review, {sev_counts_analysis['info']} info)")

    timings["analysis_s"] = time.monotonic() - t0

    # Phase 4: Verdict
    t0 = time.monotonic()
    verdict = "N/A"
    verdict_severity = "unknown"
    actions = []
    try:
        summary = retry(lambda: generate_summary(group_results, non_analyzed, api_key, usage_acc))
        verdict = summary.get("verdict", "N/A")
        verdict_severity = summary.get("verdict_severity", "unknown")
        actions = summary.get("actions", [])
    except Exception as e:
        verdict = f"ERROR generating verdict: {e}"
    timings["verdict_s"] = time.monotonic() - t0
    timings["total_s"] = sum(timings.values())

    # Build Report
    INPUT_COST_PER_MTK = 3.00
    OUTPUT_COST_PER_MTK = 15.00
    input_cost = usage_acc["input_tokens"] / 1_000_000 * INPUT_COST_PER_MTK
    output_cost = usage_acc["output_tokens"] / 1_000_000 * OUTPUT_COST_PER_MTK

    # Flatten findings from group results
    findings = []
    for r in group_results:
        if r.get("error") or not r.get("analysis"):
            continue
        analysis = r["analysis"]
        cluster_ids_for_group = r["group"]["cluster_ids"]
        for f in analysis.get("findings", []):
            findings.append({
                "severity": f.get("severity", "info"),
                "title": f.get("title", f.get("description", "Unknown")),
                "impact": f.get("impact", f.get("description", "")),
                "source": f.get("source", "terraform"),
                "category": f.get("category", "other"),
                "action": f.get("action", "review"),
                "affected_clusters": cluster_ids_for_group,
                "grafana_url": _grafana_url(cluster_ids_for_group[0], start_ns, end_ns) if cluster_ids_for_group else "",
                "sample_diff": r["group"]["representative_logs"][:2000] if r["group"].get("representative_logs") else "",
            })

    # Severity counts
    sev_cluster_counts = {"critical": 0, "review": 0, "info": 0, "unknown": 0}
    counted_clusters = set()
    for f in findings:
        for cid in f["affected_clusters"]:
            key = (f["severity"], cid)
            if key not in counted_clusters:
                counted_clusters.add(key)
                if f["severity"] in sev_cluster_counts:
                    sev_cluster_counts[f["severity"]] += 1
    # Clusters with no findings → info
    for r in group_results:
        if not r.get("error") and r.get("analysis"):
            analysis_findings = r["analysis"].get("findings", [])
            if not analysis_findings or all(f.get("severity") == "info" for f in analysis_findings):
                for cid in r["group"]["cluster_ids"]:
                    if not any((s, cid) in counted_clusters for s in ("critical", "review")):
                        if ("info", cid) not in counted_clusters:
                            counted_clusters.add(("info", cid))
                            sev_cluster_counts["info"] += 1
    sev_cluster_counts["unknown"] = len(non_analyzed["no_logs"]) + len(non_analyzed["errors"]) + len(non_analyzed["skipped"])

    report = {
        "window": {
            "from_utc": start_human,
            "to_utc": end_human,
            "start_ns": start_ns,
            "end_ns": end_ns,
        },
        "clusters_total": total_clusters,
        "clusters_with_logs": n_with_logs,
        "clusters_no_logs": non_analyzed["no_logs"],
        "clusters_errored": non_analyzed["errors"],
        "clusters_skipped": non_analyzed["skipped"],
        "patterns_total": n_groups,
        "verdict": verdict,
        "verdict_severity": verdict_severity,
        "severity_counts": sev_cluster_counts,
        "actions": actions,
        "findings": findings,
        "usage": {
            "model_analysis": CLAUDE_MODEL,
            "model_verdict": CLAUDE_VERDICT_MODEL,
            "input_tokens": usage_acc["input_tokens"],
            "output_tokens": usage_acc["output_tokens"],
            "estimated_cost_usd": round(input_cost + output_cost, 4),
        },
        "timing": {k: round(v, 1) for k, v in timings.items()},
        "grafana_url": _grafana_base_url(start_ns, end_ns),
    }

    renderer.render(report)
    if stdout_renderer:
        stdout_renderer.render(report)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(
        description="CI Release AI Check — downloads Loki diff logs per cluster and runs Claude analysis."
    )
    parser.add_argument("dry_run_output", help="Path to the dry-run output file")
    parser.add_argument("timestamps", help="Path to the timestamps file (start_ns/end_ns)")
    parser.add_argument("--analyze-terraform", action="store_true", default=False,
                        help="Analyze Terraform diffs")
    parser.add_argument("--analyze-helm", action="store_true", default=False,
                        help="Analyze Helm diffs")
    parser.add_argument("--verbose", action="store_true", default=False,
                        help="Show per-cluster progress, sample diffs, timing, and API cost")
    parser.add_argument("--json", nargs="?", const=True, default=None, dest="json_output",
                        metavar="PATH",
                        help="Output JSON (to stdout, or to PATH if specified)")
    args = parser.parse_args()

    if not args.analyze_terraform and not args.analyze_helm:
        parser.error("at least one of --analyze-terraform or --analyze-helm is required")
    main(
        args.dry_run_output,
        args.timestamps,
        analyze_terraform=args.analyze_terraform,
        analyze_helm=args.analyze_helm,
        verbose=args.verbose,
        json_output=args.json_output,
    )
