# alloy

This bundle reproduces the Alloy values from the legacy BYOK stack. It runs as
a DaemonSet, uses `qovery-high-priority`, and pushes to the stable Kubernetes
service URL `http://loki-gateway.qovery.svc/loki/api/v1/push`. The Loki gateway
routes that endpoint to `loki` in SingleBinary mode and to `loki-write` in
SimpleScalable mode.

The `log-collector` layer is customer-managed, optional, and disabled by
default. Alloy has no cluster-specific runtime input. Its root-template
dependencies require both `loki` and `qovery-priority-class`; q-core rejects a
binding that enables Alloy without those components.
