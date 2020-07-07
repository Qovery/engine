//resource "helm_release" "externaldns" {
//  name = "externaldns"
//  chart = "../../../lib/common/charts/externaldns"
//  namespace = "kube-system"
//  create_namespace = true
//  atomic = true
//  max_history = 50
//
//  values = [file("chart_values/external-dns.yaml")]
//
//  depends_on = [aws_eks_cluster.eks_cluster]
//}