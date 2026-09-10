# Local Karpenter Source 2 example — lot B

These are **synthetic, non-deployable example inputs**, not discovered customer resources.
Replace their values only when preparing a verified test environment in a later lot.
They are intentionally excluded from the registered catalogue and templates.

| Component | Local configuration | Target chart | Required predecessors |
| --- | --- | --- | --- |
| `karpenter-crd` | Empty declarative bundle | `karpenter-crd` 1.10.0 | None |
| `karpenter` | Controller Pkl bundle | `karpenter` 1.10.0 | `karpenter-crd` |
| `karpenter-configuration` | Pool Pkl bundle | `karpenter-custom-resources` 0.1.0 | CRDs and controller |

Each runtime-values bundle is self-contained and uses a byte-identical copy of the canonical SDK.
The SDK now exposes component-wide logical inputs and an optional Kubernetes version in
its typed context; Karpenter-specific admission rules stay in its feature folders.
The eventual layer is optional, disabled by default, and uses existing `kind: requires`
dependencies and `kube-system` releases. Component composition belongs to the root template,
not a new layer API in Pkl. There is no published digest or activable layer descriptor yet.

`configuration.request.json` demonstrates two independent pools: standard AMI/tag discovery
with Spot enabled, and custom AMI/subnet IDs with a taint and `WhenEmpty` consolidation.
`controller.request.json` demonstrates an independent EC2 group, an IAM role with a path and
an Equal toleration whose value is empty.

From the repository root, evaluate a request with the same entrypoint as q-core:

```sh
pkl eval -p "request=$(cat platform-catalog/examples/karpenter-v0/configuration.request.json)" \
  platform-catalog/components/karpenter-configuration/config/runtime-values/model.pkl
```

Change `operation` to DESCRIBE, RESOLVE_REQUIREMENTS or VALIDATE to inspect descriptors,
logical requirements or violations. Only a valid COMPILE response contains `helmValues`.

```sh
./scripts/test-platform-config.sh
cargo test --manifest-path tools/platform-catalog-tests/Cargo.toml --test karpenter_models
```

The Rust tests isolate each bundle, evaluate all four operations, render actual charts and
parse resulting manifests. They cover pool independence, stable names, defaults, placement,
CRDs and a tag containing YAML break-out text and a Helm expression remaining literal data.
Native Pkl tests cover malformed drafts, inactive branches, indexed violations and admission.

Before activation, lots C–E must supply and verify Kubernetes context, EKS identity, role/SG/
subnet/AMI compatibility, IRSA, queue, suitable independent nodes, ownership, rename/removal
guards, CRD establishment and controller readiness. The controller install must skip its own
CRDs. The existing namespace and Helm policies also require the planned bounded changes.
Console editing and Source 3 are outside this lot. Manual AWS preparation remains the
temporary D1 shortcut tracked by KAR-08; these files do not replace those runtime checks.
