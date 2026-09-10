# Karpenter CRDs — unpublished lot B

Use the existing vendored `karpenter-crd` chart version `1.10.0`, matching the controller.
The configuration is declarative and empty; no Pkl evaluator or logical input is necessary.
`static-values/base.yaml` and `runtime-values/managed-values.yaml` follow the existing bundle layout.

This component precedes `karpenter`, then `karpenter-configuration`, in the future optional
Karpenter layer. The controller's own bundled CRDs must be skipped during Helm execution.
CRD establishment, ownership, refusal of an existing customer installation and failure/retry
policy must be implemented and tested before activation. Deleting this component is not an
authorized cleanup strategy for a partial installation.

Nothing is added to the publication catalogue or active templates in lot B.
See [local verification](../../../examples/karpenter-v0/README.md).
