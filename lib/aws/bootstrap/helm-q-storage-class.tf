resource "helm_release" "q_storageclass" {
  name = "q-storageclass"
  chart = "charts/q-storageclass"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
  ]
}