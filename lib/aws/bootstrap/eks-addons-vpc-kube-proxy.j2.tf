resource "aws_eks_addon" "kube_proxy" {
  cluster_name         = aws_eks_cluster.eks_cluster.name
  addon_name           = "kube-proxy"

  # Pick the recommended version for the k8s version or override if set
  addon_version        = "{{ eks_addon_kube_proxy.version }}"
  resolve_conflicts_on_update = "OVERWRITE"
  resolve_conflicts_on_create = "OVERWRITE"

  tags = local.tags_eks
}
