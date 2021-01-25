locals {
  additional_tags = {

  }
}

locals {
  tags_eks = {
    ClusterId = var.kubernetes_cluster_id,
    ClusterName = var.kubernetes_cluster_name,
    Region = var.region
    {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds{% endif %}
  }
}

resource "aws_cloudwatch_log_group" "eks_cloudwatch_log_group" {
  name = var.eks_cloudwatch_log_group
  retention_in_days = 7

  tags = local.tags_eks
}

resource "aws_eks_cluster" "eks_cluster" {
  name            = "qovery-${var.kubernetes_cluster_id}"
  role_arn        = aws_iam_role.eks_cluster.arn
  version         = var.eks_k8s_versions.masters

  enabled_cluster_log_types = ["api","audit","authenticator","controllerManager","scheduler"]

  vpc_config {
    security_group_ids = [aws_security_group.eks_cluster.id]
    subnet_ids = flatten([aws_subnet.eks_zone_a.*.id, aws_subnet.eks_zone_b.*.id,aws_subnet.eks_zone_c.*.id])
  }

  tags = local.tags_eks

  depends_on = [
    aws_iam_role_policy_attachment.eks_cluster_AmazonEKSClusterPolicy,
    aws_iam_role_policy_attachment.eks_cluster_AmazonEKSServicePolicy,
    aws_cloudwatch_log_group.eks_cloudwatch_log_group,
  ]
}
