# Authoring a Helm platform component

This directory contains the Qovery-owned configuration layered on top of published Helm charts.
Each component is shipped as an independent, digest-pinned OCI config bundle and must use this
layout:

```text
components/<component>/config/
  static-values/
    base.yaml                  # source 1: Qovery values valid in every context
    overlays/                  # source 2: static mode/provider/capability fragments
      customer-managed.yaml
      aws.yaml
      qovery-karpenter.yaml
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
  < static-values/overlays/<capability>.yaml (enabled capabilities in enum declaration order)
  < runtime-values/managed-values.yaml
  < evaluator-produced values
```

Missing base values or overlays are empty fragments. Never put secrets, customer identifiers, or
account-specific values in this public directory.

q-core selects capability overlays from the target cluster's typed capabilities. Name each file
after the lower-kebab-case capability token; for example, `QOVERY_KARPENTER` selects
`qovery-karpenter.yaml`.
Every source uses recursive RFC 7386 merging: maps preserve unrelated nested keys, scalars and
lists replace earlier values, and `null` deletes an unprotected key.

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

## Structure a Pkl evaluator by setting

q-core invokes only the `runtime-values/model.pkl` entrypoint declared by `configRef.evaluator`.
Keep that file as an I/O shim: decode the request with `sdk/request.pkl`, hand the component to
`sdk/evaluate.pkl`, render JSON. The four operations (`DESCRIBE`, `RESOLVE_REQUIREMENTS`,
`VALIDATE`, `COMPILE`) are implemented once in the SDK; the component only declares data.

Organise that data **by setting**, not by operation. A reader looking for one customer-facing
setting must find everything about it in one folder, in files that always carry the same names:

```text
runtime-values/
  model.pkl                    # request decoding, SDK evaluation, JSON output only
  settings.pkl                 # table of contents: one Feature { settings; rules; fragment } per folder
  <setting>/
    setting.pkl                # WHAT it is: typed declaration (type, default, bounds, label, activity)
    dependencies.pkl           # WHAT IT REQUIRES from other settings or the cluster (rules)
    helm.pkl                   # WHICH chart values it owns (a fragment, merged with the others)
  <enum setting>/
    setting.pkl                # the choices and the availability rules (provider, mode)
    <choice>.pkl               # one file per choice: its inputs and its Helm hooks
    helm.pkl                   # assembles the fragment from the selected choice
  contract.pkl                 # vendored copy of the canonical q-core/Pkl contract
  sdk/                         # vendored copy of the shared authoring SDK
```

Conventions that keep this readable:

- `setting.pkl` is declarative. Type, default, bounds, allowed values: the SDK derives `DESCRIBE`,
  the applied default and the type/bound violations. Never re-implement a type check by hand.
- A file that would be empty is not created. A setting without rule has no `dependencies.pkl`.
- A dependency lives in the folder of the setting that is **blamed**, the `fieldPath` of its
  violation. A folder reads another setting through that setting's declaration, by importing the
  sibling's `setting.pkl` and nothing else (`storageSetting.selectedBackend(scope)`), never through
  a string key: renaming or retyping a setting then breaks evaluation instead of silently disabling
  a rule. The layering check refuses any other cross-folder import and any import cycle.
- What a choice requires from the cluster (its inputs, compile-only inputs, provider) is declared
  on that choice, in its own file when the enum has several substantial choices.
- Each `helm.pkl` writes only the chart keys its setting owns. Fragments are deep-merged in the
  feature order of `settings.pkl`, which is also the Console order and the key order of
  `values.final.yaml`; two settings writing the same leaf fail evaluation.
- Predicates and fragments receive one `Scope`: `scope.config` (resolved draft), `scope.cluster`,
  `scope.inputs`, `scope.enabledComponents`. Nothing else is in reach, by design.
- Use the SDK's composite declarations for concepts every chart shares. Resource budgets are one:
  `resources/setting.pkl` names the workloads and their preset budgets, and `sdk/resources.pkl`
  derives the selector, the CUSTOM fields, their rules and the `<target>.resources` values.

Loki is the reference bundle; its [README](loki/config/README.md) walks through each folder.
cluster-agent (one Helm fragment, no setting) and qovery-operator (one enum required under a
capability) show the minimal form. A structured draft (objects, lists of rows) uses the same
declarations one level down with `ObjectSetting` and `ArraySetting`; the karpenter fixture under
`platform-catalog/pkl/tests/fixtures/` is the reference for it.

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
- Keep constraints on the setting or logical input they constrain (`setting.pkl`, a backend file).
  Keep cross-setting rules in the `dependencies.pkl` of the setting that is blamed.
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
