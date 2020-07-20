resource "helm_release" "cert_manager" {
  name = "cert-manager"
  chart = "common/bootstrap/charts/cert-manager"
  namespace = "cert-manager"
  create_namespace = true
  atomic = true
  max_history = 50

  values = [file("chart_values/cert-manager.yaml")]

  set {
    name = "version"
    value = "0.15.2"
  }

  set {
    name = "installCRDs"
    value = "true"
  }

  set {
    name = "replicaCount"
    value = "2"
  }

  set {
    name = "podDnsPolicy"
    value = "None"
  }

  set {
    name = "podDnsConfig.nameservers"
    value = "{1.1.1.1,8.8.8.8}"
  }

  set {
    name = "prometheus.servicemonitor.enabled"
    value = "true"
  }

  set {
    name = "prometheus.servicemonitor.prometheusInstance"
    value = "qovery"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.prometheus_operator
  ]
}