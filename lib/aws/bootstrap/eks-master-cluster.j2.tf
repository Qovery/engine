locals {
  additional_tags = {

  }
}

locals {
  tags_common = {
    ClusterId = var.kubernetes_cluster_id
    ClusterLongId = var.kubernetes_full_cluster_id
    OrganizationId = var.organization_id,
    Region = var.region
    creationDate = time_static.on_cluster_create.rfc3339
    QoveryProduct = "EKS"
    {% if resource_expiration_in_seconds > -1 %}ttl = var.resource_expiration_in_seconds{% endif %}
  }

  tags_eks = merge(
  local.tags_common,
  {
    "Service" = "EKS"
  }
  )
}

resource "time_static" "on_cluster_create" {}

resource "aws_cloudwatch_log_group" "eks_cloudwatch_log_groups" {
  name = "/aws/eks/${var.eks_cloudwatch_log_group}/cluster"
  retention_in_days = 7

  tags = local.tags_eks
}

resource "aws_eks_cluster" "eks_cluster" {
  name            = var.kubernetes_cluster_name
  role_arn        = aws_iam_role.eks_cluster.arn
  version         = var.eks_k8s_versions.masters

  enabled_cluster_log_types = ["api","audit","authenticator","controllerManager","scheduler"]

  vpc_config {
    security_group_ids = [aws_security_group.eks_cluster.id]
    subnet_ids = flatten([
      {% if user_provided_network -%}
      data.aws_subnet.eks_zone_a[*].id,
      data.aws_subnet.eks_zone_b[*].id,
      data.aws_subnet.eks_zone_c[*].id,
      {%- else -%}
      aws_subnet.eks_zone_a[*].id,
      aws_subnet.eks_zone_b[*].id,
      aws_subnet.eks_zone_c[*].id,
      {%- endif %}
      {% if vpc_qovery_network_mode == "WithNatGateways" %}
      aws_subnet.eks_zone_a_public[*].id,
      aws_subnet.eks_zone_b_public[*].id,
      aws_subnet.eks_zone_c_public[*].id,
      {% endif %}
    ])
  }

  tags = local.tags_eks

  # grow this value to reduce random AWS API timeouts
  timeouts {
    create = "60m"
    update = "90m"
    delete = "30m"
  }

  depends_on = [
    aws_iam_role_policy_attachment.eks_cluster_AmazonEKSClusterPolicy,
    aws_iam_role_policy_attachment.eks_cluster_AmazonEKSServicePolicy,
    aws_cloudwatch_log_group.eks_cloudwatch_log_groups,
  ]
}
