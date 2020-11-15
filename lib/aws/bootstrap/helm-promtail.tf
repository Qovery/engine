resource "helm_release" "promtail" {
  name = "promtail"
  chart = "common/charts/promtail"
  namespace = "logging"
  create_namespace = true
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "loki.serviceName"
    value = "loki"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
  ]
}
