resource "helm_release" "q_storageclass" {
  name = "q-storageclass"
  chart = "../../../lib/aws/charts/q-storageclass"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  depends_on = [
    aws_eks_cluster.eks_cluster,
  ]
}