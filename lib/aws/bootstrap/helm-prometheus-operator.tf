resource "helm_release" "prometheus_operator" {
  name = "prometheus-operator"
  chart = "common/charts/prometheus-operator"
  namespace = "prometheus"
  create_namespace = true
  atomic = true
  max_history = 50

  values = [file("chart_values/prometheus_operator.yaml")]

  set {
    name = "nameOverride"
    value = "prometheus-operator"
  }

  set {
    name = "fullnameOverride"
    value = "prometheus-operator"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
  ]
}