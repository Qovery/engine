locals {
  additional_tags = {

  }
}

locals {
  tags_common = {
    ClusterId = var.kubernetes_cluster_id
    ClusterLongId = var.kubernetes_cluster_long_id
    OrganizationId = var.organization_id,
    OrganizationLongId = var.organization_long_id,
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

resource "aws_cloudwatch_log_group" "cloudwatch_eks_log_groups" {
  name = var.cloudwatch_eks_log_groups
  retention_in_days = var.aws_cloudwatch_eks_logs_retention_days

  tags = local.tags_eks
}

variable "public_access_cidrs" {
  type    = list(string)
  default = [
    {%- for cidr in public_access_cidrs -%}
    "{{ cidr }}",
    {%- endfor -%}
  ]
}

resource "aws_eks_cluster" "eks_cluster" {
  name            = var.kubernetes_cluster_name
  role_arn        = aws_iam_role.eks_cluster.arn
  version         = var.eks_k8s_versions.masters

  enabled_cluster_log_types = ["api","audit","authenticator","controllerManager","scheduler"]
  upgrade_policy {
      support_type = "STANDARD"
  }

  access_config {
    authentication_mode = "API_AND_CONFIG_MAP"
    bootstrap_cluster_creator_admin_permissions = false
  }

{% if aws_eks_encrypt_secrets_kms_key_arn -%}
  encryption_config {
      provider {
        key_arn = "{{ aws_eks_encrypt_secrets_kms_key_arn }}"
      }
      resources = ["secrets"]
  }
{%- endif %}

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
    public_access_cidrs = var.public_access_cidrs
    endpoint_private_access = true
    endpoint_public_access = true
  }

  tags = local.tags_eks

  # grow this value to reduce random AWS API timeouts
  timeouts {
    create = "60m"
    update = "90m"
    delete = "30m"
  }


  // To avoid unnecessary updates to the access_config block for old clusters.
  // Otherwise it forces the re-creation of the cluster.
  lifecycle {
    ignore_changes = [
      access_config[0].bootstrap_cluster_creator_admin_permissions,
    ]
  }


  depends_on = [
    aws_iam_role_policy_attachment.eks_cluster_AmazonEKSClusterPolicy,
    aws_iam_role_policy_attachment.eks_cluster_AmazonEKSServicePolicy,
    aws_cloudwatch_log_group.cloudwatch_eks_log_groups,
  ]
}
