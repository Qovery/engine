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
`qovery-operator` component. Its evaluator requires `cpuArchitectures` when
`QOVERY_DEMO` is active and adds it to the Operator environment. Bootstrap and
Operator self-update therefore reuse the same explicit architecture without
introducing cluster-specific runtime inputs.
