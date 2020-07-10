resource "helm_release" "grafana" {
  name = "grafana"
  chart = "../../../lib/common/charts/grafana"
  namespace = "prometheus"
  atomic = true
  max_history = 50

  values = [file("chart_values/grafana.yaml")]

  depends_on = [aws_eks_cluster.eks_cluster, helm_release.prometheus-operator]
}