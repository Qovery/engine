# Authoring a Helm platform component

This directory contains the Qovery-owned configuration layered on top of published Helm charts.
Each component is shipped as an independent, digest-pinned OCI config bundle and must use this
layout:

```text
components/<component>/config/
  static-values/
    base.yaml                  # source 1: Qovery values valid in every context
    overlays/                  # source 2: static mode/provider fragments
      customer-managed.yaml
      aws.yaml
  runtime-values/              # source 3: values that require resolved runtime inputs
    managed-values.yaml        # simple whole-value mapping, or
    model.pkl                  # evaluator entrypoint when logic is required
    ...                        # focused evaluator modules
  README.md                    # component-specific behavior and operational boundaries
```

`static-values/` and `runtime-values/` are semantic names, not execution stages. The exact merge
order remains:

```text
chart defaults
  < static-values/base.yaml
  < static-values/overlays/<mode>.yaml
  < static-values/overlays/<provider>.yaml
  < runtime-values/managed-values.yaml
  < evaluator-produced values
```

Missing base values or overlays are empty fragments. Never put secrets, customer identifiers, or
account-specific values in this public directory.

## Choose the smallest source 3

Use `runtime-values/managed-values.yaml` when every dynamic Helm leaf is a direct mapping from one
declared runtime input:

```yaml
environmentVariables:
  CLUSTER_JWT_TOKEN: "${cluster.jwtToken}"
```

The placeholder must occupy the whole scalar. Defaults, conditions, partial interpolation,
transformations, conditional inputs, or cross-field validation require a Pkl evaluator directly.
Do not grow `managed-values.yaml` into a template language.

An evaluator makes the declarative mapping optional. Both forms may coexist when the declarative
mapping owns independent direct leaves and the evaluator adds a disjoint conditional fragment;
document that boundary in the component README. Evaluator values win on overlap, which should stay
a migration mechanism rather than the normal design.

## Structure a Pkl evaluator by user action

q-core invokes only the `runtime-values/model.pkl` entrypoint declared by `configRef.evaluator`.
Keep that file as a router for the four language-neutral operations:

| Operation | User/system action | Recommended module |
| --- | --- | --- |
| `DESCRIBE` | Render editable Console fields | `describe.pkl` |
| `RESOLVE_REQUIREMENTS` | Refresh conditional inputs for the current draft | `requirements.pkl` |
| `VALIDATE` | Check the draft before saving | `validate.pkl` |
| `COMPILE` | Produce derived Helm values before deployment | `compile.pkl` |

Shared product concepts may use a domain subdirectory. For example Loki keeps storage types,
logical inputs and backend instances in `storage/types.pkl`, `storage/inputs.pkl`, and
`storage/backends.pkl`. Prefer a domain name over generic directories such as `constraints/` or
`utils/`: a logical input's key, type, label, description and constraint should stay together.

Recommended evaluator tree:

```text
runtime-values/
  model.pkl                    # request decoding, operation routing, JSON output only
  contract.pkl                 # vendored copy of the canonical q-core/Pkl contract
  describe.pkl                 # fields, defaults and field-level constraints
  requirements.pkl             # conditional logical inputs
  validate.pkl                 # types, cross-field rules and stable violation codes
  compile.pkl                  # readable high-level Helm topology
  profile.pkl                  # defaults and safe profile type narrowing
  <domain>/
    types.pkl                  # domain types
    inputs.pkl                 # logical inputs with their constraints
    backends.pkl               # supported instances/capability registry
    helm.pkl                   # domain-specific adaptation to chart values, when needed
```

The canonical contract lives in `platform-catalog/pkl/component-contract.pkl`. Every executable
component keeps a local copy because q-core resolves imports only within that component's
digest-pinned bundle. Run `./scripts/sync-platform-pkl-contract.sh` after changing the canonical
file; CI rejects missing or stale copies, and publication injects the canonical file into the
staged bundle. Component-specific types must stay outside `contract.pkl`.

Split chart-specific low-level adaptation from product intent when it becomes substantial. Loki's
`storage/helm.pkl` is such an adapter; `compile.pkl` remains readable without knowing every Loki
chart key.

## Evaluator invariants

- Return stable machine-readable violation codes; the Console must not parse error messages.
- `DESCRIBE` exposes fields and their constraints without requiring runtime inputs.
- `RESOLVE_REQUIREMENTS` returns only inputs activated by the current draft and cluster context.
- The logical input names must be declared in the root template's `runtimeInputs`.
- q-core decides which input provider supplies a logical input. Pkl describes what the component
  needs, not whether it comes from the customer, q-core, or Terraform.
- `VALIDATE` and `COMPILE` apply the same product rules. `COMPILE` additionally checks trusted
  system inputs needed only while producing values.
- Invalid input must never produce partial Helm values. Compilation fails closed.
- Keep constraints beside the field or logical input they constrain. Keep cross-field rules in
  `validate.pkl`.
- Pkl imports are static. More files improve navigation, not lazy execution.
- Imports must stay within the same digest-pinned bundle; filesystem, environment, packages and
  network access are unavailable.

## Adding a component

1. Freeze or mirror the Helm chart and add it to `platform-catalog/catalog.yaml`.
2. Create `static-values/base.yaml`; a comments-only file is allowed when no Qovery static value
   exists yet.
3. Add only genuinely static mode/provider fragments under `static-values/overlays/`.
4. Choose `runtime-values/managed-values.yaml` or a Pkl evaluator using the rule above.
5. Declare every runtime input and its input providers in the root template.
6. Document lifecycle restrictions, secret handling and unsupported cases in the component README.
7. Add contract fixtures for every operation and important provider/mode combination.
8. Publish the component and activate a catalog snapshot containing its verified digest. During
   mutable-v0 the version tag may stay unchanged; once tags become immutable, bump the component
   version and update every root-template reference.

If the component exposes no configurable field, there is no mutation policy to declare yet: keep
`configSchema` empty and document that boundary. As soon as a field is added, decide its mutation
policy while authoring it rather than retrofitting the decision after customer exposure.

Cross-layer dependencies belong to the root template's component descriptor. Use `requires` when
the dependency must be enabled with the component; it also orders the dependency first. Use `after`
when ordering is required only if both components are enabled. Keep dependencies between components
in the same layer: they document runtime requirements and protect future component-level selection.
New optional layers must start with `enabledByDefault: false`.

For Pkl components, run from the Engine repository root:

```shell
PKL_BIN=pkl ./scripts/test-platform-config.sh
pkl format --diff-name-only platform-catalog/components/<component>/config/runtime-values
```

The component-specific [Loki guide](loki/config/README.md) is the reference implementation.
