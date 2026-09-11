# Karpenter custom pool configuration model

This bundle compiles the accepted Source 2 pool profile into values for the new
`karpenter-custom-resources` chart, version `0.1.0`, targeting Karpenter `1.10.0`.
It does not use or modify the legacy `karpenter-configuration` chart.

`model.pkl` decodes the request and calls the canonical SDK evaluator. `settings.pkl`
assembles the `nodePools/` feature: `setting.pkl` declares types, bounds and defaults;
`dependencies.pkl` declares indexed uniqueness and cluster admission rules; `inputs.pkl`
declares the component-wide logical inputs; `helm.pkl` projects validated active values.
The component factory receives cluster inputs for the real EKS discovery-tag default
and the draft to declare AWS inputs only when at least one pool exists. No request decoding, validation orchestration or compilation gate is duplicated.

Contract and SDK copies are synchronized from `platform-catalog/pkl`. The SDK derives both
prototypes and row descriptors, validates raw drafts, and passes defaulted active values
to the Helm fragment only after validation. RESOLVE_REQUIREMENTS accepts missing required
profile values; VALIDATE and COMPILE enforce them, as for the other catalogue components.
No runtime import reaches the test fixture, another component or an example file.

## Profile and mapping

Each `nodePools` item creates one `NodePool` and one `EC2NodeClass`, both named
`qovery-<name>`. A name is 1–56 lowercase DNS characters. Names remain stable when rows move.
Absent or empty pool lists render no resources. Duplicate names, unknown fields and malformed active values are rejected.

| Profile | Helm/resource result |
| --- | --- |
| `instanceTypes` | Explicit unique nonempty instance-type requirement |
| `spotEnabled` | Default false → On-Demand; true → Spot and On-Demand |
| `consolidation.policy` | Default `WhenEmptyOrUnderutilized`, alternative `WhenEmpty` |
| `taints` | Optional key/value/effect list; duplicate key/effect pairs rejected |
| `ami.mode: default` | Legacy `al2@latest` through 1.32, `al2023@latest` from 1.33 |
| `ami.mode: custom` | AMI ID and family; default AL2023, also AL2 and Bottlerocket |
| `subnets.mode: auto` | Editable tag; default `karpenter.sh/discovery=<aws.eksClusterName>` |
| `subnets.mode: custom` | Unique nonempty list of subnet IDs |

Only the active AMI/subnet branch is validated and compiled. Inactive draft values remain
available to q-core for a later mode switch. Descriptors use the A1 `items` prototype and
`itemFields` per-row evaluations. No example EKS name becomes a default when inputs are absent.

Fixed pool settings match the legacy default-pool baseline: `expireAfter: 720h`,
`terminationGracePeriod: 4h`, weight 50, consolidation delay 1m and 10% disruption budget.
Node metadata hop limit is 2. Shared node role and security group are supplied explicitly.
Disk mappings and additional EC2 tags are outside this profile: the pinned Karpenter AMI-family
defaults apply; the legacy disk-size setting is not reproduced. This must remain visible in
the demo preparation. AMI architecture, availability and bootstrap compatibility need AWS checks.

A taint with an empty string value compiles without the `value` property: both represent an
empty taint value, but the pinned NodePool CRD's pattern rejects an explicit empty string.
Controller tolerations may retain an explicit empty value.

## Logical requirements and admission

`aws.eksClusterName`, `aws.nodeRoleName` (a role name, not an ARN) and
`aws.nodeSecurityGroupId` are required scalar CLUSTER inputs when at least one pool exists. They identify precreated resources;
their syntax/presence is not evidence of existence, permissions or correct cluster/VPC ownership.
RESOLVE_REQUIREMENTS advertises them; VALIDATE and COMPILE require valid values.

With a nonempty pool list, outside DESCRIBE context must contain `provider: AWS`, `mode: CUSTOMER_MANAGED` and an exact
`kubernetesVersion` such as `1.34`. KAR-03 bounds this demo to 1.30–1.35. This is a demo policy,
not a claim that upstream Karpenter rejects every older version: the [pinned version provider](https://github.com/aws/karpenter-provider-aws/blob/v1.10.0/pkg/providers/version/version.go)
admits 1.26–1.35. Older legacy AMI rules are covered as pure compatibility tests only.

## Activation dependencies

The registered `karpenter` layer contains this component after both `karpenter-crd` and
`karpenter` using existing `dependsOn` / `kind: requires`, in `kube-system`. Publication and
cluster deployment are separate steps; editing these sources does not publish an artifact.

Renaming/removing deployed pools cannot be validated from this draft alone. The future runtime
guard must compare the desired set against reliably observed owned resources before Helm.
Existing customer Karpenter must be detected and blocked; this model does not implement adoption.

See [local examples and verification](../../../examples/karpenter-v0/README.md).

## POC activation and empty custom configuration

The single optional `karpenter` layer groups CRDs, controller, Qovery pools and custom pools.
The custom release remains `kube-system/karpenter-custom-configuration`, distinct from the
Qovery release `kube-system/karpenter-configuration`. Per-component activation is future work.
Controller and Qovery fields remain required when enabling the layer.

An absent `nodePools` field or `nodePools: []` is valid and compiles `pools: []`, rendering
no Kubernetes resource. It exposes no custom AWS requirements and does not validate unused
AWS values or cluster/version admission. The model's component factory conditions its logical
input declarations on the draft; scalar/row validation and the compile gate stay in the
canonical SDK. Template runtime inputs are optional at transport level; once any pool exists,
the model requires all three AWS inputs and retains every existing row/context constraint.
Malformed arrays and incomplete rows are still rejected.

An empty desired state removes resources previously owned by this custom Helm release.
It is not a preserve-existing-resources switch. Retain all desired pools in the configuration
before updating an existing release; no automatic migration or restoration is provided.
