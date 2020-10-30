data "aws_availability_zones" "available" {}

locals {
  tags_eks_vpc = merge(
    local.tags_eks,
    {
      Name = "qovery-eks-workers",
      "kubernetes.io/cluster/qovery-${var.eks_cluster_id}" = "shared",
      "kubernetes.io/role/elb" = 1,
    }
  )
}

resource "aws_vpc" "eks" {
  cidr_block = var.vpc_cidr_block
  enable_dns_hostnames = true

  tags = local.tags_eks_vpc
}

resource "aws_subnet" "eks_zone_a" {
  count = length(var.eks_subnets_zone_a)

  availability_zone = data.aws_availability_zones.available.names[0]
  cidr_block = var.eks_subnets_zone_a[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = local.tags_eks_vpc
}

resource "aws_subnet" "eks_zone_b" {
  count = length(var.eks_subnets_zone_b)

  availability_zone = data.aws_availability_zones.available.names[1]
  cidr_block = var.eks_subnets_zone_b[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = local.tags_eks_vpc
}

resource "aws_subnet" "eks_zone_c" {
  count = length(var.eks_subnets_zone_c)

  availability_zone = data.aws_availability_zones.available.names[2]
  cidr_block = var.eks_subnets_zone_c[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = local.tags_eks_vpc
}

resource "aws_internet_gateway" "eks_cluster" {
  vpc_id = aws_vpc.eks.id

  tags = local.tags_eks_vpc
}

resource "aws_route_table" "eks_cluster" {
  vpc_id = aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.eks_cluster.id
  }

  tags = local.tags_eks_vpc
}

resource "aws_route_table_association" "eks_cluster_zone_a" {
  count = length(var.eks_subnets_zone_a)

  subnet_id = aws_subnet.eks_zone_a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster_zone_b" {
  count = length(var.eks_subnets_zone_b)

  subnet_id = aws_subnet.eks_zone_b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster_zone_c" {
  count = length(var.eks_subnets_zone_c)

  subnet_id = aws_subnet.eks_zone_c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}