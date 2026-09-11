# cert-manager-configs

This bundle creates the Qovery DNS provider Secret, `letsencrypt-qovery` ClusterIssuer, and wildcard
Certificate in the `cert-manager` namespace. q-core supplies the exact managed domain, normalized
Qovery DNS endpoint, cluster JWT, and deployment-policy ACME values.

Only DNS-01 is enabled. Ingress HTTP-01 and Gateway HTTP-01 remain disabled.

For a customer-managed TLS certificate, configure
envoy.custom_certificate.existing_secret_name to letsencrypt-acme-qovery-cert and pre-provision
that kubernetes.io/tls Secret in the cert-manager namespace. The private key never enters q-core or the
platform configuration; the fixed name keeps the Gateway and ReferenceGrant references aligned.

The runtime bundle follows the canonical SDK layout: `model.pkl` only decodes the request and
calls `sdk/evaluate.pkl`; `settings.pkl` assembles the component; `configuration/setting.pkl`
declares typed settings and `configuration/helm.pkl` maps them to chart values. Cross-setting
rules, when needed, live in `configuration/dependencies.pkl`. The SDK derives descriptors and
recursive validation from those declarations, including unknown fields and explicit nulls.
Run `./scripts/sync-platform-pkl-sdk.sh` after changing the shared contract or SDK, then
`./scripts/test-platform-config.sh` and the `platform-catalog-tests` architecture tests.
