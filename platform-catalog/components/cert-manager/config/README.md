# cert-manager

This bundle installs the frozen cert-manager `v1.20.0` chart for the customer-managed
`dns-certificates` layer: optional in `qovery-cluster-v0`, mandatory in `qovery-self-managed-v0`
and `qovery-demo-v0`. It owns the CRDs and keeps them on Helm uninstall.

Gateway API, ListenerSet, VPA, and Prometheus ServiceMonitor integration are disabled for this
network-free vertical. The release identity and namespace are fixed to `cert-manager`.
