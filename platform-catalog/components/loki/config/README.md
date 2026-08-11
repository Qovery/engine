# Loki product configuration model

```text
loki/
  config/
    static-values/
      base.yaml
      overlays/
        qovery-karpenter.yaml   # Source 2 capability overlay selected by q-core
    runtime-values/
      model.pkl          # L5 entrypoint: decodes prop:request, renders JSON
      evaluation.pkl     # L4 builds the EvaluationResult envelope
      describe.pkl       # L3 one module per contract operation, composition only
      requirements.pkl
      validate.pkl
      compile.pkl
      profile.pkl        # L1 typed reads of the stored draft
      contract.pkl       # L0 vocabulary, no dependencies
      context.pkl
      product.pkl
      sdk/               # L0 vendored authoring SDK (machine-synced, see ../../../pkl/README.md)
        request.pkl
        validate.pkl
        result.pkl
      storage/           # L2 self-contained feature package
        types.pkl
        inputs.pkl
        backends.pkl
        requirements.pkl
        validate.pkl
        helm.pkl
      resources/         # L2 self-contained feature package
        types.pkl
        targets.pkl
        presets.pkl
        fields.pkl
        validate.pkl
        helm.pkl
  tests/
    runtime-values.test.pkl
    runtime-values.test.pkl-expected.pcf
    compile-golden.tests.pkl
    compile-golden.tests.pkl-expected.pcf
    rules.tests.pkl
    resource-profiles.tests.pkl
```

The files under `runtime-values/` follow the Console use cases. `model.pkl` is the only entrypoint
called by q-core and routes each request to a small module named after the user action:

| Console use case | Data source | Pkl operation and files |
| --- | --- | --- |
| List templates, layers and components | [`templates/qovery-cluster-v0/template.yaml`](../../../templates/qovery-cluster-v0/template.yaml), published in OCI | No Pkl call for the list. q-core then enriches every component with `DESCRIBE`. |
| List the parameters a customer can change | Component config bundle | `DESCRIBE` → `model.pkl` → `describe.pkl` + no-op `validate.pkl`; `describe.pkl` reads `storage/backends.pkl` |
| Refresh conditional inputs after a parameter changes | Current form draft + cluster context | `RESOLVE_REQUIREMENTS` → `model.pkl` → `describe.pkl` + `requirements.pkl` + `validate.pkl` |
| Check constraints before saving | Current form draft + resolved inputs | `VALIDATE` → `model.pkl` → `describe.pkl` + `requirements.pkl` + `validate.pkl` |
| Build the final Helm values for deployment | Valid saved profile + resolved inputs | `COMPILE` → the validation path above, then `compile.pkl` → `storage/helm.pkl` |

This distinction is intentional: Pkl does **not** define which layers exist or which components are
in a layer. The root template is the catalog composition source of truth. Pkl describes and
compiles the configuration of one component after q-core has found that component in the template.

Pkl imports are static: evaluating `model.pkl` resolves all of its imported modules from the bundle.
The table describes the functions that contribute to each response, not a lazy file-loading order.

## Where to look

- `describe.pkl`: the labels, descriptions and Console ordering of the editable fields
  (`retentionWeeks`, `highAvailability`, `storage`, `resources.profile` and the per-workload
  resource fields). Presentation only — every default and bound is owned by the module that owns
  the concept, so this layer stays a pure consumer;
- `requirements.pkl`: resolves the draft, then asks each feature package which of its inputs the
  selection activates;
- `validate.pkl`: the operation gate, the rules for the product fields that belong to no feature
  package, and one call per feature package;
- `compile.pkl`: the readable, provider-neutral Loki Helm topology. It resolves the draft once and
  passes plain values down, which is what lets the feature packages stay independent of it;
- `product.pkl`: defaults and bounds for the settings that belong to no feature package;
- `storage/helm.pkl`: the whole `loki:` chart block plus the per-backend Helm adapters;
- `storage/types.pkl`: the shared storage types;
- `storage/inputs.pkl`: logical runtime inputs with their labels, types and constraints;
- `storage/backends.pkl`: the supported backend instances and provider matrix;
- `storage/requirements.pkl`: the inputs the selected backend activates;
- `storage/validate.pkl`: storage type/value rules, provider compatibility and the required-input
  checks for the backend's customer and compile-only inputs;
- `resources/types.pkl`: the profile vocabulary, integer units, bounds and budget/target types;
- `resources/targets.pkl`: the workload-target registry and dotted Source 3 field keys;
- `resources/presets.pkl`: the versioned `SMALL`/`MEDIUM`/`LARGE` budget tables;
- `resources/fields.pkl`: the selector and per-workload custom fields with preset-seeded defaults;
- `resources/validate.pkl`: the `CUSTOM` value rules (integers, bounds, required requests,
  `limit >= request`);
- `resources/helm.pkl`: budget-to-`resources`-block compilation;
- `profile.pkl`: defaults and safe type conversion shared by resolve, validate and compile;
- `contract.pkl`: vendored canonical operation and JSON response types shared with q-core;
- `sdk/`: vendored authoring SDK — request decoding and typed readers (`sdk/request.pkl`), the
  canonical violation codes and generic validators (`sdk/validate.pkl`), and the result envelope
  owning the fail-closed COMPILE gate (`sdk/result.pkl`). Machine-synced from
  `platform-catalog/pkl/sdk`, never edited here;
- `context.pkl`: Loki-specific provider and cluster-mode types;
- `model.pkl`: routing only.

There are two kinds of constraints, kept next to the value they constrain:

- field constraints returned by `DESCRIBE` live in `describe.pkl`, such as retention min/max and
  the allowed storage choices for the cluster provider;
- logical-input constraints returned by `RESOLVE_REQUIREMENTS` live with their input in
  `storage/inputs.pkl`, such as bucket-name, IAM-role ARN, service-account email and UUID patterns.

`validate.pkl` applies both sets and adds rules involving several values, for example “high
availability requires object storage”. This avoids duplicating constraint metadata in a separate
generic rules file.

## Pkl syntax used here

| Syntax | Meaning in this model |
| --- | --- |
| `import "describe.pkl"` | Load another module from the same OCI bundle. |
| `local` | Private implementation detail, omitted from rendered output. |
| `function name(arg: Type)` | Reusable typed function. |
| `typealias Operation = "DESCRIBE" \| ...` | Closed vocabulary checked by Pkl instead of a free-form string. |
| `new contract.Field { ... }` | Construct a checked contract object. Misspelled properties fail evaluation. |
| `open class StorageBackend` | Allow the storage registry to define a stricter object-storage subtype. |
| `class ObjectStorageBackend extends StorageBackend` | Require every object backend to declare its provider, Loki identifier, and bucket input. |
| `backend is ObjectStorageBackend` | Narrow the type before reading object-storage-only properties. |
| `Mapping<String, StorageBackend>` | Keep every backend in one typed registry instead of repeating provider matrices. |
| `new Mapping { ["key"] = value }` | Build dynamic Helm/JSON key-value data. |
| `when (condition) { ... }` | Add mapping entries only when the condition is true. |
| `value ?? fallback` | Use `fallback` when `value` is null. |
| `value?.property` | Read a property only when `value` is not null. |

## Operations

- `DESCRIBE`: returns the Console fields and their effective provider-specific choices.
- `RESOLVE_REQUIREMENTS`: activates logical inputs from the current draft, for example a GCS
  bucket and service account when `storage=gcs`.
- `VALIDATE`: returns all product and input violations without persisting invalid configuration.
- `COMPILE`: revalidates and emits Source 3 Helm values only when there is no violation.

q-core sends the request as JSON through `prop:request` and receives JSON from `model.pkl`'s
`output`. Pkl syntax and errors never cross the backend API boundary.

## Self-managed storage matrix

`pvc` is provider-neutral and keeps the chart's persistent-volume configuration. Object storage is
provider-specific and disables Loki data PVCs; local `/var/loki` state uses `emptyDir`.

| Cluster provider | `storage` | Runtime inputs | Kubernetes identity |
| --- | --- | --- | --- |
| AWS | `s3` | bucket; IAM role ARN; q-core region | EKS IRSA annotation |
| GCP | `gcs` | bucket; GCP service-account email | GKE Workload Identity annotation |
| Azure | `azureBlob` | account; container; managed-identity client ID | Azure Workload Identity label + annotation |
| Scaleway | `s3Compatible` | bucket; endpoint; region; credentials Secret name | pre-created Kubernetes Secret |

The evaluator rejects a storage/provider mismatch. High availability is valid with any of the four
object-storage values and invalid with `pvc`.

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

`storage/backends.pkl` is the source of truth for this matrix. Adding a backend starts by declaring
its provider, Loki object-store identifier, bucket input, customer-input list, and internal
compile-only input list there.
`describe.pkl` derives the Console choices from that registry, `requirements.pkl` derives the
conditional inputs, and `validate.pkl` checks provider compatibility from the same objects. Only
chart-specific values remain in `storage/helm.pkl`; every backend must have an explicit Helm
adapter, including backends whose adapter is a no-op. The colocalized Pkl test checks exact registry
coverage so a new backend cannot silently compile without its chart behavior. `compile.pkl` stays a
readable overview of the resulting Loki topology.

The unscoped catalog remains a capability index and therefore lists all five storage values. A
contextual catalog read and every cluster preview narrow the `storage` field to the effective pair:
AWS=`pvc|s3`, GCP=`pvc|gcs`, Azure=`pvc|azureBlob`, Scaleway=`pvc|s3Compatible`. Qovery-managed
clusters currently expose only `pvc`. The Console renders the returned `allowedValues`; it does not
encode this matrix or any Loki-specific provider condition.

## Resource profiles

One component-level selector, `resources.profile = CHART_DEFAULT | SMALL | MEDIUM | LARGE | CUSTOM`
(q-core `docs-v2/slice-4-7-source3-resource-profiles.md` owns the product contract):

- `CHART_DEFAULT` (the default) emits no `resources` fragment, so a configuration stored before the
  selector existed keeps its exact compiled values — the golden tests prove byte identity;
- `SMALL`/`MEDIUM`/`LARGE` apply the component-owned budget tables in `resources/presets.pkl`. One
  preset is role-aware internally (each workload target gets its own budget) while the customer
  selects a single value. Presets are resource budgets, not capacity guarantees. The first table is
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
S3 bucket name and the Loki IAM role ARN. The AWS backend also declares `infra.awsRegion` as an
internal compile-only input. It is trusted cluster context: q-core declares it as a `qcoreValue`
sourced from `cluster.region`, resolves it immediately before `COMPILE`, and passes it in
`clusterInputs`. `VALIDATE` therefore does not ask the Console for a region, while `COMPILE` derives
the requirement from the backend registry and fails closed if q-core does not inject it.

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

This POC intentionally enables object storage only for `CUSTOMER_MANAGED`. `QOVERY_MANAGED` remains
fail-closed until the Terraform-output execution barrier is available before Helm compilation.

## Security boundary

In production, imports use q-core's virtual `bundle:/` loader. It exposes only `.pkl` modules from
the same digest-pinned OCI component bundle. Filesystem, environment, package, and network access
remain disabled.

Run the contract examples from the engine repository root:

```shell
PKL_BIN=pkl ./scripts/test-platform-config.sh
```

The script first runs the component-local Pkl facts and snapshots, then exercises the complete JSON
contract through `model.pkl`.

## Module layering

The tree above is not a convention, it is a checked invariant. `tools/platform-catalog-tests/tests/
module_layering.rs` parses every import in the published bundles and fails on: a feature package
importing anything but `contract.pkl`/`context.pkl` from the root; one feature package importing
another; a cross-package import whose alias is not `<package><Module>`; a same-package import that
is aliased; an entrypoint that imports anything but `sdk/request.pkl` and `evaluation.pkl`; any
import added to the vendored contract; and a vendored `sdk/` module importing anything but
`contract.pkl` or another `sdk/` module. Importing `sdk/` is allowed from every module — it is the
shared authoring layer, not a feature package. The same file also proves each of those rules fires,
so the check cannot silently stop matching. Run it with:

```shell
cargo test --manifest-path tools/platform-catalog-tests/Cargo.toml --test module_layering
```

The feature-to-root allow-list is a ratchet: it may shrink, never grow. Removing `context.pkl` from
it means moving `SupportedProvider` — a storage concept — into `storage/types.pkl`.
