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
//    name = "image.tag"
//    value = "0.14.3"
//  }
//
//  set {
//    name = "webhook.imag.tag"
//    value = "0.14.3"
//  }
//
//  set {
//    name = "cainjector.image.tag"
//    value = "0.14.3"
//  }
//
//  depends_on = [aws_eks_cluster.eks_cluster]
//}