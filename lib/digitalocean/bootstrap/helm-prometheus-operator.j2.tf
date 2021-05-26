{% if metrics_history_enabled %}
resource "helm_release" "prometheus_operator" {
  name = "prometheus-operator"
  chart = "common/charts/prometheus-operator"
  namespace = "prometheus"
  create_namespace = true
  atomic = true
  max_history = 50

  values = [file("chart_values/prometheus_operator.yaml")]

  // avoid fake timestamp on any CRDs updates as takes a long time to be deployed and not needed if not regularly updated

  set {
    name = "nameOverride"
    value = "prometheus-operator"
  }

  set {
    name = "fullnameOverride"
    value = "prometheus-operator"
  }

  # Limits kube-state-metrics
  set {
    name = "kube-state-metrics.resources.limits.cpu"
    value = "100m"
  }

  set {
    name = "kube-state-metrics.resources.requests.cpu"
    value = "20m"
  }

  set {
    name = "kube-state-metrics.resources.limits.memory"
    value = "128Mi"
  }

  set {
    name = "kube-state-metrics.resources.requests.memory"
    value = "128Mi"
  }

  # Limits prometheus-node-exporter
  set {
    name = "prometheus-node-exporter.resources.limits.cpu"
    value = "20m"
  }

  set {
    name = "prometheus-node-exporter.resources.requests.cpu"
    value = "10m"
  }

  set {
    name = "prometheus-node-exporter.resources.limits.memory"
    value = "32Mi"
  }

  set {
    name = "prometheus-node-exporter.resources.requests.memory"
    value = "32Mi"
  }

  # Limits kube-state-metrics
  set {
    name = "kube-state-metrics.resources.limits.cpu"
    value = "30m"
  }

  set {
    name = "kube-state-metrics.resources.requests.cpu"
    value = "20m"
  }

  set {
    name = "kube-state-metrics.resources.limits.memory"
    value = "128Mi"
  }

  set {
    name = "kube-state-metrics.resources.requests.memory"
    value = "128Mi"
  }

  # Limits prometheusOperator
  set {
    name = "prometheusOperator.resources.limits.cpu"
    value = "500m"
  }

  set {
    name = "prometheusOperator.resources.requests.cpu"
    value = "500m"
  }

  set {
    name = "prometheusOperator.resources.limits.memory"
    value = "512Mi"
  }

  set {
    name = "prometheusOperator.resources.requests.memory"
    value = "512Mi"
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
    helm_release.q_storageclass,
  ]

{% if test_cluster %}
  set {
    name = "defaultRules.config"
    value = "{}"
  }
{% endif %}
}
{% endif %}