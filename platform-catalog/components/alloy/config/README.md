# alloy

This bundle reproduces the Alloy values from the legacy BYOK stack. It runs as
a DaemonSet, uses `qovery-high-priority`, and pushes to the stable Kubernetes
service URL `http://loki-gateway.qovery.svc/loki/api/v1/push`. The Loki gateway
routes that endpoint to `loki` in SingleBinary mode and to `loki-write` in
SimpleScalable mode.

Alloy belongs to the existing `log-infra` layer with Loki. The layer is enabled by default for
Qovery-managed and customer-managed clusters. Alloy has no cluster-specific runtime input. Its
root-template dependencies require both `loki` and `qovery-priority-class`; q-core orders those
components first and rejects a future component-level selection that enables Alloy without them.

The image is pinned by manifest digest in the same `pub-mirror-alloy` Public ECR repository used by
the legacy Engine runtime override. Keeping that override in this bundle avoids anonymous Docker
Hub pulls from every cluster node.
