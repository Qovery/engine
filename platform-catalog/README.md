# Platform catalog (Engine v2)

Source of truth for the **Qovery-owned configuration of platform components**
(cluster-agent, shell-agent, loki, …). q-core compiles the final Helm values of
each component from these files; the Engine worker only executes
`helm upgrade --install -f values.final.yaml`.

**Invariant: a config change must never require an Engine binary release.**
The publish flow below is therefore fully independent from the engine release
pipeline — the boundary is the published OCI artifact, not this repo.

## Layout

```text
platform-catalog/
  catalog.yaml                 # publish manifest — the helm-freeze.yaml of config bundles
  components/
    <component>/
      config/
        base.yaml              # source 1 — Qovery base values, identical everywhere
                               #   ("base": the upstream chart already has its own
                               #   defaults; this is the Qovery layer on top)
        overlays/              # source 2 — context fragments, convention-named:
                               #   <mode>.yaml then <provider>.yaml, kebab-cased enum
                               #   names (e.g. customer-managed.yaml, aws.yaml)
```

File semantics (the contract q-core's loader implements):

- an **absent** file is an empty fragment;
- a **present-but-invalid** YAML file is a hard compilation error;
- merge order: `base < mode overlay < provider overlay < managed config`.

cluster-agent and shell-agent have an intentionally empty (comments-only)
`base.yaml`: no Qovery-owned config yet — their values are q-core manifest
wiring. Keeping the file makes every component publishable as a bundle from
day one (uniform seam for q-core, no "component without bundle" special case)
and documents where config lands when it arrives. A comments-only file parses
to YAML null, which the loader must treat as an empty fragment.

## Relation to helm-freeze

Upstream charts are vendored by `helm-freeze` (see
[lib-engine/lib/helm-freeze.yaml](../lib-engine/lib/helm-freeze.yaml)) and stay
untouched. This catalog holds only what Qovery layers **on top** of those
charts. [catalog.yaml](catalog.yaml) mirrors the helm-freeze experience: one
reviewed manifest listing each component, its chart, and the bundle version to
publish.

The files under [lib-engine/lib/common/bootstrap/chart_values/](../lib-engine/lib/common/bootstrap/chart_values/)
are the *current* (Engine v1) values, read by the Engine binary at runtime.
They remain authoritative until q-core swaps a component to bundle-based
compilation; byte-level golden tests on compiled values are the regression net
for that swap.

## Publishing

Manual only, for now (decided 2026-07-08). Run the `publish-platform-catalog`
GitLab job (manual — it publishes the config bundles then the chart mirror;
set `PLATFORM_CONFIG_COMPONENTS` / `PLATFORM_CHARTS` to a name list or `none`
to restrict), or locally:

```bash
PLATFORM_CONFIG_REGISTRY=<registry> ./scripts/publish-platform-config.sh loki
```

Each component's `config/` directory is pushed with ORAS as one OCI artifact
(`artifactType: application/vnd.qovery.platform-config.v1`) to:

```text
<registry>/platform-config/<component>:<version>   # version from catalog.yaml
```

The script writes `platform-config-publish.json` (component, version, ref,
digest) — the digest is the pin q-core records. `oras pull` of the reference
restores the exact `config/` directory content.

## Chart mirroring

Config bundles deliberately do **not** embed the chart (independent lifecycles,
and upstream charts are not ours). Instead, the frozen chart copies are
mirrored as separate OCI charts on the same registry: the `charts:` list in
[catalog.yaml](catalog.yaml) is published by `scripts/publish-frozen-charts.sh`
(second step of the manual `publish-platform-catalog` GitLab job) to:

```text
oci://<registry>/charts/<name>:<Chart.yaml version>
```

Rationale: the v2 worker would otherwise pull charts from upstream repos
(e.g. grafana.github.io) inside the customer cluster at install time — the
frozen copy reviewed in this repo would *not* be the artifact actually
executed. For Qovery-authored charts (qovery-cluster-agent, qovery-shell-agent)
and the 7 `no_sync: true` Qovery-modified charts, an upstream pull is wrong,
not just fragile: that content exists nowhere upstream. q-core simply points
`chart.repository` at the mirror.

**To mirror another chart, add a name+path entry to the `charts:` list in
catalog.yaml, and declare its `charts/<name>` ECR repository in the infra
Terraform** (ECR does not auto-create repositories on push, and the publish
scripts deliberately don't either — registry repositories live in the infra
Terraform, like the rest of the AWS infra). Current
scope: loki, qovery-cluster-agent, qovery-shell-agent.

Caveat for later: Qovery-authored and `no_sync` charts keep a version number
that does not change with content (agents are pinned at `0.1.0`; `no_sync`
charts keep the upstream number). Under mutable-v0 this is fine — consumers
pin by digest — but bump the chart version on content changes, or settle a
`-qovery.N` suffix convention, before tags are made immutable.

## Decisions (state as of 2026-07-08)

- **Target registry**: `public.ecr.aws/r3m4q3r9` (public ECR, decided
  2026-07-09) — the engine/q-core running on customer clusters pulls bundles
  and charts anonymously. The content is already public via the GitHub engine
  repo, and it must stay that way: **never put secrets or account-specific
  identifiers in platform-catalog/** (per-cluster values flow through q-core
  runtime inputs, not bundles). Overridable via `PLATFORM_CONFIG_REGISTRY`.
  The ECR repositories are declared in the infra Terraform,
  currently: `platform-config/{cluster-agent,shell-agent,loki}` and
  `charts/{loki,qovery-cluster-agent,qovery-shell-agent}` — with no lifecycle
  policy (retention rule: never delete a version a q-core release may still
  pin).
- **Tag immutability**: mutable-v0 for now — a version tag may be re-pushed and
  q-core pins/caches by **digest** only. The immutable-tag guard is kept
  commented in `scripts/publish-platform-config.sh`, ready to enable.
- **Version scheme**: monotonic per component (`v1`, `v2`, …) in `catalog.yaml`.
- **Granularity**: one bundle per component (aligned with the q-core
  `configRef {chart, version}` seam), not one global bundle.
- **Retention**: never delete a version a q-core release may still pin.
