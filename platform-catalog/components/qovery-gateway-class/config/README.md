# qovery-gateway-class

This bundle creates Qovery's public and private GatewayClasses plus their EnvoyProxy defaults.
Both classes use the controller name established by `envoy-gateway`, and retain the legacy
single-replica, anti-concentrated data-plane defaults for demo and customer-managed clusters.

Its Pkl evaluator exposes the shared legacy Envoy HPA policy and access-log format through
`DESCRIBE`, validates replica and utilization bounds, and applies the resulting values to both
the public and private GatewayClass proxies. It base64-encodes the access-log JSON for the frozen
Helm chart's safe transport convention.

The runtime bundle follows the canonical SDK layout: `model.pkl` only decodes the request and
calls `sdk/evaluate.pkl`; `settings.pkl` assembles the component; `configuration/setting.pkl`
declares typed settings and `configuration/helm.pkl` maps them to chart values. Cross-setting
rules, when needed, live in `configuration/dependencies.pkl`. The SDK derives descriptors and
recursive validation from those declarations, including unknown fields and explicit nulls.
Run `./scripts/sync-platform-pkl-sdk.sh` after changing the shared contract or SDK, then
`./scripts/test-platform-config.sh` and the `platform-catalog-tests` architecture tests.
