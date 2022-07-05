{% if vpc_qovery_network_mode == "WithoutNatGateways" and not user_provided_network %}

variable "eks_subnets_zone_a_private" {
  description = "EKS private subnets Zone A"
  default = {{ eks_zone_a_subnet_blocks_private }}
  type = list(string)
}

variable "eks_subnets_zone_b_private" {
  description = "EKS private subnets Zone B"
  default = {{ eks_zone_b_subnet_blocks_private }}
  type = list(string)
}

variable "eks_subnets_zone_c_private" {
  description = "EKS private subnets Zone C"
  default = {{ eks_zone_c_subnet_blocks_private }}
  type = list(string)
}

# Public subnets
resource "aws_subnet" "eks_zone_a" {
  count = length(var.eks_subnets_zone_a_private)

  availability_zone = var.aws_availability_zones[0]
  cidr_block = var.eks_subnets_zone_a_private[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = local.tags_eks_vpc
}

resource "aws_subnet" "eks_zone_b" {
  count = length(var.eks_subnets_zone_b_private)

  availability_zone = var.aws_availability_zones[1]
  cidr_block = var.eks_subnets_zone_b_private[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = local.tags_eks_vpc
}

resource "aws_subnet" "eks_zone_c" {
  count = length(var.eks_subnets_zone_c_private)

  availability_zone = var.aws_availability_zones[2]
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

  // todo(pmavro): add tests for it when it will be available in the SDK
  {% for route in vpc_custom_routing_table %}
  route {
    cidr_block = "{{ route.destination }}"
    gateway_id = "{{ route.target }}"
  }
  {% endfor %}

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