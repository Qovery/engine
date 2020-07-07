//resource "helm_release" "cert-manager" {
//  name = "cert-manager"
//  chart = "../../../lib/common/charts/cert-manager"
//  namespace = "cert-manager"
//  create_namespace = true
//  atomic = true
//  max_history = 50
//
//  values = [file("chart_values/cert-manager.yaml")]
//
//  set {
//    name = "version"
//    value = "0.15.2"
//  }
//
//  set {
//    name = "installCRDs"
//    value = "true"
//  }
//
//  depends_on = [aws_eks_cluster.eks_cluster]
//}