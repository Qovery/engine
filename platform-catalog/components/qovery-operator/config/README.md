# qovery-operator configuration

q-core resolves every value in `runtime-values/managed-values.yaml` before rendering the Operator chart.

Demo clusters activate the `QOVERY_DEMO` capability, which merges
`static-values/overlays/qovery-demo.yaml` and configures:

- `QOVERY_ENGINE_WORKER_IMAGE_TAG_SUFFIX: "-slim"` — the Operator appends the
  suffix to the Engine service version when constructing the worker image tag.
  For example, Engine service version `v1.341.0` produces
  `public.ecr.aws/r3m4q3r9/engine:v1.341.0-slim`. The suffix stays separate from
  `engineWorker.imageRepository`, which must remain an untagged repository.
- `QOVERY_ENVIRONMENT_ENGINE_WORKER_PROFILE: "LOCAL_DEMO"` — the Operator
  interprets this profile to enable the Kubernetes builder on `ENVIRONMENT`
  worker Jobs only. Infrastructure workers never receive builder settings.

The configuration is persisted in the Platform Template binding under the
`qovery-operator` component. Its evaluator (`runtime-values/cpuArchitectures/`) requires
`cpuArchitectures` when `QOVERY_DEMO` is active and adds it to the Operator environment. Bootstrap and
Operator self-update therefore reuse the same explicit architecture without
introducing cluster-specific runtime inputs.

## Placement

`runtime-values/placement/` lets the cluster owner choose where the Operator runs. The three
settings are optional; leaving them empty compiles exactly the values the chart received before
this feature existed.

| Setting | Meaning |
| --- | --- |
| `nodeSelectorKey` | Node label key. Defaults to `eks.amazonaws.com/nodegroup`. |
| `nodeSelectorValue` | Node label value. Without it, no node is selected. |
| `tolerations` | Taints to tolerate: `key`, `value` (may be empty), `effect`. |

Typical values are `eks.amazonaws.com/nodegroup=<group>` for an EKS managed node group and
`karpenter.sh/nodepool=<pool>` for a Karpenter NodePool, with a matching toleration when the pool
is tainted — Qovery's `nodepool/stable` taint, for instance, carries no value and uses
`NoSchedule`. The toleration operator is always `Equal`, which already covers valueless taints, so
it is not exposed. `kubernetes.io/arch` is refused as a selector key: the Engine sets it per build
and the Operator image is multi-arch.

The selector and the tolerations are independent. Tolerating a taint without selecting a node is
accepted, for a pool that repels pods without labelling them.

One placement drives two targets, so the Operator and the Engine worker Jobs it creates can never
diverge:

- `nodeSelector` and `tolerations` schedule the Operator Deployment, through the chart values it
  already renders;
- `QOVERY_ENGINE_WORKER_NODE_SELECTOR` and `QOVERY_ENGINE_WORKER_TOLERATIONS` describe the same
  placement to the Operator binary, which applies it to every worker Job.

Both environment variables contain JSON using the Kubernetes `PodSpec` field shapes. The Helm
adapter serializes the same objects used for the Deployment, and the Operator deserializes them
into Kubernetes types before validating them:

```yaml
QOVERY_ENGINE_WORKER_NODE_SELECTOR: '{"karpenter.sh/nodepool":"stable"}'
QOVERY_ENGINE_WORKER_TOLERATIONS: '[{"key":"nodepool/stable","value":"","effect":"NoSchedule","operator":"Equal"}]'
```

The selector is a JSON object of string labels; tolerations are a JSON array of objects.
An environment variable is emitted only when the corresponding setting is filled in.
This encoding requires the Operator version that reads JSON placement variables; the former
comma/semicolon syntax is no longer emitted. The customer-facing settings are unchanged.

A selector matching no node leaves the Operator pod `Pending`: bootstrap shows it immediately and
the customer re-runs it with corrected values, while a self-update keeps the previous Operator
running under the chart's `RollingUpdate` strategy. Worker Jobs that cannot be scheduled stay
`Pending` until their deadline and are then reported as failed. On a Karpenter NodePool the
`Pending` state is usually transient, since Karpenter provisions a node on demand; a managed node
group must already have capacity.
