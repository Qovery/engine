{% if enable_karpenter %} # TODO PG remove once we are confident that CoreDns addon is ok

resource "aws_eks_addon" "aws_coredns" {
  cluster_name = aws_eks_cluster.eks_cluster.name
  addon_name   = "coredns"

  # Pick the recommended version for the k8s version or override if set
  addon_version     = "{{ eks_addon_coredns.version }}"
  resolve_conflicts = "OVERWRITE"

  tags = local.tags_eks

  {% if enable_karpenter and bootstrap_on_fargate %}
  depends_on = [
    aws_eks_fargate_profile.core-dns
  ]
  {% endif %}
}
{% endif %}