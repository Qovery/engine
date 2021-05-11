{% if log_history_enabled %}
resource "helm_release" "promtail" {
  name = "promtail"
  chart = "common/charts/promtail"
  namespace = "logging"
  create_namespace = true
  atomic = true
  max_history = 50

  set {
    name = "loki.serviceName"
    value = "loki"
  }

  # It's mandatory to get this class to ensure paused infra will behave properly on restore
  # and logs will always be forwarded (no other pod will preempt)
  set {
    name = "priorityClassName"
    value = "system-node-critical"
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
  ]
}
{% endif %}