# Loki product configuration model

```text
loki/
  config/
    static-values/
      base.yaml
      overlays/
        qovery-karpenter.yaml   # Source 2 capability overlay selected by q-core
    runtime-values/
      model.pkl                 # the only entrypoint q-core calls: decode, evaluate, render JSON
      settings.pkl              # table of contents: one feature per setting folder, in Console order
      retentionWeeks/
        setting.pkl             # what it is: Int, default 12, 1..52
        helm.pkl                # what it owns: loki.limits_config.retention_period
      highAvailability/
        setting.pkl             # Bool, default false
        dependencies.pkl        # needs object storage
        helm.pkl                # the topology: deploymentMode, replicas, replication factor, gateway
      storage/
        setting.pkl             # Enum over the backends below, with the availability rules
        backend.pkl             # template every backend file amends
        pvc.pkl                 # one file per choice: the option, its cluster inputs, its Helm hooks
        s3.pkl
        gcs.pkl
        azureBlob.pkl
        s3Compatible.pkl
        patterns.pkl            # input formats shared by several backends
        helm.pkl                # loki.storage, schema, identity wiring, persistence of every workload
      resources/
        setting.pkl             # the workloads and the preset budget table; the SDK derives the rest
      contract.pkl              # vendored contract (machine-synced, see ../../../pkl/README.md)
      sdk/                      # vendored SDK, including the generic evaluator
  tests/
    fixtures.pkl                # contexts, drafts and readers shared by the suites
    evaluation.tests.pkl        # the four operations end to end
    retentionWeeks.tests.pkl    # one suite per setting folder
    highAvailability.tests.pkl
    storage.tests.pkl
    resources.tests.pkl
    compile-golden.tests.pkl    # golden COMPILE outputs, one per storage backend
    compile-golden.tests.pkl-expected.pcf
```

## How to read a setting

Every folder answers the same three questions, in files that always carry the same names:

| File | Question | What the evaluator derives from it |
| --- | --- | --- |
| `setting.pkl` | What is it? | The Console descriptor (`DESCRIBE`), the default, `INVALID_TYPE`, `VALUE_OUT_OF_RANGE`, `VALUE_NOT_ALLOWED`, which settings are active for the current draft |
| `dependencies.pkl` | What does it require from other settings or the cluster? | One violation per rule, with its stable code, on the path of the setting that is blamed |
| `helm.pkl` | Which chart values does it own? | Its fragment of `helmValues`, deep-merged with the others; two fragments writing the same leaf value is an evaluation error |

A file that would be empty is not created: `retentionWeeks` depends on nothing, and `storage` has
no `dependencies.pkl` because what it requires from the cluster is attached to each backend choice
and its availability rules are declared on the enum itself (below).

`settings.pkl` lists one `Feature { settings; rules; fragment }` per folder. The three members have
no implicit default: a folder without rule writes `rules = List()`, one without Helm values writes
`fragment = sdkHelm.none`, and an omitted member fails evaluation. The
feature order is the Console order and the merge order of the Helm fragments, hence the key order
of `values.final.yaml`.

An enum setting whose choices carry their own inputs and Helm wiring, like `storage`, is split one
level further: **one file per choice**, amending `backend.pkl`. Reading `s3.pkl` tells you that
choosing S3 is only offered on AWS, asks the customer for a bucket and an IAM role, needs the region
from q-core at compile time, and how the chart authenticates through IRSA. Adding a backend is
adding a file and listing it in `storage/setting.pkl`.

The rules that decide whether a choice is *offered* on a cluster (provider match, no object storage
on Qovery-managed clusters) are declared once on the enum setting as `availability` rules: they
narrow the Console list, gate the choice's inputs, and report their own violation when the selected
choice fails them. An unavailable choice activates no input, whatever the reason.

Resource profiles are a **composite declaration** provided by the SDK (`sdk/resources.pkl`): every
chart has `resources` blocks, so the selector, the per-workload CUSTOM fields, the "limit at least
request" rules and the `<target>.resources` values are derived once for every component. Loki only
declares its five workloads, which of them run in each topology, and the SMALL/MEDIUM/LARGE budget
of each, in `resources/setting.pkl`. The MEDIUM budgets pre-fill the CUSTOM fields without ever
being applied: a request left empty under CUSTOM is reported, never silently filled.

## Where a rule lives

A dependency lives in the folder of the setting **that is blamed**, that is the `fieldPath` of the
violation it reports. "High availability requires object storage" is reported on
`highAvailability`, so it lives in `highAvailability/dependencies.pkl`, even though it reads the
storage choice. It reads it through the storage declaration, `storageSetting.selectedBackend(scope)`,
not through a string key: a folder may import a sibling's `setting.pkl` and nothing else, and the
layering check refuses import cycles. Inside its own folder a setting is read the same way
(`setting.isOn(scope)`, `setting.value(scope)`); `scope.config` is the resolved draft behind those
accessors, with defaults applied and ill-typed values nulled.

## Pkl syntax used here

| Syntax | Meaning in this model |
| --- | --- |
| `import "../sdk/settings.pkl" as sdkSettings` | Load a module from the same OCI bundle. Cross-folder imports carry the `<folder><Module>` alias, checked by the layering test. |
| `amends "backend.pkl"` | A backend file *is* a filled-in copy of the template: same properties, no new public ones. `local` properties may be added. |
| `local` | Private implementation detail, omitted from rendered output. |
| `new sdkSettings.IntSetting { ... }` | Construct a checked SDK object. Misspelled properties fail evaluation. |
| `(scope) -> setting.enabled(scope) && !backend.isObjectStorage` | A predicate over the evaluation scope: `scope.cluster` (mode, provider, capabilities), `scope.inputs`, `scope.enabledComponents`, and the settings read through their declarations (`setting.isOn(scope)`, `storageSetting.selectedBackend(scope)`). |
| `List<Target>(!isEmpty, ...)` | A type constraint: the declaration fails at evaluation when the table is inconsistent. |
| `const function` | A module function that class bodies may call (Pkl requirement for helpers used from inside a class). |
| `new Mapping { ["key"] = value }` | Build dynamic Helm/JSON key-value data. |
| `when (condition) { ... }` | Add mapping entries only when the condition is true. |
| `value ?? fallback` | Use `fallback` when `value` is null. |
| `x as Int` | Narrow a validated value; COMPILE only runs on a validated draft, so the cast is an invariant, not a check. |

## Console use cases

`model.pkl` is the only entrypoint called by q-core. The four operations are implemented once, in
`sdk/evaluate.pkl`; the component only supplies data.

| Console use case | Operation | What the evaluator does with the Loki declarations |
| --- | --- | --- |
| List the parameters a customer can change | `DESCRIBE` | Renders every `setting.pkl` in the order of `settings.pkl`; with a cluster context, narrows the storage choices to the available ones |
| Refresh conditional inputs after a change | `RESOLVE_REQUIREMENTS` | Renders the active settings, returns the inputs of the selected available backend, applies the rules that hold on an incomplete draft |
| Check constraints before saving | `VALIDATE` | Same, plus required values, input presence and formats |
| Build the final Helm values | `COMPILE` | Same, plus the q-core compile-only inputs; then merges the `helm.pkl` fragments in the order of `settings.pkl` |

q-core sends the request as JSON through `prop:request` and receives JSON from `model.pkl`'s
`output`. Pkl syntax and errors never cross the backend API boundary. Pkl does **not** define which
layers exist or which components are in a layer: the root template is the catalog composition
source of truth. Pkl describes and compiles the configuration of one component after q-core has
found that component in the template.

## Self-managed storage matrix

`pvc` is provider-neutral and keeps the chart's persistent-volume configuration. Object storage is
provider-specific and disables Loki data PVCs; local `/var/loki` state uses `emptyDir`.

| Cluster provider | `storage` | Runtime inputs | Kubernetes identity |
| --- | --- | --- | --- |
| AWS | `s3` | bucket; IAM role ARN; q-core region | EKS IRSA annotation |
| GCP | `gcs` | bucket; GCP service-account email | GKE Workload Identity annotation |
| Azure | `azureBlob` | account; container; managed-identity client ID | Azure Workload Identity label + annotation |
| Scaleway | `s3Compatible` | bucket; endpoint; region; credentials Secret name | pre-created Kubernetes Secret |

Each row is one file under `storage/`. The evaluator rejects a storage/provider mismatch. High
availability is valid with any of the four object-storage values and invalid with `pvc`.

The unscoped catalog remains a capability index and therefore lists all five storage values. A
contextual catalog read and every cluster preview narrow the `storage` field to the effective pair:
AWS=`pvc|s3`, GCP=`pvc|gcs`, Azure=`pvc|azureBlob`, Scaleway=`pvc|s3Compatible`. Qovery-managed
clusters currently expose only `pvc`. The Console renders the returned `allowedValues`; it does not
encode this matrix or any Loki-specific provider condition.

The Loki workload image uses the Qovery `pub-mirror-loki` Public ECR repository and is pinned by
manifest digest. The gateway's upstream `nginx-unprivileged` image is also digest-pinned; it remains
on Docker Hub until the corresponding Qovery mirror repository exists. The gateway uses
`qovery-high-priority`, and the root template requires the PriorityClass component before Loki.

When q-core resolves the typed `QOVERY_KARPENTER` cluster capability, Source 2 loads
`static-values/overlays/qovery-karpenter.yaml`. The overlay applies Qovery's stable-nodepool
affinity and toleration policy to `singleBinary`, `write`, `read`, `backend`, and the gateway.
Self-managed Karpenter installations never select this Qovery-specific overlay. The capability is
preparatory for Qovery-managed Engine v2 clusters; the current customer-managed Engine v2 flow
cannot select it.

## Resource profiles

One component-level selector, `resources.profile = CHART_DEFAULT | SMALL | MEDIUM | LARGE | CUSTOM`
(q-core `docs-v2/slice-4-7-source3-resource-profiles.md` owns the product contract):

- `CHART_DEFAULT` (the default) emits no `resources` fragment, so a configuration stored before the
  selector existed keeps its exact compiled values — the golden tests prove it;
- `SMALL`/`MEDIUM`/`LARGE` apply the budget table in `resources/setting.pkl`. One preset is
  role-aware internally (each workload target gets its own budget) while the customer selects a
  single value. Presets are resource budgets, not capacity guarantees. The first table is
  PROVISIONAL until the Slice 4.7 calibration review approves observed numbers;
- `CUSTOM` exposes `resources.<target>.requests|limits.cpuMilli|memoryMi` integer fields for the
  active topology; `500` compiles to `500m` and `512` to `512Mi`. Requests are required, limits
  stay optional, and `limit >= request` is enforced independently for CPU and memory.

Transparency: the contract has no read-only rendering, so the custom fields are returned only
while `CUSTOM` is selected — an exposed field would otherwise be editable yet ignored. Each
preset's numeric budgets for the active topology are published in the `resources.profile` field
description instead (the fallback defined by the slice, and explicitly temporary: it moves to
dedicated read-only fields once the contract and Console support them). The `CUSTOM` fields carry
the `MEDIUM` recommendation in `defaultValue`. Custom values hidden by the current topology/profile
stay in the context-free DESCRIBE allow-list: q-core preserves them, validation ignores them, and
the compiler never reads them. Fields are returned in Console rendering order, with `storage` last
so it sits directly above the cluster-inputs section its choice activates.

Inactive chart targets receive no resource block: single-binary mode configures `singleBinary`;
high availability configures `read`, `write`, `backend` and `gateway`. The compiled values are
complete on their own — no namespace `LimitRange` or other admission-time default is needed to
finish them.

A published preset table is immutable. Changing a number requires a new bundle version and a new
root template release, announced with the old and new budgets, because it changes compiled customer
infrastructure without any customer action.

## Runtime values

For AWS, the model exposes only the values a user or another platform component must provide: the
S3 bucket name and the Loki IAM role ARN. The AWS backend also declares `infra.awsRegion` as a
compile-only input (`s3.pkl`, `option.compileInputs`). It is trusted cluster context: q-core
declares it as a `qcoreValue` sourced from `cluster.region`, resolves it immediately before
`COMPILE`, and passes it in `clusterInputs`. `VALIDATE` therefore does not ask the Console for a
region, while `COMPILE` checks it and fails closed if q-core does not inject it.

For S3-compatible storage, q-core receives only the Secret name. The customer creates that Secret
in the Loki namespace before deployment:

```shell
kubectl -n qovery create secret generic loki-object-storage \
  --from-literal=S3_ACCESS_KEY_ID='<access-key>' \
  --from-literal=S3_SECRET_ACCESS_KEY='<secret-key>'
```

The compiled values reference `${S3_ACCESS_KEY_ID}` and `${S3_SECRET_ACCESS_KEY}`, enable Loki's
environment expansion, and load the named Secret with `extraEnvFrom`. Credentials therefore never
enter q-core, the binding, the public OCI bundle, or `values.final.yaml`.

## Deployment lifecycle boundary

This bundle compiles the desired Helm values for a fresh Engine v2 installation. Its request does
not contain the last applied profile or the currently deployed Loki topology, so it cannot safely
distinguish a first installation from a day-2 storage or `SingleBinary`/`SimpleScalable`
transition.

Until a migration workflow supplies that applied state, q-core must keep `storage` and
`highAvailability` immutable after Loki has been installed. A storage migration must append a new,
future-dated Loki schema period; a topology migration must follow the chart's staged migration
mode. Existing Engine v1 Loki installations are therefore not migration inputs for this bundle.

Object storage is enabled only for `CUSTOMER_MANAGED`. `QOVERY_MANAGED` remains fail-closed until
the Terraform-output execution barrier is available before Helm compilation; on such a cluster an
object-storage choice is not offered, reports `MANAGED_OBJECT_STORAGE_EXECUTION_NOT_AVAILABLE`,
and activates none of its inputs.

## Security boundary

In production, imports use q-core's virtual `bundle:/` loader. It exposes only `.pkl` modules from
the same digest-pinned OCI component bundle. Filesystem, environment, package, and network access
remain disabled.

## Module layering

The tree above is a checked invariant. `tools/platform-catalog-tests/tests/module_layering.rs`
parses every import in the published bundles and fails on: a setting folder importing anything but
`contract.pkl` from the bundle root; a setting folder importing anything but a sibling's
`setting.pkl`; two folders importing each other's `setting.pkl`; a cross-folder import whose alias
is not `<folder><Module>`; a same-folder import that is aliased; an entrypoint that
imports anything but `settings.pkl` and `sdk/`; any import added to the vendored contract; and a
vendored `sdk/` module importing anything but `contract.pkl` or another `sdk/` module. Importing
`sdk/` is allowed from every module. The same file proves each rule fires, so the check cannot
silently stop matching. Run it with:

```shell
cargo test --manifest-path tools/platform-catalog-tests/Cargo.toml --test module_layering
```

## Changes from the previous layout

Contract-visible, none covered by a captured exchange, each pinned by a test:

- an explicit `null` on a setting is `INVALID_TYPE`; it used to fall back to the default;
- `RESOLVE_REQUIREMENTS` on a Qovery-managed cluster with an object-storage choice lists no input;
- the order of violations follows the Console order of settings, then the dependency rules;
- the key order of `values.final.yaml` follows the Console order (retention before topology).

## Tests

```bash
./scripts/test-platform-config.sh
```

One suite per setting folder, an end-to-end suite per operation, and golden COMPILE outputs
rendered as JSON so that a model change shows up as a plain values diff in the merge request.
Regenerate the golden file after a deliberate change with:

```bash
pkl test --overwrite platform-catalog/components/loki/tests/compile-golden.tests.pkl
```
