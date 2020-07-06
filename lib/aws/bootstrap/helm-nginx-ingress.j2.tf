resource "helm_release" "nginx-ingress" {
  name = "nginx-ingress"
  chart = "../../../lib/common/charts/nginx-ingress"
  namespace = "nginx-ingress"
  create_namespace = true
  atomic = true
  max_history = 50

  values = [file("chart_values/nginx-ingress.yaml")]

  depends_on = [aws_eks_cluster.eks_cluster, helm_release.prometheus-operator]
}