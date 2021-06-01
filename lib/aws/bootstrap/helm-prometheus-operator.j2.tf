//locals {
//  prometheus_namespace = "prometheus"
//}
//
//resource "kubernetes_namespace" "prometheus_namespace" {
//  metadata {
//    name = local.prometheus_namespace
//  }
//}
//
//resource "helm_release" "prometheus_operator" {
//  name = "prometheus-operator"
//  chart = "common/charts/prometheus-operator"
//  namespace = local.prometheus_namespace
//  // high timeout because on bootstrap, it's one of the biggest dependencies and on upgrade, it can takes time
//  // to upgrade because of crd and the number of elements it has to deploy
//  timeout = 480
//  atomic = true
//  max_history = 50
//
//  values = [file("chart_values/prometheus_operator.yaml")]
//
//  // avoid fake timestamp on any CRDs updates as takes a long time to be deployed and not needed if not regularly updated
//
//  set {
//    name = "nameOverride"
//    value = "prometheus-operator"
//  }
//
//  set {
//    name = "fullnameOverride"
//    value = "prometheus-operator"
//  }
//
//  # Limits kube-state-metrics
//  set {
//    name = "kube-state-metrics.resources.limits.cpu"
//    value = "100m"
//  }
//
//  set {
//    name = "kube-state-metrics.resources.requests.cpu"
//    value = "20m"
//  }
//
//  set {
//    name = "kube-state-metrics.resources.limits.memory"
//    value = "128Mi"
//  }
//
//  set {
//    name = "kube-state-metrics.resources.requests.memory"
//    value = "128Mi"
//  }
//
//  # Limits prometheus-node-exporter
//  set {
//    name = "prometheus-node-exporter.resources.limits.cpu"
//    value = "20m"
//  }
//
//  set {
//    name = "prometheus-node-exporter.resources.requests.cpu"
//    value = "10m"
//  }
//
//  set {
//    name = "prometheus-node-exporter.resources.limits.memory"
//    value = "32Mi"
//  }
//
//  set {
//    name = "prometheus-node-exporter.resources.requests.memory"
//    value = "32Mi"
//  }
//
//  # Limits kube-state-metrics
//  set {
//    name = "kube-state-metrics.resources.limits.cpu"
//    value = "30m"
//  }
//
//  set {
//    name = "kube-state-metrics.resources.requests.cpu"
//    value = "20m"
//  }
//
//  set {
//    name = "kube-state-metrics.resources.limits.memory"
//    value = "128Mi"
//  }
//
//  set {
//    name = "kube-state-metrics.resources.requests.memory"
//    value = "128Mi"
//  }
//
//  # Limits prometheusOperator
//  set {
//    name = "prometheusOperator.resources.limits.cpu"
//    value = "1"
//  }
//
//  set {
//    name = "prometheusOperator.resources.requests.cpu"
//    value = "500m"
//  }
//
//  set {
//    name = "prometheusOperator.resources.limits.memory"
//    value = "1Gi"
//  }
//
//  set {
//    name = "prometheusOperator.resources.requests.memory"
//    value = "1Gi"
//  }
//
//{% if test_cluster %}
//  set {
//    name = "defaultRules.config"
//    value = "{}"
//  }
//{% endif %}
//
//  depends_on = [
//    aws_eks_cluster.eks_cluster,
//    helm_release.aws_vpc_cni,
//    kubernetes_namespace.prometheus_namespace,
//  ]
//}