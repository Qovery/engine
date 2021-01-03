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

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
    helm_release.prometheus_operator,
    helm_release.q_storageclass,
  ]
}