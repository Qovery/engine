data "aws_availability_zones" "available" {}

locals {
  tags_ec2_vpc = merge(
  local.tags_common,
  {
    Name = "qovery-ec2-${var.kubernetes_cluster_id}",
    "kubernetes.io/cluster/qovery-${var.kubernetes_cluster_id}" = "shared",
    "kubernetes.io/role/elb" = 1,
    {% if resource_expiration_in_seconds > 0 %}ttl = var.resource_expiration_in_seconds,{% endif %}
  }
  )

  tags_ec2_vpc_public = merge(
  local.tags_ec2_vpc,
  {
    "Public" = "true"
  }
  )
}

# VPC
resource "aws_vpc" "ec2" {
  cidr_block = var.vpc_cidr_block
  enable_dns_hostnames = true

  tags = local.tags_ec2_vpc
}

# Internet gateway
resource "aws_internet_gateway" "ec2_instance" {
  vpc_id = aws_vpc.ec2.id

  tags = local.tags_ec2_vpc
}