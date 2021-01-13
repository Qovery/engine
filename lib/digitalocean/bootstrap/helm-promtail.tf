resource "helm_release" "promtail" {
  name = "promtail"
  chart = "common/charts/promtail"
  namespace = "logging"
  create_namespace = true
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "loki.serviceName"
    value = "loki"
  }

  # Limits
  set {
    name = "resources.limits.cpu"
    value = "100m"
  }

  set {
    name = "resources.requests.cpu"
    value = "100m"
  }

  set {
    name = "resources.limits.memory"
    value = "128Mi"
  }

  set {
    name = "resources.requests.memory"
    value = "128Mi"
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
  ]
}
