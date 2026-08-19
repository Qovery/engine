# qovery-operator configuration

q-core resolves every value in `runtime-values/managed-values.yaml` before rendering the Operator chart.

`engineWorker.imageTagSuffix` is forwarded verbatim as
`QOVERY_ENGINE_WORKER_IMAGE_TAG_SUFFIX`. Its default is an empty string. q-core
selects the value from the cluster context: it supplies `-slim` for demo
clusters and an empty string for other clusters. The catalog does not infer the
cluster type.

The suffix stays separate from `engineWorker.imageRepository` and from the
Engine service version. The Operator appends it when constructing the worker
image tag. For example, Engine service version `v1.341.0` and suffix `-slim`
produce `public.ecr.aws/r3m4q3r9/engine:v1.341.0-slim`.
