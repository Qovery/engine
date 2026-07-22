# cert-manager-configs

This bundle creates the Qovery DNS provider Secret, `letsencrypt-qovery` ClusterIssuer, and wildcard
Certificate in the `cert-manager` namespace. q-core supplies the exact managed domain, normalized
Qovery DNS endpoint, cluster JWT, and deployment-policy ACME values.

Only DNS-01 is enabled. Ingress HTTP-01, Gateway HTTP-01, ReferenceGrant, and user-provided Envoy
certificate paths are disabled for Slice 4.8.
