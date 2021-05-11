{% if log_history_enabled or metrics_history_enabled %}
locals {
  cloudflare_datasources = <<DATASOURCES
datasources:
  datasources.yaml:
    apiVersion: 1
    datasources:
      - name: Prometheus
        type: prometheus
        url: "http://prometheus-operator-prometheus:9090"
        access: proxy
        isDefault: true
      - name: PromLoki
        type: prometheus
        url: "http://${helm_release.loki.name}.${helm_release.loki.namespace}.svc:3100/loki"
        access: proxy
        isDefault: false
      - name: Loki
        type: loki
        url: "http://${helm_release.loki.name}.${helm_release.loki.namespace}.svc:3100"
DATASOURCES
}

resource "helm_release" "grafana" {
  name = "grafana"
  chart = "common/charts/grafana"
  namespace = "prometheus"
  atomic = true
  max_history = 50

  values = [
    file("chart_values/grafana.yaml"),
    local.cloudflare_datasources,
  ]

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
    {% if metrics_history_enabled %}
    helm_release.prometheus_operator,
    {% endif %}
    helm_release.q_storageclass,
  ]
}
{% endif %}