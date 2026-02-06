{% if eks_pod_identity_addon_enabled -%}
resource "aws_eks_addon" "eks_pod_identity_agent" {
  cluster_name                = aws_eks_cluster.eks_cluster.name
  addon_name                  = "eks-pod-identity-agent"
  addon_version               = "{{ eks_addon_pod_identity.version }}"
  resolve_conflicts_on_update = "OVERWRITE"
  resolve_conflicts_on_create = "OVERWRITE"
  tags                        = local.tags_eks
}
{% endif -%}
