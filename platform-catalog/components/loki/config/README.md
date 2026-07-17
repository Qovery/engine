# Loki product configuration model

```text
loki/
  config/
    static-values/
      base.yaml
      overlays/
    runtime-values/
      model.pkl
      contract.pkl
      describe.pkl
      requirements.pkl
      validate.pkl
      compile.pkl
      profile.pkl
      storage/
        types.pkl
        inputs.pkl
        backends.pkl
        helm.pkl
  tests/
    runtime-values.test.pkl
    runtime-values.test.pkl-expected.pcf
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

- `describe.pkl`: the editable fields, labels, defaults and field constraints rendered by the
  Console (`retentionWeeks`, `highAvailability`, `storage`);
- `requirements.pkl`: which logical runtime inputs become visible for the current draft;
- `validate.pkl`: cross-field rules, provider compatibility and stable violation codes;
- `compile.pkl`: the readable, provider-neutral Loki Helm topology;
- `storage/helm.pkl`: the chart-specific Helm values for each storage backend;
- `storage/types.pkl`: the shared storage types;
- `storage/inputs.pkl`: logical runtime inputs with their labels, types and constraints;
- `storage/backends.pkl`: the supported backend instances and provider matrix;
- `profile.pkl`: defaults and safe type conversion shared by resolve, validate and compile;
- `contract.pkl`: JSON response types shared with q-core;
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
