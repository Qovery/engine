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
  templates/
    <template-key>/
      template.yaml            # root release composition; config pins are rendered at publish time
  components/
    <component>/
      config/
        static-values/
          base.yaml            # source 1 — Qovery values valid in every context
          overlays/            # source 2 — <mode>.yaml then <provider>.yaml
        runtime-values/        # source 3 — values requiring resolved runtime inputs
          managed-values.yaml  # direct whole-value mapping, or
          model.pkl            # optional Pkl evaluator entrypoint
          ...                  # focused evaluator/domain modules
        README.md              # component-specific reviewer and operations guide
```

File semantics (the contract q-core's loader implements):

- an absent `static-values/base.yaml` or context overlay is an empty fragment;
- a component without an evaluator must publish `runtime-values/managed-values.yaml`; an evaluator makes it optional;
- a **present-but-invalid** YAML file is a hard compilation error;
- merge order: `base < mode overlay < provider overlay < declarative mapping < evaluator-derived config`.

`runtime-values/managed-values.yaml` is the deliberately small source-3 format for components that
only map resolved runtime inputs to Helm paths. A placeholder must occupy the whole scalar value, for example
`CLUSTER_JWT_TOKEN: "${cluster.jwtToken}"`. Defaults, conditions, partial interpolation,
transformations, or any other logic require the component evaluator directly; there is no
intermediate template language. q-core loads and validates the mapping against the release
manifest's runtime-input declarations while warming the digest-pinned bundle, then renders it with
the same generic compiler used by the previous in-manifest `managedValues` representation.

An evaluator makes `runtime-values/managed-values.yaml` optional. When a component temporarily has
both, q-core merges the declarative mapping before the evaluator-derived values, so derived values win. Mixing
the two forms is discouraged: use the declarative mapping when there is nothing to calculate, and
the evaluator as soon as logic is required.

When a component declares a Pkl evaluator in the q-core release manifest,
`runtime-values/model.pkl` is the stable entrypoint for the executable source of truth for source 3.
Its focused relative imports stay inside the same digest-pinned bundle. q-core evaluates the module set
with Pkl 0.32 through the language-neutral JSON contract (`DESCRIBE`,
`RESOLVE_REQUIREMENTS`, `VALIDATE`, `COMPILE`). The model receives the request
through the external property `request` and returns JSON through `output.text`.
It can import only `.pkl` modules from its bundle through q-core's virtual `bundle:/` loader. It cannot
import arbitrary filesystem, package, or network modules, and it cannot read environment variables,
files, or network resources. Only the bundle modules, Pkl standard library, and `prop:request` are
enabled by q-core.

The Kotlin Loki deriver remains a test oracle during Slice 4; it is not a
production fallback. Once the bundle is pinned, an unavailable or invalid Pkl
model makes the affected catalog operation fail closed.

qovery-operator uses the same declarative runtime-input mapping as the execution-layer components,
but remains a bootstrap descriptor outside the Operator's own execution DAG. Its cluster JWT and
identity values are resolved by q-core in memory; the public bundle contains placeholders only.

cluster-agent and shell-agent keep an intentionally empty (comments-only)
`static-values/base.yaml` until their static Engine-v1 parity values land. Their per-cluster wiring
now lives in `runtime-values/managed-values.yaml`:
the bundle records the Helm paths and abstract placeholders, while q-core's release manifest records
the input providers (`qcoreValue`, customer value, Terraform output, and so on). The real JWT and
cluster identifiers never enter the public bundle; q-core resolves them in memory when compiling
`values.final.yaml`. A comments-only base file parses to YAML null, which the loader treats as an
empty fragment.

Loki keeps the provider-neutral `pvc` option and exposes an explicit self-managed object-storage
matrix: AWS/S3, GCP/GCS, Azure/Blob Storage, and Scaleway/S3-compatible. These are evaluator rules,
not provider overlays, because the selected fragment depends on both the product `storage` setting
and cluster provider. S3-compatible credentials stay in a customer-created Kubernetes Secret; only
its name is a runtime input. Qovery-managed object storage remains fail-closed until Terraform
outputs are available before Helm compilation.

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
GitLab job (manual — it publishes config bundles, then the chart mirror, then root templates;
set `PLATFORM_CONFIG_COMPONENTS` / `PLATFORM_CHARTS` / `PLATFORM_TEMPLATES` to a name list or `none`
to restrict), or locally:

```bash
PLATFORM_CONFIG_REGISTRY=<registry> ./scripts/publish-platform-catalog.sh
```

Each component's `config/` directory is pushed with ORAS as one OCI artifact
(`artifactType: application/vnd.qovery.platform-config.v1`) to:

```text
<registry>/platform-config/<component>:<version>   # version from catalog.yaml
```

The script writes `platform-config-publish.json` (component, version, ref,
digest) — the digest is the pin q-core records. `oras pull` of the reference
restores the exact `config/` directory content.

## Root template publication

Root releases are generic OCI artifacts, not Helm charts. The
`scripts/publish-platform-catalog.sh` script runs the existing bundle and chart
publishers, then consumes both machine-readable outputs from the earlier steps,
verifies every `configRef` and chart version, replaces
all config bundle pins in a temporary `template.yaml`, and only then publishes:

```text
<registry>/platform-templates/<template-key>:<release-version>
```

The artifact type is `application/vnd.qovery.platform-template.v1`; its only
payload is `template.yaml` with media type
`application/vnd.qovery.platform-template.layer.v1+yaml`. The script writes
`platform-templates-publish.json`, then renders the complete supported release
set and its default into a separate immutable snapshot:

```text
<registry>/platform-catalog/catalog:<catalog-version>
```

That artifact contains only `catalog.yaml`, with artifact type
`application/vnd.qovery.platform-catalog.v1` and layer media type
`application/vnd.qovery.platform-catalog.layer.v1+yaml`. Its tag defaults to
the commit SHA, while `platform-catalog-publish.json` reports the immutable
`canonicalRef` (`.../catalog@sha256:...`) consumed by q-core. The committed
template source may contain an explicit
`__PUBLISHED_CONFIG_DIGEST__` placeholder: it is never published directly, and
rendering fails unless every reference has a matching verified publication
output.

Publication order is an invariant: bundles first, charts second, root templates
third, and the complete catalog snapshot last. A partial selection is accepted
only when the resulting outputs still cover the complete root graph. ECR
repositories are infrastructure-owned and must include both
`platform-templates/<template-key>` and `platform-catalog/catalog` before the
first push.

On `main`, the manual GitLab job sends the emitted canonical reference to the
existing authenticated q-core service-version endpoint with service type
`PLATFORM_CATALOG` for dev and production. Merge-request jobs publish previews
without activating them. q-core validates and prewarms the complete graph before
updating `engine_version(name = 'platform-catalog')`; a rejected activation
leaves the previous database pointer and last-known-good in-memory snapshot
unchanged. The CI signature stays in protected variables and is never written
to publication output.

Keep every activated digest and all of its transitive bundle/chart content
available. Rollback selects a previous immutable catalog `canonicalRef` through
the same authenticated endpoint; it does not move a tag, republish artifacts,
or require a q-core rollout.

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
  currently: `platform-config/{qovery-operator,cluster-agent,shell-agent,loki}` and
  `charts/{qovery-operator,loki,qovery-cluster-agent,qovery-shell-agent}` — with no lifecycle
  policy (retention rule: never delete a version a q-core release may still
  pin).
- **Tag immutability**: mutable-v0 for now — a version tag may be re-pushed and
  q-core pins/caches by **digest** only. The immutable-tag guard is kept
  commented in `scripts/publish-platform-config.sh`, ready to enable.
- **Version scheme**: monotonic per component (`v1`, `v2`, …) in `catalog.yaml`.
- **Source 3 evaluator**: Pkl 0.32 is the first production implementation.
  Alternative languages are reconsidered after the Console vertical (MR-C),
  based on concrete authoring and operational feedback rather than a parallel
  pre-production bake-off.
- **Granularity**: one bundle per component (aligned with the q-core
  `configRef {chart, version}` seam), not one global bundle.
- **Retention**: never delete a version a q-core release may still pin.
