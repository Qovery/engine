"""Output renderers for CI Release AI Check."""

import json
import re
import sys
import textwrap

SEVERITY_EMOJI = {
    "critical": "🔴",
    "review": "🟡",
    "info": "🟢",
    "unknown": "⚪",
}

SEVERITY_COLOR = {
    "critical": "red",
    "review": "yellow",
    "info": "green",
    "unknown": "dim",
}

# Severity ordering for merge tie-breaking: higher wins. Used to pick a merged group's
# representative member so a benign finding can never mask a more severe one sharing its
# normalized title.
_SEVERITY_RANK = {"critical": 3, "review": 2, "info": 1, "unknown": 0}

ANSI_CODES = {
    "red": "\033[31m",
    "yellow": "\033[33m",
    "green": "\033[32m",
    "dim": "\033[2m",
    "bold": "\033[1m",
    "reset": "\033[0m",
}

_ANSI_RE = re.compile(r"\033\[[0-9;]*[A-Za-z]")


def color(text: str, style: str) -> str:
    code = ANSI_CODES.get(style, "")
    return f"{code}{text}{ANSI_CODES['reset']}" if code else text


def strip_ansi(text: str) -> str:
    return _ANSI_RE.sub("", text)


# Strip cluster-specific tokens from a finding title so the SAME issue on different
# clusters normalizes identically, while DIFFERENT issues stay distinct.
_TITLE_NORMALIZE = [
    (re.compile(r'qovery-[a-z]+-\S+'), 'qovery-bucket'),                                   # bucket names: qovery-logs-zXXXX
    (re.compile(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}', re.I), ''),  # uuids
    (re.compile(r'\bz[0-9a-f]{8,9}\b'), ''),                                               # qovery cluster short ids
    (re.compile(r'\bv?\d+(?:\.\d+)+\b'), ''),                                              # version numbers v1.301.0
    (re.compile(r'\b[0-9a-f]{8,}\b'), ''),                                                 # long hex / kms key ids
    (re.compile(r'\d+'), ''),                                                              # remaining digits/counts
    (re.compile(r'[^a-z ]+'), ' '),                                                        # punctuation
    (re.compile(r'\s+'), ' '),                                                             # collapse whitespace
]


def _normalize_title(title: str) -> str:
    """Cluster-agnostic, issue-specific signature of a finding title for grouping."""
    t = title.lower()
    for pattern, replacement in _TITLE_NORMALIZE:
        t = pattern.sub(replacement, t)
    return t.strip()


def finding_group_key(finding: dict) -> tuple:
    """Grouping key: (source, normalized title).

    Keying on the normalized title means the SAME issue recurring across clusters
    merges (cluster-specific tokens are stripped), while DIFFERENT issues stay
    distinct. A category-based key would fold unrelated findings together (e.g. an
    HTTPS bucket policy and an IAM permission removal), and the merge would then
    silently drop all but the largest — hiding real findings. Over-separating is a
    far safer failure mode for a safety tool than silently dropping a finding.
    """
    return (finding.get("source", ""), _normalize_title(finding.get("title", "")))


def merge_findings(findings: list) -> list:
    """Merge findings that describe the same issue, unioning their affected clusters.

    Each group's surviving fields (severity/title/impact/action/resource/grafana_url/…)
    all come from ONE representative member: the MOST SEVERE, tie-broken by
    affected-cluster count. Choosing the representative by severity — not blast radius —
    means a benign member can never mask a more severe one that normalizes to the same
    title: the verdict pass then re-judges the scariest wording, so it cannot under-rate
    what it cannot see. Copying every field from the same member (rather than patching a
    subset) also keeps metadata consistent with the shown text — e.g. grafana_url points
    at the representative's cluster, not an unrelated first-seen member's.

    Returns groups sorted by affected-cluster count, descending. Applied ONCE upstream
    (before the verdict pass) so the model judges severity at the same granularity the
    operator sees. Idempotent, so re-running it in the renderer is a no-op.
    """
    groups: dict = {}
    order: list = []
    for f in findings:
        key = finding_group_key(f)
        if key not in groups:
            groups[key] = []
            order.append(key)
        groups[key].append(f)

    merged = []
    for key in order:
        members = groups[key]
        clusters = list(dict.fromkeys(
            cid for m in members for cid in m["affected_clusters"]
        ))
        rep = max(members, key=lambda m: (
            _SEVERITY_RANK.get(m.get("severity"), 0), len(m["affected_clusters"]),
        ))
        # Aggregate every distinct affected resource (representative's first), rather than
        # keeping only one: the group key strips resource-identifying tokens, so members
        # can name DIFFERENT real resources. Dropping all but one would silently hide
        # co-affected resources — the exact thing the new resource field exists to surface.
        resources = list(dict.fromkeys(
            m.get("resource", "") for m in [rep, *members] if m.get("resource", "")
        ))
        merged.append({**rep, "affected_clusters": clusters, "resource": ", ".join(resources)})
    return sorted(merged, key=lambda g: len(g["affected_clusters"]), reverse=True)


def should_use_color(stream=None) -> bool:
    stream = stream or sys.stderr
    return hasattr(stream, "isatty") and stream.isatty()


PHASE_LABELS = {
    "download": ("Downloading logs", "⏳"),
    "analysis": ("Analyzing patterns", "⏳"),
    "verdict": ("Generating verdict", "⏳"),
}


class ProgressReporter:
    def __init__(self, stream=None, is_tty=None, verbose=False):
        self._stream = stream or sys.stderr
        self._is_tty = is_tty if is_tty is not None else (hasattr(self._stream, "isatty") and self._stream.isatty())
        self._verbose = verbose
        self._last_line_len = 0

    def _write(self, text: str, newline: bool = True) -> None:
        self._stream.write(text + ("\n" if newline else ""))
        self._stream.flush()

    def _clear_line(self) -> None:
        if self._is_tty and self._last_line_len:
            self._stream.write("\r" + " " * self._last_line_len + "\r")
            self._stream.flush()

    def start(self, phase: str, total: int) -> None:
        label, emoji = PHASE_LABELS.get(phase, (phase, "⏳"))
        if self._is_tty:
            line = f"{emoji} {label}: 0/{total}..."
            self._write(line, newline=False)
            self._last_line_len = len(line)
        else:
            count_label = "clusters" if phase == "download" else "patterns"
            self._write(f"{label} for {total} {count_label}...")

    def update(self, phase: str, current: int, total: int) -> None:
        if not self._is_tty:
            return
        label, emoji = PHASE_LABELS.get(phase, (phase, "⏳"))
        self._clear_line()
        line = f"{emoji} {label}: {current}/{total}..."
        self._write(line, newline=False)
        self._last_line_len = len(line)

    def finish(self, phase: str, summary: str) -> None:
        if self._is_tty:
            self._clear_line()
            self._write(f"✅ {summary}")
            self._last_line_len = 0
        else:
            self._write(summary)

    def verbose_detail(self, line: str) -> None:
        if self._verbose:
            self._write(line)


class Renderer:
    def __init__(self, stream=None, use_color=None):
        self._stream = stream or sys.stdout
        self._use_color = use_color if use_color is not None else should_use_color(self._stream)

    def _write(self, text: str = "") -> None:
        self._stream.write(text + "\n")

    def _styled(self, text: str, style: str) -> str:
        return color(text, style) if self._use_color else text

    def _severity_prefix(self, severity: str) -> str:
        emoji = SEVERITY_EMOJI.get(severity, "")
        return f"{emoji} " if emoji else ""

    def _write_labeled(self, label: str, text: str, width: int = 120) -> None:
        """Write a labeled line with hanging indent on wrap, e.g. '     Impact: ...'."""
        first_line = label + text
        if len(strip_ansi(first_line)) <= width:
            self._write(first_line)
            return
        indent = " " * len(label)
        wrapped = textwrap.fill(text, width=width - len(label), subsequent_indent=indent)
        for i, line in enumerate(wrapped.splitlines()):
            self._write(label + line if i == 0 else indent + line.lstrip())

    def _render_disclaimer_box(self, grafana_url: str = None) -> None:
        url = grafana_url or "https://qortal.qovery.com/grafana/d/ae51ecxhq2tj4a/infra-cluster-diff"
        border = "═" * 75
        lines = [
            border,
            "⚠️  BETA — AI-generated analysis — always verify findings manually",
            "    before proceeding. The model may misclassify changes or miss issues.",
            "    Always cross-check with the Grafana dashboard before proceeding:",
            f"    {url}",
            border,
        ]
        for line in lines:
            self._write(self._styled(line, "yellow"))

    def render(self, report: dict) -> None:
        raise NotImplementedError


class DefaultRenderer(Renderer):
    # cluster_id -> Qovery org name, for labeling internal clusters in cluster lists.
    # Populated by render(); the class-level default lets _render_cluster_list run
    # standalone (e.g. in tests) without a render() pass.
    _qovery_cluster_names = {}
    # raw finding id -> group label (C1/R1/...). Populated by _prepare_labels();
    # class-level default lets render helpers run standalone in tests.
    _id_to_label: dict = {}

    def render(self, report: dict) -> None:
        self._qovery_cluster_names = report.get("qovery_cluster_names", {})
        self._prepare_labels(report)
        self._render_header(report)
        self._render_verdict(report)
        self._render_summary_table(report)
        self._render_findings(report)
        self._render_unknown(report)
        self._render_footer(report)

    def _render_header(self, report: dict) -> None:
        window = report["window"]
        self._write("Terraform/Helm Diff Analysis")
        self._write(f"Window: {window['from_utc']} → {window['to_utc']}")

    def _render_verdict(self, report: dict) -> None:
        sev = report.get("verdict_severity", "unknown")
        prefix = self._severity_prefix(sev)
        verdict_text = report["verdict"]
        self._write()
        self._write(f"{prefix}{self._styled(f'VERDICT: {verdict_text}', SEVERITY_COLOR.get(sev, 'bold'))}")

    def _render_summary_table(self, report: dict) -> None:
        counts = report["severity_counts"]
        self._write()
        self._write(f"{'Severity':<14} {'Patterns':>8}   {'Clusters':>8}   Action")
        rows = [
            ("critical", "CRITICAL", "stop"),
            ("review", "REVIEW", "review"),
            ("info", "INFO", "none"),
            ("unknown", "UNKNOWN", "investigate"),
        ]
        for key, label, action in rows:
            prefix = self._severity_prefix(key)
            n_clusters = counts.get(key, 0)
            n_patterns = self._count_patterns_for_severity(report, key)
            self._write(f"{prefix}{label:<11} {n_patterns:>8}   {n_clusters:>8}   {action}")

    def _count_patterns_for_severity(self, report: dict, severity: str) -> int:
        if severity == "unknown":
            count = 0
            if report.get("clusters_no_logs"):
                count += 1
            if report.get("clusters_errored"):
                count += 1
            if report.get("clusters_skipped"):
                count += 1
            return count
        return sum(1 for f in report.get("findings", []) if f["severity"] == severity)

    def _prepare_labels(self, report: dict) -> None:
        """Assign a stable label (C1/C2… critical, R1/R2… review) to each rendered
        finding group. Must mirror the grouping/ordering used by _render_findings.
        """
        self._id_to_label = {}
        findings = report.get("findings", [])

        crit = sorted(
            [f for f in findings if f.get("severity") == "critical"],
            key=lambda f: len(f["affected_clusters"]), reverse=True,
        )
        for i, f in enumerate(crit, 1):
            if f.get("id") is not None:
                self._id_to_label[f["id"]] = f"C{i}"

        review = [f for f in findings if f.get("severity") == "review"]
        label_for_key = {finding_group_key(g): f"R{i}" for i, g in enumerate(merge_findings(review), 1)}
        for f in review:
            label = label_for_key.get(finding_group_key(f))
            if label and f.get("id") is not None:
                self._id_to_label[f["id"]] = label

    def _render_findings(self, report: dict) -> None:
        findings = report.get("findings", [])

        for severity, section_title in [("critical", "Critical findings"), ("review", "Review findings")]:
            raw = [f for f in findings if f["severity"] == severity]
            if not raw:
                continue
            # Findings arrive already merged upstream (merge_findings before the verdict
            # pass), so grouping here is idempotent. Critical stay one-per-line ordered by
            # blast radius; review re-runs the (no-op) merge to keep the ordering contract.
            if severity == "critical":
                sev_findings = sorted(raw, key=lambda f: len(f["affected_clusters"]), reverse=True)
            else:
                sev_findings = merge_findings(raw)
            self._write()
            self._write(section_title)
            for f in sev_findings:
                self._render_finding_detail(f)

        info_findings = merge_findings([f for f in findings if f["severity"] == "info"])
        if info_findings:
            self._write()
            self._write("Info findings")
            for f in info_findings:
                self._render_finding_info(f)

    def _source_tag(self, finding: dict) -> str:
        src = finding.get("source", "").upper()
        return f"[{src}] " if src else ""

    def _render_finding_detail(self, finding: dict, include_impact_action: bool = True) -> None:
        sev = finding["severity"]
        prefix = self._severity_prefix(sev)
        n = len(finding["affected_clusters"])
        label = self._id_to_label.get(finding.get("id"))
        label_part = f"[{label}] " if label else ""
        self._write(f"  {label_part}{prefix}{self._source_tag(finding)}{finding['title']} ({n} cluster{'s' if n != 1 else ''})")
        resource = finding.get("resource", "")
        if resource:
            self._write_labeled("     Resource: ", resource)
        if include_impact_action:
            self._write_labeled("     Impact: ", finding['impact'])
            self._write_labeled("     Action: ", finding['action'])
        self._render_cluster_list(finding["affected_clusters"], indent="     ")

    def _render_finding_info(self, finding: dict) -> None:
        self._render_finding_detail(finding, include_impact_action=False)

    def _cluster_label(self, cid: str) -> str:
        """Append the Qovery org name for internal clusters; customer clusters stay bare."""
        name = self._qovery_cluster_names.get(cid)
        return f"{cid} — {name}" if name else cid

    def _render_cluster_list(self, cluster_ids: list, indent: str = "     ") -> None:
        if not cluster_ids:
            return
        self._write(f"{indent}Clusters: {self._cluster_label(cluster_ids[0])}")
        padding = " " * len("Clusters: ")
        for cid in cluster_ids[1:]:
            self._write(f"{indent}{padding}{self._cluster_label(cid)}")

    def _render_unknown(self, report: dict) -> None:
        no_logs = report.get("clusters_no_logs", [])
        errored = report.get("clusters_errored", [])
        skipped = report.get("clusters_skipped", [])
        if not no_logs and not errored and not skipped:
            return

        self._write()
        self._write("Unknown")

        if no_logs:
            prefix = self._severity_prefix("unknown")
            n = len(no_logs)
            self._write(f"  {prefix}No logs ({n} cluster{'s' if n != 1 else ''})")
            self._render_cluster_list(no_logs, indent="     ")

        if errored:
            prefix = self._severity_prefix("unknown")
            n = len(errored)
            self._write(f"  {prefix}Download errors ({n} cluster{'s' if n != 1 else ''})")
            for e in errored:
                self._write(f"     {e['cluster_id']}: {e['error']}")

        if skipped:
            prefix = self._severity_prefix("unknown")
            n = len(skipped)
            self._write(f"  {prefix}Skipped — too large ({n} cluster{'s' if n != 1 else ''})")
            for s in skipped:
                self._write(f"     {s['cluster_id']}: {s['grafana_url']}")

    def _render_footer(self, report: dict = None) -> None:
        usage = (report or {}).get("usage", {})
        cost = usage.get("estimated_cost_usd")
        if cost is not None:
            self._write()
            self._write(f"AI analysis cost: ${cost:.4f}")
        self._write()
        self._render_disclaimer_box((report or {}).get("grafana_url"))


SAMPLE_DIFF_MAX_LINES = 10


class VerboseRenderer(DefaultRenderer):
    def render(self, report: dict) -> None:
        super().render(report)
        self._render_timing(report)
        self._render_cost(report)

    def _render_finding_detail(self, finding: dict, include_impact_action: bool = True) -> None:
        super()._render_finding_detail(finding, include_impact_action=include_impact_action)
        self._render_sample_diff(finding)

    def _render_finding_info(self, finding: dict) -> None:
        self._render_finding_detail(finding, include_impact_action=True)

    def _render_sample_diff(self, finding: dict) -> None:
        diff = finding.get("sample_diff", "")
        if not diff:
            return
        lines = diff.splitlines()
        truncated = len(lines) > SAMPLE_DIFF_MAX_LINES
        display_lines = lines[:SAMPLE_DIFF_MAX_LINES]
        self._write(f"     Sample diff:")
        for line in display_lines:
            self._write(f"       {line}")
        if truncated:
            self._write(f"       ... ({len(lines) - SAMPLE_DIFF_MAX_LINES} more lines)")

    def _render_timing(self, report: dict) -> None:
        timing = report.get("timing")
        if not timing:
            return
        self._write()
        self._write("Timing")
        self._write(f"  Download:  {timing['download_s']:.1f}s")
        self._write(f"  Grouping:  {timing['grouping_s']:.1f}s")
        self._write(f"  Analysis:  {timing['analysis_s']:.1f}s")
        self._write(f"  Verdict:   {timing['verdict_s']:.1f}s")
        self._write(f"  Total:     {timing['total_s']:.1f}s")

    def _render_cost(self, report: dict) -> None:
        usage = report.get("usage")
        if not usage:
            return
        input_tok = usage["input_tokens"]
        output_tok = usage["output_tokens"]
        cost = usage["estimated_cost_usd"]
        self._write()
        self._write("API cost")
        self._write(f"  Model (analysis): {usage['model_analysis']}")
        self._write(f"  Model (verdict):  {usage['model_verdict']}")
        self._write(f"  Input tokens:     {input_tok:>10,}")
        self._write(f"  Output tokens:    {output_tok:>10,}")
        self._write(f"  Total:            ${cost:.4f}")

    def _render_footer(self, report: dict = None) -> None:
        self._write()
        self._render_disclaimer_box((report or {}).get("grafana_url"))


JSON_FINDING_KEYS = {"severity", "title", "impact", "source", "resource", "category", "action", "affected_clusters", "grafana_url"}


class JsonRenderer(Renderer):
    def __init__(self, stream=None, file_path=None):
        super().__init__(stream=stream, use_color=False)
        self._file_path = file_path

    def render(self, report: dict) -> None:
        output = {
            "version": 1,
            "window": report["window"],
            "verdict": report["verdict"],
            "verdict_severity": report.get("verdict_severity", "unknown"),
            "clusters_total": report["clusters_total"],
            "clusters_with_logs": report["clusters_with_logs"],
            "clusters_no_logs": report["clusters_no_logs"],
            "clusters_errored": report["clusters_errored"],
            "clusters_skipped": report["clusters_skipped"],
            "patterns_total": report["patterns_total"],
            "severity_counts": report["severity_counts"],
            "findings": [
                {k: v for k, v in f.items() if k in JSON_FINDING_KEYS}
                for f in report.get("findings", [])
            ],
            "qovery_cluster_names": report.get("qovery_cluster_names", {}),
            "usage": report.get("usage", {}),
            "timing": report.get("timing", {}),
            "grafana_url": report.get("grafana_url", ""),
        }
        json_str = json.dumps(output, indent=2)

        if self._file_path:
            with open(self._file_path, "w") as f:
                f.write(json_str + "\n")
        else:
            self._write(json_str)


def create_renderer(verbose=False, json_output=None):
    """Create the appropriate renderer and progress reporter.

    Args:
        verbose: Enable verbose mode
        json_output: True for JSON to stdout, string path for JSON to file, None for no JSON

    Returns:
        (renderer, progress_reporter, stdout_renderer) — stdout_renderer is non-None only
        when json_output is a file path (so JSON goes to file, human output to stdout).
    """
    progress = ProgressReporter(verbose=verbose)
    stdout_renderer = None

    if json_output is True:
        renderer = JsonRenderer()
    elif isinstance(json_output, str):
        renderer = JsonRenderer(file_path=json_output)
        stdout_renderer = VerboseRenderer(use_color=should_use_color(sys.stdout)) if verbose else DefaultRenderer(use_color=should_use_color(sys.stdout))
    elif verbose:
        renderer = VerboseRenderer(use_color=should_use_color(sys.stdout))
    else:
        renderer = DefaultRenderer(use_color=should_use_color(sys.stdout))

    return renderer, progress, stdout_renderer
