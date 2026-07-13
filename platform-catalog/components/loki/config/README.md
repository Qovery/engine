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

## Security boundary

In production, imports use q-core's virtual `bundle:/` loader. It exposes only `.pkl` modules from
the same digest-pinned OCI component bundle. Filesystem, environment, package, and network access
remain disabled.

Run the contract examples from the engine repository root:

```shell
PKL_BIN=pkl ./scripts/test-platform-config.sh
```
