{% if vpc_qovery_network_mode == "WithoutNatGateways" %}
# Public subnets
resource "aws_subnet" "eks_zone_a" {
  count = length(var.eks_subnets_zone_a_private)

  availability_zone = data.aws_availability_zones.available.names[0]
  cidr_block = var.eks_subnets_zone_a_private[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = local.tags_eks_vpc
}

resource "aws_subnet" "eks_zone_b" {
  count = length(var.eks_subnets_zone_b_private)

  availability_zone = data.aws_availability_zones.available.names[1]
  cidr_block = var.eks_subnets_zone_b_private[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = local.tags_eks_vpc
}

resource "aws_subnet" "eks_zone_c" {
  count = length(var.eks_subnets_zone_c_private)

  availability_zone = data.aws_availability_zones.available.names[2]
  cidr_block = var.eks_subnets_zone_c_private[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

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
  count = length(var.eks_subnets_zone_a_private)

  subnet_id = aws_subnet.eks_zone_a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster_zone_b" {
  count = length(var.eks_subnets_zone_b_private)

  subnet_id = aws_subnet.eks_zone_b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster_zone_c" {
  count = length(var.eks_subnets_zone_c_private)

  subnet_id = aws_subnet.eks_zone_c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}
{% endif %}