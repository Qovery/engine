# cluster-agent

The cluster agent belongs to the mandatory `qovery-stack` layer. Its direct runtime inputs provide
the gateway URL, cluster identity, JWT, and image through `managed-values.yaml`. Its small Pkl
evaluator derives only `environmentVariables.LOKI_URL` from the effective component selection:

- when `loki` is enabled, it uses the stable in-cluster gateway shared by SingleBinary and
  SimpleScalable modes;
- when `loki` is disabled, it emits the legacy-compatible empty string.

`runtime-values/contract.pkl` and `runtime-values/sdk/` are vendored copies of the canonical
evaluator contract and authoring SDK under `platform-catalog/pkl/`; the model uses the shared
operation vocabulary, the SDK request readers, and the SDK result envelope. Both copies are
machine-synced by `./scripts/sync-platform-pkl-sdk.sh`, never edited here.

The root template declares `after loki`: q-core installs Loki first when `log-infra` is enabled, but
the ordering edge does not force that optional layer to be selected. This keeps the cluster agent
functional when log history is disabled and avoids retries against a nonexistent Kubernetes
Service.
