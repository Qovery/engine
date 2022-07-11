locals {
  tags_eks_vpc = merge(
  local.tags_common,
  {
    Name = "qovery-eks-workers",
    "kubernetes.io/cluster/qovery-${var.kubernetes_cluster_id}" = "shared",
    "kubernetes.io/role/elb" = 1,
    {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds,{% endif %}
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
resource "aws_internet_gateway" "eks_cluster" {
  vpc_id = data.aws_vpc.eks.id

  tags = local.tags_eks_vpc
}

{% else %}

resource "aws_vpc" "eks" {
  cidr_block = var.vpc_cidr_block
  enable_dns_hostnames = true
  tags = local.tags_eks_vpc
}

# Internet gateway
resource "aws_internet_gateway" "eks_cluster" {
  vpc_id = aws_vpc.eks.id

  tags = local.tags_eks_vpc
}
{%- endif -%}
