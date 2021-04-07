resource "helm_release" "prometheus-adapter" {
  name = "prometheus-adapter"
  chart = "common/charts/prometheus-adapter"
  namespace = helm_release.prometheus_operator.namespace
  atomic = true
  max_history = 50

  set {
    name = "metricsRelistInterval"
    value = "30s"
  }

  set {
    name = "prometheus.url"
    value = "http://prometheus-operated.${helm_release.prometheus_operator.namespace}.svc"
  }

  # PDB
  set {
    name = "podDisruptionBudget.enabled"
    value = "true"
  }

  set {
    name = "podDisruptionBudget.maxUnavailable"
    value = "1"
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

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
    helm_release.prometheus_operator,
  ]
}