resource "helm_release" "q_priority_class" {
  name = "q-priority-class"
  chart = "common/charts/q-priorityclass"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
  ]
}