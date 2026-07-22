# external-dns

This bundle installs ExternalDNS with Qovery DNS through its PowerDNS provider. It watches
Kubernetes Services only; Ingress and Gateway API sources stay disabled until the network slice
derives them from installed capabilities.

The API key is read from the `external-dns-secret` Secret. Only the non-secret JWT revision enters
the pod annotation used to roll the deployment after credential rotation.
