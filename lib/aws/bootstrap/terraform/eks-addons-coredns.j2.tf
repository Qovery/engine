resource "aws_eks_addon" "aws_coredns" {
  cluster_name = aws_eks_cluster.eks_cluster.name
  addon_name   = "coredns"

  # Pick the recommended version for the k8s version or override if set
  addon_version               = "{{ eks_addon_coredns.version }}"
  resolve_conflicts_on_update = "OVERWRITE"
  resolve_conflicts_on_create = "OVERWRITE"

  tags = local.tags_eks

  # CoreDNS configuration to run on infrastructure nodes
  configuration_values = jsonencode({
    tolerations = [
      {
        key    = "node.qovery.com/infrastructure"
        value  = "true"
        effect = "NoSchedule"
      }
    ]
    replicaCount = {{ eks_addon_coredns.replica_count }}
    resources = {
      limits = {
        {% if eks_addon_coredns.resources.limit_cpu -%}
        cpu    = "{{ eks_addon_coredns.resources.limit_cpu }}"
        {% endif -%}
        {% if eks_addon_coredns.resources.limit_memory -%}
        memory = "{{ eks_addon_coredns.resources.limit_memory }}"
        {% endif -%}
      }
      requests = {
        {% if eks_addon_coredns.resources.request_cpu -%}
        cpu    = "{{ eks_addon_coredns.resources.request_cpu }}"
        {% endif -%}
        {% if eks_addon_coredns.resources.request_memory -%}
        memory = "{{ eks_addon_coredns.resources.request_memory }}"
        {% endif -%}
      }
    }
  })
}
