# Loki product configuration model

You do not need to know Pkl to review this model. Read the files in this order:

1. `model.pkl` — short entrypoint and operation routing;
2. `catalog.pkl` — fields shown by the Console and conditional S3 inputs;
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

- `DESCRIBE`: returns the static Console field catalogue.
- `RESOLVE_REQUIREMENTS`: activates logical inputs from the current draft, for example S3 bucket
  and role when `storage=s3`.
- `VALIDATE`: returns all product and input violations without persisting invalid configuration.
- `COMPILE`: revalidates and emits Source 3 Helm values only when there is no violation.

q-core sends the request as JSON through `prop:request` and receives JSON from `model.pkl`'s
`output`. Pkl syntax and errors never cross the backend API boundary.

## S3 runtime values

The model exposes only the values a user or another platform component must provide: the S3 bucket
name and the Loki IAM role ARN. The AWS region is trusted cluster context: q-core declares
`infra.awsRegion` as a `qcoreValue` sourced from `cluster.region`, resolves it immediately before
`COMPILE`, and passes it in `clusterInputs`. `VALIDATE` therefore does not ask the Console for a
region, while `COMPILE` still fails closed if q-core does not inject it.

## Deployment lifecycle boundary

This bundle compiles the desired Helm values for a fresh Engine v2 installation. Its request does
not contain the last applied profile or the currently deployed Loki topology, so it cannot safely
distinguish a first installation from a day-2 `pvc`/`s3` or `SingleBinary`/`SimpleScalable`
transition.

Until a migration workflow supplies that applied state, q-core must keep `storage` and
`highAvailability` immutable after Loki has been installed. A storage migration must append a new,
future-dated Loki schema period; a topology migration must follow the chart's staged migration
mode. Existing Engine v1 Loki installations are therefore not migration inputs for this bundle.

## Security boundary

In production, imports use q-core's virtual `bundle:/` loader. It exposes only `.pkl` modules from
the same digest-pinned OCI component bundle. Filesystem, environment, package, and network access
remain disabled.

Run the contract examples from the engine repository root:

```shell
PKL_BIN=pkl ./scripts/test-platform-config.sh
```
