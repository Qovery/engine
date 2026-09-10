# Pkl authoring SDK

Single source of the component-agnostic Pkl modules shared by every executable platform
configuration bundle:

```text
pkl/
  contract.pkl       # canonical evaluator response contract (operations, Field, Violation, ...)
  sdk/
    request.pkl      # prop:request decoding, typed request readers, test request builder
    validate.pkl     # canonical violation codes, constructors, accumulation, generic validators
    result.pkl       # EvaluationResult envelope with the fail-closed COMPILE gate
    settings.pkl     # typed setting declarations: descriptor, default, type and bound checks
    rules.pkl        # dependency rules between settings, or between a setting and the cluster
    helm.pkl         # per-setting Helm fragments, deep-merged with conflict detection
    resources.pkl    # composite declaration: workload targets + preset budgets -> fields, rules, Helm
    evaluate.pkl     # the generic evaluator: the four operations over a declared component
  tests/             # native Pkl tests of the SDK primitives (never published)
```

This directory deliberately mirrors a bundle's `runtime-values/` root: `sdk/request.pkl` imports
`../contract.pkl` and resolves it here during authoring and inside `runtime-values/` at runtime,
so the vendored copies stay byte-identical to the source.

## The setting-centric evaluator

`settings.pkl`, `rules.pkl`, `helm.pkl`, `resources.pkl` and `evaluate.pkl` let a component be
**data**: a `Component` is a label and an ordered list of `Feature`s, each contributing settings,
dependency rules and one Helm fragment (one feature per setting folder). A feature spells out its
three members; Pkl refuses an omitted one (`rules = List()` and `fragment = sdkHelm.none` are
written, never assumed). `evaluate.evaluate(component,
request)` implements the four operations once:

| Declaration | Where it lives in a bundle | What the evaluator derives |
| --- | --- | --- |
| `IntSetting`, `BoolSetting`, `StringSetting`, `EnumSetting` | `<setting>/setting.pkl` | `Field` descriptor, applied default, `INVALID_TYPE`, `VALUE_OUT_OF_RANGE`, `LENGTH_OUT_OF_RANGE`, `VALUE_NOT_ALLOWED`, activity (`activeWhen`), conditional requirement (`requiredWhen`) |
| `Option` (value, provider, inputs, compileInputs) and `Availability` | `<enum setting>/setting.pkl`, or one file per option | Narrowed `allowedValues` with a cluster context, `RESOLVE_REQUIREMENTS`, `REQUIRED_INPUT_MISSING` and `INPUT_PATTERN_MISMATCH` at the right phase, the availability violation of a selected unavailable option |
| `Rule` | `<blamed setting>/dependencies.pkl` | One violation per violated rule, on the operations it declares |
| `Fragment` | `<setting>/helm.pkl` | `helmValues`, deep-merged in declaration order; a leaf written by two fragments throws |
| `ResourceProfiles` (targets, preset budgets, recommended preset) | `resources/setting.pkl` | The profile selector, four CUSTOM fields per active target pre-filled with the recommendation, `REQUIRED_RESOURCE_REQUEST_MISSING`, `LIMIT_BELOW_REQUEST`, and the `<target>.resources` blocks |
| `ObjectSetting`, `ArraySetting` (nested settings) | `<setting>/setting.pkl` | `ObjectField` / `ArrayField` descriptors with the new-row prototype and one descriptor list per current row, defaults at every level, `INVALID_TYPE` / `UNKNOWN_FIELD` / `LENGTH_OUT_OF_RANGE` / `DUPLICATE_ITEM` on indexed paths, and the active-value projection |

Every predicate, contextual description and Helm fragment receives one `Scope`: `config` (the
resolved configuration: every declared key present, absent values at their default, ill-typed values
`null` and reported separately), `cluster` (mode, provider, capabilities), `inputs` (the cluster
inputs) and `enabledComponents`. A rule reads another setting through that setting's declaration
(`storageSetting.selectedBackend(scope)`, `highAvailabilitySetting.enabled(scope)`), never through a
string key; the declaration classes expose `isOn`, `value` and `selectedValue` for that. Violation order is stable: unknown fields, then each active
setting in Console order (its own value, the availability of its selected option, the inputs that
option activates), then the rules. The feature order is also the merge order of the fragments,
hence the key order of the compiled values.

Loki is the reference bundle for this layout (see its [README](../components/loki/config/README.md));
cluster-agent, qovery-operator and the karpenter fixture use it too. Structured drafts use the same
declarations one level down: an `ObjectSetting` groups members, an `ArraySetting` repeats one item
template (a scalar setting or an `ObjectSetting` row). Nested members follow their own `activeWhen`,
so a row's shape depends on the row's values, and violations carry indexed paths
(`nodePools[1].ami.id`). Helm fragments receive the active configuration (inactive settings and
members dropped, defaults applied), which is what a compiler reads.

What a declaration means for a draft, in one table:

| Draft | DESCRIBE / contextual descriptors | VALIDATE and COMPILE | Compiled value |
| --- | --- | --- | --- |
| Scalar absent | `defaultValue` shown | nothing, unless `required` / `requiredWhen` (VALIDATE and COMPILE only) | the default, or absent |
| Scalar explicit `null`, or wrong type | unchanged | `INVALID_TYPE` | never compiled (gate) |
| Value outside bounds / choices | unchanged | `VALUE_OUT_OF_RANGE`, `LENGTH_OUT_OF_RANGE`, `VALUE_NOT_ALLOWED`, on every contextual operation | never compiled |
| Object absent | members with their defaults | required members still due | `{}` with member defaults |
| Array absent | empty `itemFields` | size bounds apply (`LENGTH_OUT_OF_RANGE` on `minItems`) | `[]` |
| Row of the wrong shape | the prototype at that index | `INVALID_TYPE` at `key[i]` | never compiled |
| Inactive member (`activeWhen` false) | not listed | not validated; the unknown-key check still knows it | dropped; the stored draft keeps it |
| Unknown key, at any level | unchanged | `UNKNOWN_FIELD` on the parent path | never compiled |

## What belongs here

Only primitives that are true for **every** component: the operation contract, request access,
canonical violation codes and generic validators (required, enum, integer bounds, string length,
input presence and pattern), violation accumulation, and the envelope constructor that encodes
"COMPILE returns no helmValues when violations exist". Component business rules, chart-specific
compilation, provider abstractions, and component vocabularies (`context.pkl`) must stay in the
component bundle — the layering check (`tools/platform-catalog-tests/tests/module_layering.rs`)
rejects an SDK module that imports anything but `contract.pkl` or another `sdk/` module.

## Vendoring workflow

q-core resolves Pkl imports only inside one digest-pinned OCI bundle, so each executable component
carries a byte-identical copy of `contract.pkl` and `sdk/` under `config/runtime-values/`. Those
copies are machine-managed, never hand-edited:

1. edit the canonical files here, then run `./scripts/sync-platform-pkl-sdk.sh` and commit the
   synchronized copies together with the change;
2. `./scripts/test-platform-config.sh` (and CI) runs the sync in `--check` mode and fails on a
   missing, stale, or extraneous vendored file;
3. `./scripts/publish-platform-config.sh` refuses to publish while copies are out of sync, and
   injects the canonical files into its staging directory so the published layer is always exact.

## Tests

`tests/` covers the SDK primitives natively; `./scripts/test-platform-config.sh` runs them with the
component suites. Component tests import their own vendored copy
(`../config/runtime-values/sdk/...`), which keeps them honest about the bytes actually published.

## Structured configuration fields (A1, unpublished)

`Field` and `Constraints` retain their scalar types, property order and string-encoded defaults.
`LogicalInput.type` remains `string | number | bool`. The only shared response signature change
is `EvaluationResult.fields` / `result.envelope` accepting `ConfigurationField`: the existing
`Field`, an `ObjectField`, or an `ArrayField`. The four operations and lazy compilation gate are
unchanged. No structured descriptor is emitted by the published component models in this change.

| Variant | Additional wire properties | Meaning |
| --- | --- | --- |
| `Field` | None | Existing scalar JSON, including `defaultValue: "false"` |
| `ObjectField` (`type: "object"`) | `fields` | Ordered relative child descriptors |
| `ArrayField` (`type: "array"`) | `items`, optional `itemFields`, collection `constraints` | Ordered JSON collection |
| `ScalarItem` | Scalar `type`, scalar `constraints` | Array element value, without a field key or label |
| `ObjectItem` | `type: "object"`, `fields` | Prototype for a new object row |

Collection constraints are a separate type: `minItems`, `maxItems`, `uniqueItems`. Bounds are
nonnegative and `maxItems >= minItems` when both are set. They never appear in scalar constraints.
Container classes cannot carry contradictory properties such as `ObjectField.items` or scalar
collection constraints. Nested object keys are unique literal names matching
`[A-Za-z_][A-Za-z0-9_-]*`; existing top-level scalar keys (including dotted names) are unchanged.
Direct arrays of arrays and structured defaults are outside this extension. Optional empty arrays
need no default payload; in the fixture, absent `taints` compiles as `[]`. Scalar defaults apply
only to absent values; explicit `false`, `[]`, `null`, and blank strings are not replaced by defaults.
Explicit null or a wrong type on an active field produces a violation.

### Prototype, current rows and errors

For object arrays, `items.fields` describes a **new row using the component's defaults**.
`itemFields` is mandatory and is a JSON array of complete relative field lists, one for each
current draft element, in exactly the input order. An absent, empty or mistyped array has
`itemFields: []`; a mistyped element still occupies its index and gets a prototype description.
For scalar arrays `itemFields` must be null in Pkl and is omitted by the existing JSON renderer.
Only the component can check correspondence to the draft; the fixture tests pin length and order.

The prototype is not a shared condition for existing rows. With two pools, for example, only the
custom-AMI row includes the required `id` descriptor. Switching a mode recomputes that row's fields
through the existing operation, while the submitted configuration retains inactive drafts. Fields
absent from the evaluated description are inactive, not instructions to delete stored values.
Consumers should use `items` to start a new row, then request a fresh preview, and use
`itemFields[index]` to render existing rows. Descriptors carry defaults, not copies of saved values.

Child keys stay relative (`ami`, then `id`). `validate.fieldPath` and `validate.indexPath` compose
`nodePools[1].ami.id`; `validate.atPath` prefixes local violations while preserving codes/messages.
Indices refer to this request only. A consumer must match a response to its draft revision before
showing indexed errors after reorder. All mutations must preserve the complete collection.

### SDK responsibility boundaries

| Change | Existing responsibility extended |
| --- | --- |
| `contract.pkl` | Closed response vocabulary, structured shapes and descriptor invariants |
| `sdk/request.pkl`: `objectValue`, `arrayValue` | Optional views of decoded JSON containers, without rewriting the draft |
| `sdk/validate.pkl`: paths, prefixing, size, uniqueness | Generic diagnostics using the existing violation envelope |
| `sdk/result.pkl` | Broader field parameter only; compiler supplier and fail-closed gate unchanged |
| `sdk/settings.pkl`: `ObjectSetting`, `ArraySetting`, `activeValue` | Nested declarations rendered, resolved and validated by the same evaluator as scalars |
| `tests/fixtures/karpenter-v1/config/runtime-values` | Declarations only: `nodePools/setting.pkl` (the pool shape), `nodePools/dependencies.pkl` (unique names), `nodePools/helm.pkl` (the inspection projection) |

The fixture is a component like any other: `model.pkl` hands `settings.pkl` to `sdk/evaluate.pkl`.
It is an autonomous bundle with relative imports. Its four valid exchanges are unchanged by the
setting-centric rewrite; the two invalid exchanges were regenerated because violation messages now
follow the SDK's uniform wording (`Karpenter id does not match the expected format`; a message
names the setting, never the indexed path), codes and paths being identical. `sync-platform-pkl-sdk.sh` also manages its contract and
SDK copies; `module_layering` checks it alongside production bundles. The publisher is unchanged,
and test fixtures cannot satisfy its existing check that executable components exist. SDK modules
still import only the canonical contract, siblings or the already-allowed Pkl standard library.

### Sensitive descendants and rollout gate

Every nested descriptor must have `sensitive = false`, including descendants of a top-level
sensitive container, `items.fields`, and every evaluated `itemFields` row. This conservative A1
restriction is enforced by the Pkl contract; it does not promise recursive secret redaction.
Top-level scalar sensitivity retains its current behavior. Karpenter fixtures contain no secrets.
A2 must reject disallowed sensitive descendants recursively at the consumer boundary, and cover
preview, persistence/redaction and HTTP mapping before enabling structured bundles. Any later
relaxation needs explicit recursive redaction tests; marking a parent sensitive does not waive the
current descendant restriction.

The current strict q-core reader rejects the new types/properties. A2 must add discriminated
platform descriptor/constraint readers and recursive response mapping without widening scalar
logical inputs or Service Catalog types. Console collection editing is a later step. A1 proves
local Pkl exchanges and compatibility of existing scalar outputs, not a q-core save/reload cycle,
Console rendering, Karpenter chart rendering, AWS checks or Kubernetes deployment.

The reusable [karpenter-v1 fixtures](tests/fixtures/karpenter-v1/README.md) identify the six exchange
pairs, isolated bundle root and validation commands. The 16 [scalar-v1 baseline exchanges](tests/fixtures/scalar-v1/exchanges.json)
were captured from unmodified Engine `bc6142c261eacfb605c08c4d26fb311018dd0541` before this extension;
they cover all four operations for cluster-agent, qovery-operator, Loki defaults and Loki AWS S3.
