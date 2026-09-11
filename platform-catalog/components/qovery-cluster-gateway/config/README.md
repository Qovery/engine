# qovery-cluster-gateway

This bundle creates the public Qovery Gateway, its EnvoyProxy, and the bootstrap DNS HTTPRoute.
It derives the route hostname from q-core's root managed domain by adding the wildcard label; the
same root domain is consumed unchanged by `cert-manager-configs` when issuing the TLS certificate.
The baseline intentionally uses standard Kubernetes Service behavior, so cloud-specific load
balancer annotations can be introduced later as typed provider overlays rather than embedded in a
shared chart value file.

Its Pkl evaluator exposes the chart-scoped legacy `envoy.*` cluster advanced settings through
`DESCRIBE`, validates the input, and compiles it to `qovery-cluster-gateway` Helm values. The
profile deliberately keeps omitted values absent so the reviewed static defaults remain in force.
The evaluator currently covers the public data-plane HPA, stream idle timeout, path policy,
trusted X-Forwarded-For hops, access logging, custom error responses, compression, and the default
backend. Access-log JSON is base64-encoded by the evaluator because the frozen Helm chart decodes
that value before parsing it. Per-service `network.gateway_api.*` settings and the cluster retry/request-timeout
fallbacks remain Engine route-policy inputs: they do not map to this Helm chart.

The certificate collection uses the shared `ArrayField` / `ObjectItem` descriptors, with
`fields` for object members and `itemFields` for current draft rows, as consumed by q-core
and the Console. Scalar descriptors retain their existing wire shape. The canonical contract
and SDK are synchronized into each bundle with `scripts/sync-platform-pkl-sdk.sh`.

The runtime bundle follows the canonical SDK layout: `model.pkl` only decodes the request and
calls `sdk/evaluate.pkl`; `settings.pkl` assembles the component; `configuration/setting.pkl`
declares typed settings and `configuration/helm.pkl` maps them to chart values. Cross-setting
rules, when needed, live in `configuration/dependencies.pkl`. The SDK derives descriptors and
recursive validation from those declarations, including unknown fields and explicit nulls.
Run `./scripts/sync-platform-pkl-sdk.sh` after changing the shared contract or SDK, then
`./scripts/test-platform-config.sh` and the `platform-catalog-tests` architecture tests.
