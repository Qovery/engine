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
  tests/             # native Pkl tests of the SDK primitives (never published)
```

This directory deliberately mirrors a bundle's `runtime-values/` root: `sdk/request.pkl` imports
`../contract.pkl` and resolves it here during authoring and inside `runtime-values/` at runtime,
so the vendored copies stay byte-identical to the source.

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
