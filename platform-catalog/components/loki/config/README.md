# Loki product configuration model

You do not need to know Pkl to review this model. Read the files in this order:

1. `model.pkl` — short entrypoint and operation routing;
2. `catalog.pkl` — fields shown by the Console and conditional object-storage inputs;
3. `validation.pkl` — product rules and stable violation codes;
4. `helm.pkl` — translation from valid product settings to Loki Helm values;
5. `profile.pkl` — defaults and safe type conversion shared by validation and compile;
6. `contract.pkl` — JSON response types shared with q-core.

## Pkl syntax used here

| Syntax | Meaning in this model |
| --- | --- |
| `import "catalog.pkl"` | Load another module from the same OCI bundle. |
| `local` | Private implementation detail, omitted from rendered output. |
| `function name(arg: Type)` | Reusable typed function. |
| `new contract.Field { ... }` | Construct a checked contract object. Misspelled properties fail evaluation. |
| `new Mapping { ["key"] = value }` | Build dynamic Helm/JSON key-value data. |
| `when (condition) { ... }` | Add mapping entries only when the condition is true. |
| `value ?? fallback` | Use `fallback` when `value` is null. |
| `value?.property` | Read a property only when `value` is not null. |

## Operations

- `DESCRIBE`: returns the complete Console field catalogue without cluster context, or the effective
  provider-specific choices when `clusterContext` is supplied.
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

The unscoped catalog remains a capability index and therefore lists all five storage values. A
contextual catalog read and every cluster preview narrow the `storage` field to the effective pair:
AWS=`pvc|s3`, GCP=`pvc|gcs`, Azure=`pvc|azureBlob`, Scaleway=`pvc|s3Compatible`. Qovery-managed
clusters currently expose only `pvc`. The Console renders the returned `allowedValues`; it does not
encode this matrix or any Loki-specific provider condition.

## Runtime values

For AWS, the model exposes only the values a user or another platform component must provide: the
S3 bucket name and the Loki IAM role ARN. The AWS region is trusted cluster context: q-core declares
`infra.awsRegion` as a `qcoreValue` sourced from `cluster.region`, resolves it immediately before
`COMPILE`, and passes it in `clusterInputs`. `VALIDATE` therefore does not ask the Console for a
region, while `COMPILE` still fails closed if q-core does not inject it.

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
