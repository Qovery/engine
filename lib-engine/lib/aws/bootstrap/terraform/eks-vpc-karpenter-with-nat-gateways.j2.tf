{% if vpc_qovery_network_mode == "WithoutNatGateways" and not user_provided_network and enable_karpenter %}

variable "eks_karpenter_subnets_zone_a_public" {
  description = "EKS public subnets Zone A"
  default     = {{ eks_zone_a_nat_gw_for_fargate_subnet_blocks_public }}
  type        = list(string)
}


# External IPs
resource "aws_eip" "eip_karpenter_zone_a" {
  domain = "vpc"
  tags = local.tags_eks_vpc
}

# Public subnets
resource "aws_subnet" "eks_karpenter_zone_a_public" {
  count = length(var.eks_karpenter_subnets_zone_a_public)

  availability_zone       = var.aws_availability_zones[0]
  cidr_block              = var.eks_karpenter_subnets_zone_a_public[count.index]
  vpc_id                  = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = local.tags_eks_vpc_public
}

# Public Nat gateways
resource "aws_nat_gateway" "eks_karpenter_zone_a_public" {
  count = length(var.eks_karpenter_subnets_zone_a_public)

  allocation_id = aws_eip.eip_karpenter_zone_a.id
  subnet_id     = aws_subnet.eks_karpenter_zone_a_public[count.index].id

  tags = local.tags_eks_vpc_public
}

# Public Routing table
resource "aws_route_table" "eks_karpenter_cluster" {
  vpc_id = aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.eks_cluster.id
  }

  tags = local.tags_eks_vpc_public
}

resource "aws_route_table_association" "eks_karpenter_cluster_zone_a_public" {
  count = length(var.eks_karpenter_subnets_zone_a_public)

  subnet_id      = aws_subnet.eks_karpenter_zone_a_public.*.id[count.index]
  route_table_id = aws_route_table.eks_karpenter_cluster.id
}

# Routing table
resource "aws_route_table" "eks_karpenter_cluster_zone_a_private" {
  count = length(aws_nat_gateway.eks_karpenter_zone_a_public)

  vpc_id = aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_nat_gateway.eks_karpenter_zone_a_public[count.index].id
  }

  tags = local.tags_eks_vpc_private
}

{% endif %}