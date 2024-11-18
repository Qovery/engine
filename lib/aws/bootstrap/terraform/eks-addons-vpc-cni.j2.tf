resource "aws_eks_addon" "vpc_cni" {
  cluster_name         = aws_eks_cluster.eks_cluster.name
  addon_name           = "vpc-cni"

  # Pick the recommended version for the k8s version or override if set
  addon_version        = "{{ eks_addon_vpc_cni.version }}"
  resolve_conflicts_on_update = "OVERWRITE"
  resolve_conflicts_on_create = "OVERWRITE"

  # Get configuration fields: `aws eks describe-addon-configuration --addon-name vpc-cni --addon-version`
  # jq .configurationSchema --raw-output | jq .definitions
  # Note: it seems to miss some ENV VARs presents / supported on the plugin: CF https://github.com/aws/amazon-vpc-cni-k8s
  configuration_values = jsonencode({
    "enableNetworkPolicy": "true"
  })

  tags = local.tags_eks
}
