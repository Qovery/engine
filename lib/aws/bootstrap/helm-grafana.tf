resource "helm_release" "grafana" {
  name = "grafana"
  chart = "common/charts/grafana"
  namespace = "prometheus"
  atomic = true
  max_history = 50

  values = [file("chart_values/grafana.yaml")]

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.prometheus_operator,
    helm_release.aws_vpc_cni,
  ]
}