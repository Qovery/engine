# qovery-cert-manager-webhook

This bundle installs the public Qovery DNS ACME DNS-01 solver after cert-manager is ready.
Credentials are intentionally absent: the webhook reads the namespaced provider Secret created by
`cert-manager-configs` while solving a challenge.

The mirrored chart remains at `0.2.0`. This catalogue bundle overrides its registry settings for
Engine v2 without modifying the legacy chart used by the existing self-managed flow.
