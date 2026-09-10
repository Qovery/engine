# Karpenter controller model — unpublished lot B

Source 2 values for the existing vendored `karpenter` chart, version `1.10.0`.
No upstream chart changes, AWS provisioning or root-template activation are included.

The bundle uses the existing model → evaluation → describe/requirements/validate/compile
separation and lazy SDK compilation gate. The SDK and contract are machine-vendored unchanged.
Malformed draft fields become violations; no bundle imports another component or a fixture.

## Existing EC2 placement

`nodeSelectorValue` is required. `nodeSelectorKey` defaults to `eks.amazonaws.com/nodegroup`
and can be changed to the actual identity label of an existing independent EC2 group.
Linux node selection and required affinity exclude both Karpenter nodes and Fargate.
Identity selectors that replace those fixed constraints are rejected.
The runtime must still inspect the selected nodes: a syntactically valid label is not proof
that they exist, are Ready or independent of Karpenter.

Optional `tolerations` accept key/value/effect entries compiled with `operator: Equal`.
They supplement the upstream `CriticalAddonsOnly` toleration. Empty values are supported.

The static values retain two controller replicas. Helm recursively merges the node affinity
with the pinned chart's required host anti-affinity and zone spread constraints. The independent
group consequently needs at least two suitable hosts and capacity for two pods requesting
1 CPU and 1Gi each, with a 1Gi memory limit. Local rendering tests verify this merge.

## Inputs and context

Required scalar CLUSTER inputs are `aws.eksClusterName`, `aws.controllerRoleArn` and
`aws.interruptionQueueName`. The role ARN supports an IAM role path. The queue is a standard
SQS queue, not FIFO. The service account is `karpenter`, annotated for IRSA. Runtime checks must
verify the EKS OIDC/trust policy, AWS permissions, interruption events/queue and node placement.

Non-DESCRIBE operations require AWS / CUSTOMER_MANAGED context and an exact Kubernetes version
in the KAR-03 demo range 1.30–1.35. Version transport and validation against the live EKS are
pending in lots C–E. No fallback example values are used.

## Activation prerequisites

The future optional layer installs in `kube-system` after `karpenter-crd` with
`dependsOn: [{component: karpenter-crd, kind: requires}]`. The controller chart also ships CRDs
under `crds/`; execution must use `--skip-crds` so the separate CRD component owns them.
CRD establishment and controller availability must form readiness barriers before configuration.
Namespace exceptions, Helm failure policy and ownership guards are execution work, not Pkl checks.

This source is intentionally unregistered in `platform-catalog/catalog.yaml` and root templates.
See [examples and verification](../../../examples/karpenter-v0/README.md).

## SDK layout

The entrypoint calls `sdk/evaluate.pkl` with the component declared in `settings.pkl`.
The `placement/` feature owns its setting declarations, dependency rules, unconditional
logical inputs and Helm fragment. Generic type/bound/unknown-field checks, defaults and
the lazy compile gate are supplied by the canonical SDK. Contract and SDK copies are
maintained by `scripts/sync-platform-pkl-sdk.sh`; no local evaluator is maintained.
