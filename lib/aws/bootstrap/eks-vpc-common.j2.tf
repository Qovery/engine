locals {
  tags_eks_vpc = merge(
  local.tags_common,
  {
    Name = "qovery-eks-workers",
    "kubernetes.io/cluster/qovery-${var.kubernetes_cluster_id}" = "shared",
    "kubernetes.io/role/elb" = 1,
    {% if resource_expiration_in_seconds > -1 %}ttl = var.resource_expiration_in_seconds,{% endif %}
  }
  )

  tags_eks_vpc_public = merge(
  local.tags_eks_vpc,
  {
    "Public" = "true"
  }
  )

  tags_eks_vpc_private = merge(
  local.tags_eks,
  {
    "Public" = "false"
  }
  )
}

{%- if user_provided_network -%}
# VPC
data "aws_vpc" "eks" {
  id = "{{ aws_vpc_eks_id }}"
}

# Internet gateway
# Needed because kubelet are targeting k8s control plan on a public address
# So need a way to reach internet
#data "aws_internet_gateway" "eks_cluster" {
#  filter {
#    name   = "attachment.vpc-id"
#    values = [data.aws_vpc.eks.id]
#  }
#}

{% else %}

# VPC
resource "aws_vpc" "eks" {
  cidr_block = var.vpc_cidr_block
  enable_dns_hostnames = true
  tags = local.tags_eks_vpc
}

{% if aws_enable_vpc_flow_logs %}
# VPC flow logs
resource "aws_flow_log" "eks_vpc_flow_logs" {
  log_destination      = aws_s3_bucket.vpc_flow_logs.arn
  log_destination_type = "s3"
  traffic_type         = "ALL"
  vpc_id               = aws_vpc.eks.id
}
{% endif %}

# Internet gateway
resource "aws_internet_gateway" "eks_cluster" {
  vpc_id = aws_vpc.eks.id

  tags = local.tags_eks_vpc
}
{%- endif -%}
