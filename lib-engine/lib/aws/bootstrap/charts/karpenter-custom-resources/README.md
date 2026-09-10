# Karpenter custom resources

An unpublished chart consuming `pools`, compiled by the Karpenter configuration Pkl bundle.
Each item contains a resource `name`, `nodePoolSpec` and `nodeClassSpec`. The chart emits only
that pair; the empty default emits no resources. Legacy charts remain unchanged.

Specs are serialized using `toYaml`; customer data is never evaluated with `tpl`. A schema
checks the top-level shape and required resource sections. Semantic profile validation belongs
to Pkl and runtime compatibility/ownership checks belong to the execution layer.

The chart targets Karpenter CRD APIs `karpenter.sh/v1` and `karpenter.k8s.aws/v1`, pinned to the
vendored 1.10.0 release. Open spec maps do not announce a supported Source 3 editing flow.
Publishing and installing require the later ownership, readiness and AWS checks.
