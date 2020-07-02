resource "helm_release" "prometheus-operator" {
  name = "prometheus-operator"
  chart = "../../../lib/common/charts/prometheus-operator"
  namespace = "prometheus"
  create_namespace = true
  atomic = true
  max_history = 50

  values = ["chart_values/prometheus_operator.yaml"]

  depends_on = [aws_eks_cluster.eks_cluster]
}