# envoy-gateway

This bundle deploys the Envoy Gateway controller under Qovery's GatewayClass controller name.
Its CRDs are deliberately disabled here because `envoy-gateway-crd` owns their lifecycle. The
frozen upstream chart is published as `gateway-helm`; `envoy-gateway` is the stable Qovery release
and configuration-bundle name.

Its Pkl evaluator exposes the controller-specific legacy cluster advanced settings through
`DESCRIBE`: replicas plus CPU and memory requests and limits. It validates positive resources and
ensures a limit is never below its corresponding request before compiling Helm values.

The runtime bundle follows the canonical SDK layout: `model.pkl` only decodes the request and
calls `sdk/evaluate.pkl`; `settings.pkl` assembles the component; `configuration/setting.pkl`
declares typed settings and `configuration/helm.pkl` maps them to chart values. Cross-setting
rules, when needed, live in `configuration/dependencies.pkl`. The SDK derives descriptors and
recursive validation from those declarations, including unknown fields and explicit nulls.
Run `./scripts/sync-platform-pkl-sdk.sh` after changing the shared contract or SDK, then
`./scripts/test-platform-config.sh` and the `platform-catalog-tests` architecture tests.
