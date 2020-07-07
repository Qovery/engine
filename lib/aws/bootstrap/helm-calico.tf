
resource "helm_release" "calico" {
  name = "calico"
  chart = "../../../lib/aws/charts/calico"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  depends_on = [aws_eks_cluster.eks_cluster]
}