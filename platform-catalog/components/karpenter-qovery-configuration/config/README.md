# Qovery Karpenter pools — V0

This Source 2 bundle configures the existing `karpenter-configuration` Helm chart (1.0.1).
It creates the Qovery `default` and `stable` NodePools sharing EC2NodeClass `default`.
The optional `karpenter` layer is disabled by default and groups CRDs, controller, Qovery
pools and customer pools. Qovery pools keep the legacy release
`kube-system/karpenter-configuration`; customer pools keep the distinct release
`kube-system/karpenter-custom-configuration`. Both pool components require the controller and
CRDs. Controller and Qovery configuration remain required when the layer is enabled.

Customer `nodePools` may be absent or empty: the custom model then compiles `pools: []`,
renders no resources and requests no AWS inputs. A nonempty list activates the existing
per-pool validation and AWS requirements. Empty desired state removes resources already
owned by the custom Helm release; it does not preserve existing customer pools.
Per-component activation is future work, not implemented in this POC.

## Source 2 configuration

| Field | Contract |
| --- | --- |
| `instanceRequirements.architectures` | Shared nonempty unique list: `amd64`, `arm64` |
| `instanceRequirements.families` | Shared nonempty unique instance-family list |
| `instanceRequirements.sizes` | Shared nonempty unique instance-size list |
| `diskSizeGiB` | Shared integer disk size, at least 20 GiB; UI preset 50 GiB |
| `default.spotEnabled` | Boolean, UI preset false |
| `stable.spotEnabled` | Boolean, UI preset false |

Lists are explicit: the legacy Console derives its choices from the cloud instance catalogue,
so this model does not invent a universal family/size preset. Formats are validated locally;
regional availability and compatibility between the selected families, sizes and architectures
must be checked against AWS before deployment. Spot true allows both `spot` and `on-demand`;
false allows only `on-demand`. Field defaults inform the form; required fields must be present
in a saved configuration.

The legacy chart retains names, weights (50/10), stable taint `nodepool/stable:NoSchedule`,
720h expiration, and the shared class. This bundle sets stable consolidation to `WhenEmpty`,
with a 30s base delay and only the common 10% budget. It deliberately does not reproduce
q-core's scheduled underutilized budgets. Default consolidation remains unchanged.
The chart's own stable default remains `WhenEmptyOrUnderutilized`, preserving legacy callers.

## Logical inputs

The resolver asks for existing `aws.eksClusterName`, `aws.nodeRoleName`,
`aws.nodeSecurityGroupId`, and a verified `aws.amiSelectorTermsAlias` (AL2, AL2023 or
Bottlerocket alias). The node role is explicit; the chart's empty override still falls back
to `KarpenterNodeRole-<clusterName>` for legacy callers. An AMI alias is not inferred from a
synthetic version or ID. Bottlerocket aliases select the existing two-volume chart layout.
The model provisions no AWS infrastructure, discovers no resources, and grants no access.

q-core supplies `cluster.id`, `cluster.organizationId`, and `cluster.region` at COMPILE.
These are declared runtime inputs and checked only at compilation; they do not become
customer form fields. The model builds the existing Qovery system tags from them.
`aws-apn-id` uses the legacy unset fallback `not-set` in static values because the current
q-core runtime context has no APN value. A known existing APN tag requires a reviewed overlay;
it must not be silently replaced during a migration.

## Existing release: migration is outside this V0

This bundle is **not an importer or a safe automatic takeover of an arbitrary legacy release**.
Keeping the release name preserves its identity; it does not preserve omitted Helm values.
Neither q-core's binding nor this model reads the installed release's effective values.
Before activating it on an existing release, review the rendered manifests against the
existing release and live resource inventory. Do not activate it if settings or resources
cannot be represented and preserved. No automatic restoration, adoption or deletion is provided.

For a separately reviewed catalogue bundle, existing static/context overlays can preserve
non-exposed chart values. The established merge is chart defaults, base/context overlays,
declarative values, then compiled model values; maps merge and lists replace. This is not a
new arbitrary Helm override field in the cluster binding or a migration action in the Console.
The regression fixture demonstrates preservation when a verified overlay is explicitly
supplied; the published generic bundle does not supply that cluster-specific overlay.

Review disk IOPS/throughput, storage class, termination grace, limits, subnet selectors,
AMI settings, custom/APN tags and consolidation delays. The intentional stable policy/budget
change also requires review. Additional GPU, cronjob, public/private or renamed pools and
custom AMIs are outside this POC: do not upgrade a release containing them with the generic
bundle. Such a release needs a separate supported migration that retains every owned resource.
Do not use the customer pool release to recreate or adopt `default`/`stable`.

## Implementation and checks

`model.pkl` delegates to the canonical SDK. `settings.pkl` assembles the `pools` feature,
whose setting descriptors, rules, logical inputs and Helm fragment remain together.
The vendored contract and SDK are byte-identical to the canonical copies. There is no
new q-core/OpenAPI/Blueprint contract, API exception or cluster access in this change.

`../tests/legacy-effective.values.yaml` and
`../../../examples/karpenter-v0/qovery-configuration.request.json` are synthetic fixtures.
Tests evaluate the real isolated entrypoint and render the real chart: compare complete
legacy/new manifests, verify the deliberate stable delta, independent Spot, shared class,
logical inputs, validation gates and optional layer/release/dependency wiring.

```sh
./scripts/sync-platform-pkl-sdk.sh --check
pkl test platform-catalog/components/karpenter-qovery-configuration/tests/evaluation.tests.pkl
cargo test --manifest-path tools/platform-catalog-tests/Cargo.toml --test karpenter_models --test render_template --test module_layering
cargo fmt --manifest-path tools/platform-catalog-tests/Cargo.toml --check
cargo clippy --manifest-path tools/platform-catalog-tests/Cargo.toml --all-targets -- -D warnings
```

Publication requires the OCI repositories `platform-config/karpenter-qovery-configuration`
and `charts/karpenter-configuration`, then the existing catalogue publication pipeline to
publish the config bundle, frozen chart and root template with verified digests. Local tests
and a merged commit do not establish that those artifacts are published. Deployment remains
a separate explicit step after reviewing the target cluster's resources and prerequisites.
