# external-dns

This bundle installs ExternalDNS with Qovery DNS through its PowerDNS provider. It watches
Kubernetes Services plus HTTPRoute and GRPCRoute resources from the catalog's Envoy Gateway stack.
It intentionally excludes the deprecated TLSRoute source because the current Gateway API CRDs do
not serve the API version ExternalDNS expects.

The API key is read from the `external-dns-secret` Secret. Only the non-secret JWT revision enters
the pod annotation used to roll the deployment after credential rotation.
