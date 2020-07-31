resource "helm_release" "externaldns" {
  name = "externaldns"
  chart = "common/charts/external-dns"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  values = [file("chart_values/external-dns.yaml")]

  depends_on = [aws_eks_cluster.eks_cluster]
}