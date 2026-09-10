# Karpenter structured-fields exchange fixture v1

Unpublished A1 fixture, generated/evaluated with Pkl **0.32.0**. This directory is test data, not a
catalog component: it has no component manifest, chart or configRef, and cannot be selected by the
publisher's component inventory. The fixture version is `karpenter-v1`; `SHA256SUMS` identifies
all bundle modules and exchange JSON bytes for A2 handoff. Changing these bytes requires updating
the checksums and reviewing the golden exchanges.

`two-pools.profile.json` reproduces D11's `karpenter-configuration` fragment from
q-core `doc-features/engine-v2/roadmap/karpenter-catalog-contract-v0.md`, read at checkout HEAD
`1a20938464a95d1785e87557198383bb8ac7205d` on 2026-09-09. IDs are fictional.
Engine base: `bc6142c261eacfb605c08c4d26fb311018dd0541`.

| Files | Purpose |
| --- | --- |
| `two-pools.profile.json` | Accepted D11 intent, including different AMI/subnet modes, Spot, consolidation and taints |
| `DESCRIBE.{request,response}.json` | Prototype plus two independent evaluated row descriptions |
| `RESOLVE_REQUIREMENTS.{request,response}.json` | Same envelope; no infrastructure requirements are needed by this bounded collection fixture |
| `VALIDATE.{request,response}.json` | Valid two-pool draft, no violations, no Helm values |
| `COMPILE.{request,response}.json` | Valid two-pool projection under the existing `helmValues` property |
| `invalid-VALIDATE.{request,response}.json` | Invalid second-pool AMI ID, subnet ID and taint effect with indexed violations |
| `invalid-COMPILE.{request,response}.json` | Identical violations; `helmValues` is omitted |
| `config/runtime-values/model.pkl` | Existing-style JSON entrypoint, using only `prop:request` |
| `config/runtime-values/settings.pkl` | The component: one feature, the node pools |
| `config/runtime-values/nodePools/{setting,dependencies,helm}.pkl` | The pool shape as nested settings (row-dependent AMI and subnet members), the unique-name rule, the inspection projection |
| `config/runtime-values/{contract.pkl,sdk/}` | Copies managed exclusively by `sync-platform-pkl-sdk.sh` |

Required values (`nodePools`, a custom AMI's `id`) are reported at VALIDATE and COMPILE; like every
component, RESOLVE_REQUIREMENTS accepts an incomplete draft. An absent `taints` array is an empty
one, an absent `consolidation` object is one with its member defaults. The four valid exchanges are
byte-identical to the first capture. `invalid-VALIDATE` and
`invalid-COMPILE` were regenerated when the fixture moved to the SDK evaluator: same codes and paths,
messages in the SDK's uniform wording (they name the setting, never the indexed path: the path is in `fieldPath`).

For A2, copy **the complete `config/runtime-values/` directory** into the test bundle file store;
its entrypoint is `model.pkl`. Each `*.request.json` is the unchanged evaluation request envelope,
including the structured value under `profileConfig`. Each paired response is literal JSON output
from that entrypoint. Do not publish this fixture or treat its compiler projection as chart values.
The integration test copies this directory to a temporary root and evaluates with imports confined
to that root and resources confined to `prop:request`, proving it has no test-file runtime dependency.

```sh
./scripts/sync-platform-pkl-sdk.sh --check
./scripts/test-platform-config.sh
cargo test --manifest-path tools/platform-catalog-tests/Cargo.toml \
  --test module_layering --test runtime_models --test structured_fields --test publish_config
(cd platform-catalog/pkl/tests/fixtures/karpenter-v1 && shasum -a 256 -c SHA256SUMS)
```

`publish_config` is the existing local test using mock ORAS and `registry.invalid`; it does not
publish anything. The native `karpenter.tests.pkl` additionally exercises inactive drafts, missing
defaults, invalid types and collections, duplicate names, independent modes and reordering.

Compilation here is an inspection projection: it retains the selected input branches and applies
fixture defaults, not the future NodePool/EC2NodeClass chart mapping. The default tag value
`demo-eks` is a fixture constant. No logical input, legacy AMI version selection, cluster capability,
cloud compatibility, installation ownership or deployed-pool lifecycle guard is implied. A2 still
needs strict readers, domain/HTTP mapping, persistence round trips, error paths and sensitive
boundary checks; Console is outside A1.
